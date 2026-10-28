//! File probing, decoding and band-limited resampling run outside the webview
//! and audio callback. Only bounded, already routed frames cross into rendering.
use super::{
    GripFilter, RATE,
    stream::{FILE_WATERMARK, PcmStream, routed_frame},
};
use rubato::{FftFixedInOut, Resampler};
use serde::Serialize;
use std::{
    fs::File,
    path::Path,
    sync::{Arc, atomic::Ordering},
    thread,
    time::Duration,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions},
    errors::Error,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::{Time, TimeBase},
};

const MAX_SECONDS: f64 = 3600.0;
const MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFileInfo {
    pub path: String,
    pub name: String,
    pub duration: f64,
    pub channels: usize,
    pub sample_rate: u32,
}
struct Reader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    id: u32,
    rate: u32,
    channels: usize,
    frames: Option<u64>,
    time_base: TimeBase,
}
fn open(path: &Path) -> Result<Reader, String> {
    let file = File::open(path).map_err(|e| format!("Could not open audio file: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BYTES {
        return Err("Choose an audio file up to 512 MB.".into());
    }
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let result = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Unsupported audio file: {e}"))?;
    let format = result.format;
    let track = format
        .default_track()
        .ok_or("The file has no audio track.")?;
    let rate = track
        .codec_params
        .sample_rate
        .ok_or("Audio sample rate is missing.")?;
    let channels = track
        .codec_params
        .channels
        .ok_or("Audio channels are missing.")?
        .count();
    if !(8000..=192000).contains(&rate) || !(1..=2).contains(&channels) {
        return Err("Choose mono or stereo audio between 8 and 192 kHz.".into());
    }
    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("This audio codec is not supported: {e}"))?;
    let reader = Reader {
        id: track.id,
        rate,
        channels,
        frames: track.codec_params.n_frames,
        time_base: track
            .codec_params
            .time_base
            .unwrap_or(TimeBase::new(1, rate)),
        format,
        decoder,
    };
    Ok(reader)
}
fn finite(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}
fn seconds(time: Time) -> f64 {
    time.seconds as f64 + time.frac
}
fn next_packet(reader: &mut Reader) -> Result<Option<symphonia::core::formats::Packet>, String> {
    loop {
        match reader.format.next_packet() {
            Ok(packet) if packet.track_id() == reader.id => return Ok(Some(packet)),
            Ok(_) => continue,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => return Err(format!("Could not read audio: {e}")),
        }
    }
}
pub fn inspect_file(path: &Path) -> Result<AudioFileInfo, String> {
    let mut reader = open(path)?;
    let duration = if let Some(frames) = reader.frames {
        frames as f64 / reader.rate as f64
    } else {
        let mut end = 0;
        let mut packets = 0;
        while let Some(packet) = next_packet(&mut reader)? {
            end = end.max(packet.ts.saturating_add(packet.dur));
            packets += 1;
            if packets > 2_000_000 || seconds(reader.time_base.calc_time(end)) > MAX_SECONDS {
                return Err("Choose a track up to one hour long.".into());
            }
        }
        seconds(reader.time_base.calc_time(end))
    };
    if !duration.is_finite() || duration <= 0.0 || duration > MAX_SECONDS {
        return Err("Choose a track up to one hour long.".into());
    }
    Ok(AudioFileInfo {
        path: path.to_string_lossy().into_owned(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        duration,
        channels: reader.channels,
        sample_rate: reader.rate,
    })
}

struct Converter {
    fft: Option<FftFixedInOut<f32>>,
    pending: [Vec<f32>; 2],
    rate: u32,
    skip: usize,
    input_frames: u64,
    output_frames: u64,
    filters: [GripFilter; 2],
    direct: bool,
    mode: String,
}
impl Converter {
    fn new(rate: u32, direct: bool) -> Result<Self, String> {
        let fft = if rate == RATE {
            None
        } else {
            Some(
                FftFixedInOut::<f32>::new(rate as usize, RATE as usize, 1024, 2)
                    .map_err(|e| e.to_string())?,
            )
        };
        let skip = fft.as_ref().map_or(0, Resampler::output_delay);
        Ok(Self {
            fft,
            pending: [Vec::new(), Vec::new()],
            rate,
            skip,
            input_frames: 0,
            output_frames: 0,
            filters: Default::default(),
            direct,
            mode: "music".into(),
        })
    }
    fn emit(&mut self, output: &[Vec<f32>], limit: u64, stream: &PcmStream) -> Result<(), String> {
        for (&left, &right) in output[0].iter().zip(&output[1]) {
            if self.skip > 0 {
                self.skip -= 1;
                continue;
            }
            if self.output_frames >= limit {
                break;
            }
            if stream.canceled.load(Ordering::Acquire) {
                return Ok(());
            }
            let input = [left * 0.85, right * 0.85];
            let grip = if self.direct {
                input
            } else {
                [
                    self.filters[0].frame_mode(input[0], &self.mode),
                    self.filters[1].frame_mode(input[1], &self.mode),
                ]
            };
            let frame = routed_frame(input, grip, stream);
            while stream.queue.len() >= FILE_WATERMARK {
                if stream.canceled.load(Ordering::Acquire) {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(3));
            }
            stream
                .queue
                .push(frame)
                .map_err(|_| "Audio queue overflow".to_string())?;
            self.output_frames += 1;
            if stream.queue.len() >= RATE as usize / 10 {
                stream.ready.store(true, Ordering::Release);
            }
        }
        Ok(())
    }
    fn push(&mut self, input: &[f32], channels: usize, stream: &PcmStream) -> Result<(), String> {
        self.input_frames += (input.len() / channels) as u64;
        if self.fft.is_none() {
            let output = [
                input.chunks_exact(channels).map(|f| finite(f[0])).collect(),
                input
                    .chunks_exact(channels)
                    .map(|f| finite(f[channels - 1]))
                    .collect(),
            ];
            return self.emit(&output, u64::MAX, stream);
        }
        for frame in input.chunks_exact(channels) {
            self.pending[0].push(finite(frame[0]));
            self.pending[1].push(finite(frame[channels - 1]));
        }
        let needed = self.fft.as_ref().unwrap().input_frames_next();
        while self.pending[0].len() >= needed && !stream.canceled.load(Ordering::Acquire) {
            let block = [&self.pending[0][..needed], &self.pending[1][..needed]];
            let output = self
                .fft
                .as_mut()
                .unwrap()
                .process(&block, None)
                .map_err(|e| e.to_string())?;
            for channel in &mut self.pending {
                channel.drain(..needed);
            }
            self.emit(&output, u64::MAX, stream)?;
        }
        Ok(())
    }
    fn finish(&mut self, stream: &PcmStream) -> Result<(), String> {
        let target = (self.input_frames as f64 * RATE as f64 / self.rate as f64).round() as u64;
        if self.fft.is_some() {
            let block = [self.pending[0].as_slice(), self.pending[1].as_slice()];
            let output = self
                .fft
                .as_mut()
                .unwrap()
                .process_partial(Some(&block), None)
                .map_err(|e| e.to_string())?;
            self.emit(&output, target, stream)?;
            for _ in 0..8 {
                if self.output_frames >= target || stream.canceled.load(Ordering::Acquire) {
                    break;
                }
                let output = self
                    .fft
                    .as_mut()
                    .unwrap()
                    .process_partial::<&[f32]>(None, None)
                    .map_err(|e| e.to_string())?;
                self.emit(&output, target, stream)?;
            }
            if !stream.canceled.load(Ordering::Acquire) && self.output_frames != target {
                return Err("Could not finish audio resampling.".into());
            }
        }
        stream.total.store(self.output_frames, Ordering::Release);
        stream.eof.store(true, Ordering::Release);
        stream.ready.store(true, Ordering::Release);
        Ok(())
    }
}
fn decode(path: &Path, start: f64, kind: &str, stream: &PcmStream) -> Result<(), String> {
    let mut reader = open(path)?;
    let mut at = 0u64;
    if start > 0.0 {
        match reader.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::from(start),
                track_id: Some(reader.id),
            },
        ) {
            Ok(seek) => {
                at = (seconds(reader.time_base.calc_time(seek.actual_ts)) * reader.rate as f64)
                    .round() as u64;
                reader.decoder.reset();
            }
            Err(_) => reader = open(path)?,
        }
    }
    let skip_to = (start * reader.rate as f64).round() as u64;
    let mut converter = Converter::new(reader.rate, kind == "haptic")?;
    converter.mode = kind.into();
    while !stream.canceled.load(Ordering::Acquire) {
        let Some(packet) = next_packet(&mut reader)? else {
            break;
        };
        let decoded = reader
            .decoder
            .decode(&packet)
            .map_err(|e| format!("Could not decode audio: {e}"))?;
        if decoded.spec().rate != reader.rate || decoded.spec().channels.count() != reader.channels
        {
            return Err("Audio format changed inside the track.".into());
        }
        if decoded.capacity() > reader.rate as usize * 2 {
            return Err("Audio packet is too large.".into());
        }
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        samples.copy_interleaved_ref(decoded);
        let frames = samples.samples().len() / reader.channels;
        let discard = skip_to.saturating_sub(at).min(frames as u64) as usize;
        at += frames as u64;
        if at > reader.rate as u64 * MAX_SECONDS as u64 + reader.rate as u64 {
            return Err("Track exceeds one hour.".into());
        }
        converter.push(
            &samples.samples()[discard * reader.channels..],
            reader.channels,
            stream,
        )?;
    }
    if !stream.canceled.load(Ordering::Acquire) {
        converter.finish(stream)?;
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn spawn(
    path: String,
    start: f64,
    direct: bool,
    stream: Arc<PcmStream>,
) -> Result<(), String> {
    spawn_conversion(
        path,
        start,
        if direct { "haptic" } else { "music" }.into(),
        stream,
    )
}
pub(super) fn spawn_conversion(
    path: String,
    start: f64,
    kind: String,
    stream: Arc<PcmStream>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("controller-file-decoder".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                decode(Path::new(&path), start, &kind, &stream)
            }))
            .unwrap_or_else(|_| Err("The audio decoder stopped unexpectedly.".into()));
            if let Err(error) = result
                && !stream.canceled.load(Ordering::Acquire)
            {
                if let Ok(mut slot) = stream.error.lock() {
                    *slot = Some(error);
                }
                stream.eof.store(true, Ordering::Release);
                stream.ready.store(true, Ordering::Release);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn wav(rate: u32, frames: u32) -> Fixture {
        use std::io::Write;
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "controller-audio-{}-{}.wav",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let bytes = frames * 4;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(bytes + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&rate.to_le_bytes()).unwrap();
        file.write_all(&(rate * 4).to_le_bytes()).unwrap();
        file.write_all(&4u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();
        for i in 0..frames {
            let left =
                ((i as f32 * std::f32::consts::TAU * 120.0 / rate as f32).sin() * 16000.0) as i16;
            file.write_all(&left.to_le_bytes()).unwrap();
            file.write_all(&0i16.to_le_bytes()).unwrap();
        }
        Fixture(path)
    }
    #[test]
    fn native_decoder_resamples_without_losing_stereo_or_track_tail() {
        for rate in [22050, 44100, 48000, 96000] {
            let fixture = wav(rate, rate / 5);
            let info = inspect_file(&fixture.0).unwrap();
            assert_eq!(info.channels, 2);
            assert!((info.duration - 0.2).abs() < 0.00001);
            let stream = PcmStream::new(12, "grips", true, 9600);
            decode(&fixture.0, 0.0, "haptic", &stream).unwrap();
            assert_eq!(stream.queue.len(), 9600, "incorrect length at {rate} Hz");
            assert!(stream.eof.load(Ordering::Acquire));
            let mut peak = 0.0f32;
            while let Some(frame) = stream.queue.pop() {
                assert_eq!(frame[0], 0.0);
                assert_eq!(frame[1], 0.0);
                assert_eq!(frame[3], 0.0);
                assert!(frame.iter().all(|x| x.is_finite()));
                peak = peak.max(frame[2].abs());
            }
            assert!(
                peak > 0.3 && peak < 0.6,
                "bad amplitude at {rate} Hz: {peak}"
            );
        }
    }
    #[test]
    fn native_seek_preserves_remaining_duration() {
        let fixture = wav(44100, 8820);
        let stream = PcmStream::new(13, "both", true, 4800);
        decode(&fixture.0, 0.1, "music", &stream).unwrap();
        assert_eq!(stream.queue.len(), 4800);
        assert_eq!(stream.total.load(Ordering::Relaxed), 4800);
        assert!(stream.queue.pop().unwrap()[3].abs() < 0.00001);
    }
    #[test]
    fn native_worker_is_bounded_and_cancels_while_paused() {
        let fixture = wav(48000, 144000);
        let stream = Arc::new(PcmStream::new(14, "grips", true, 144000));
        stream.paused.store(true, Ordering::Release);
        spawn(
            fixture.0.to_string_lossy().into_owned(),
            0.0,
            false,
            stream.clone(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while stream.queue.len() < FILE_WATERMARK && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(stream.queue.len(), FILE_WATERMARK);
        assert_eq!(stream.queue.capacity(), FILE_WATERMARK);
        assert_eq!(stream.consumed.load(Ordering::Relaxed), 0);
        stream.canceled.store(true, Ordering::Release);
        while Arc::strong_count(&stream) > 1 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(Arc::strong_count(&stream), 1, "decoder did not cancel");
    }
}
