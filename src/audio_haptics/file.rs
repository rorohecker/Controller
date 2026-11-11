//! File probing, decoding and band-limited resampling run outside the webview
//! and audio callback. Only bounded, already routed frames cross into rendering.
use super::{
    GripFilter, RATE, capture_math,
    dsp::{ConvMode, OnsetDetector, ThreeBand},
    io_runtime::Permit,
    stream::{FILE_WATERMARK, PcmStream, routed_frame},
};
use rubato::{FftFixedInOut, Resampler};
use serde::Serialize;
use std::{
    fs::File,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
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
const MAX_CHANNELS: usize = 32;
/// Consecutive undecodable packets tolerated before the track is abandoned.
const MAX_DECODE_FAILURES: u32 = 8;
const MAX_STREAM_RESETS: u32 = 64;
static FILE_WORKERS: AtomicU32 = AtomicU32::new(0);
const MAX_FILE_WORKERS: u32 = 3;
const DEFAULT_BAND_GAINS: [f32; 3] = [1.0, 1.0, 0.6];

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
    resets: u32,
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
    // Some containers omit the channel layout. 0 means "learn it from the first
    // decoded packet"; layouts above stereo are folded down by `capture_math::stereo`.
    let channels = track.codec_params.channels.map_or(0, |c| c.count());
    if !(8000..=192000).contains(&rate) || channels > MAX_CHANNELS {
        return Err("Choose audio between 8 and 192 kHz with up to 32 channels.".into());
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
        resets: 0,
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
            // Chained streams (for example chained Ogg) announce a new logical
            // stream. Rebuild the decoder for it and keep reading.
            Err(Error::ResetRequired) => rebuild_decoder(reader)?,
            Err(e) => return Err(format!("Could not read audio: {e}")),
        }
    }
}
fn rebuild_decoder(reader: &mut Reader) -> Result<(), String> {
    reader.resets += 1;
    if reader.resets > MAX_STREAM_RESETS {
        return Err("The audio file changes streams too often.".into());
    }
    let params = reader
        .format
        .default_track()
        .map(|t| (t.id, t.codec_params.clone()));
    if let Some((id, params)) = params
        && let Ok(decoder) =
            symphonia::default::get_codecs().make(&params, &DecoderOptions::default())
    {
        reader.id = id;
        reader.decoder = decoder;
    } else {
        reader.decoder.reset();
    }
    Ok(())
}
/// Decode packets until one yields a channel layout (for files that omit it).
fn learn_channels(reader: &mut Reader) -> Result<(), String> {
    for _ in 0..64 {
        let Some(packet) = next_packet(reader)? else {
            break;
        };
        if let Ok(decoded) = reader.decoder.decode(&packet) {
            reader.channels = decoded.spec().channels.count();
            break;
        }
    }
    if reader.channels == 0 || reader.channels > MAX_CHANNELS {
        return Err("Audio channels are missing.".into());
    }
    Ok(())
}
pub fn inspect_file(path: &Path) -> Result<AudioFileInfo, String> {
    let mut reader = open(path)?;
    if reader.channels == 0 {
        learn_channels(&mut reader)?;
        // Learning consumed packets; short files would otherwise measure 0 s.
        let learned = reader.channels;
        reader = open(path)?;
        reader.channels = learned;
    }
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

/// Split `bands:1.0,0.8,0.5` style kinds into a mode and per-band gains.
fn parse_kind(kind: &str) -> (ConvMode, [f32; 3]) {
    let (name, params) = kind.split_once(':').unwrap_or((kind, ""));
    let mut gains = DEFAULT_BAND_GAINS;
    for (slot, value) in gains.iter_mut().zip(params.split(',')) {
        if let Ok(gain) = value.trim().parse::<f32>()
            && gain.is_finite()
        {
            *slot = gain.clamp(0.0, 2.0);
        }
    }
    (ConvMode::parse(name), gains)
}
/// Content types `play_file` and `preview_envelope` accept.
pub fn valid_kind(kind: &str) -> bool {
    let (name, params) = kind.split_once(':').unwrap_or((kind, ""));
    matches!(
        name,
        "music" | "haptic" | "bass" | "impact" | "texture" | "beats" | "bands"
    ) && (params.is_empty() || name == "bands")
        && params.split(',').count() <= 3
        && params
            .split(',')
            .all(|v| v.is_empty() || v.trim().parse::<f32>().is_ok_and(f32::is_finite))
}
fn pair(frame: &[f32], channels: usize) -> [f32; 2] {
    if channels > 2 {
        capture_math::stereo(frame, false)
    } else {
        [finite(frame[0]), finite(frame[channels - 1])]
    }
}

/// Stereo-in, stereo-out grip conversion for one content type.
struct GripDsp {
    filters: [GripFilter; 2],
    bands: [ThreeBand; 2],
    onset: OnsetDetector,
    direct: bool,
    mode: ConvMode,
    gains: [f32; 3],
}
impl GripDsp {
    fn new(kind: &str) -> Self {
        let (mode, gains) = parse_kind(kind);
        Self {
            filters: Default::default(),
            bands: Default::default(),
            onset: OnsetDetector::new(RATE),
            direct: kind == "haptic",
            mode,
            gains,
        }
    }
    fn frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        if self.direct {
            return input;
        }
        match self.mode {
            ConvMode::Bands => [
                self.bands[0].frame(input[0], RATE as f32, self.gains),
                self.bands[1].frame(input[1], RATE as f32, self.gains),
            ],
            ConvMode::Beats => {
                let pulse = self.onset.push((input[0] + input[1]) * 0.5);
                [
                    (self.filters[0].frame_conv(input[0], ConvMode::Music) + pulse)
                        .clamp(-1.0, 1.0),
                    (self.filters[1].frame_conv(input[1], ConvMode::Music) + pulse)
                        .clamp(-1.0, 1.0),
                ]
            }
            mode => [
                self.filters[0].frame_conv(input[0], mode),
                self.filters[1].frame_conv(input[1], mode),
            ],
        }
    }
}

struct Converter {
    fft: Option<FftFixedInOut<f32>>,
    pending: [Vec<f32>; 2],
    rate: u32,
    skip: usize,
    input_frames: u64,
    output_frames: u64,
    dsp: GripDsp,
}
impl Converter {
    fn new(rate: u32, kind: &str) -> Result<Self, String> {
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
            dsp: GripDsp::new(kind),
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
            let grip = self.dsp.frame(input);
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
        if channels == 0 {
            return Err("Audio channels are missing.".into());
        }
        self.input_frames += (input.len() / channels) as u64;
        if self.fft.is_none() {
            let output = [
                input
                    .chunks_exact(channels)
                    .map(|f| pair(f, channels)[0])
                    .collect(),
                input
                    .chunks_exact(channels)
                    .map(|f| pair(f, channels)[1])
                    .collect(),
            ];
            return self.emit(&output, u64::MAX, stream);
        }
        for frame in input.chunks_exact(channels) {
            let [left, right] = pair(frame, channels);
            self.pending[0].push(left);
            self.pending[1].push(right);
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
/// Decode packets, hand interleaved blocks (with `skip_to` frames trimmed) to
/// `sink`, and survive isolated corrupt packets and stream resets.
fn read_blocks(
    reader: &mut Reader,
    skip_to: u64,
    mut at: u64,
    canceled: &dyn Fn() -> bool,
    mut sink: impl FnMut(&[f32], usize) -> Result<(), String>,
) -> Result<(), String> {
    let mut buffer: Option<SampleBuffer<f32>> = None;
    let mut failures = 0u32;
    while !canceled() {
        let Some(packet) = next_packet(reader)? else {
            break;
        };
        let decoded = match reader.decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // One corrupt packet is skipped; a run of them ends the track.
            Err(Error::DecodeError(_)) => {
                failures += 1;
                if failures > MAX_DECODE_FAILURES {
                    return Err("Could not decode audio: too many damaged packets.".into());
                }
                // Keep the timeline: a skipped packet plays as silence of its length.
                let lost = (seconds(reader.time_base.calc_time(packet.dur)) * reader.rate as f64)
                    .round()
                    .clamp(0.0, reader.rate as f64 * 2.0) as u64;
                let channels = reader.channels;
                if lost > 0 && channels > 0 {
                    let discard = skip_to.saturating_sub(at).min(lost);
                    at += lost;
                    let silence = vec![0.0f32; (lost - discard) as usize * channels];
                    if !silence.is_empty() {
                        sink(&silence, channels)?;
                    }
                }
                continue;
            }
            Err(Error::ResetRequired) => {
                failures += 1;
                if failures > MAX_DECODE_FAILURES {
                    return Err("Could not decode audio: the decoder keeps resetting.".into());
                }
                reader.decoder.reset();
                continue;
            }
            Err(e) => return Err(format!("Could not decode audio: {e}")),
        };
        failures = 0;
        let channels = decoded.spec().channels.count();
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err("Audio channels are missing.".into());
        }
        if reader.channels == 0 {
            reader.channels = channels;
        }
        if decoded.spec().rate != reader.rate || channels != reader.channels {
            return Err("Audio format changed inside the track.".into());
        }
        if decoded.capacity() > reader.rate as usize * 2 {
            return Err("Audio packet is too large.".into());
        }
        let needed = decoded.capacity() * channels;
        if buffer.as_ref().is_none_or(|b| b.capacity() < needed) {
            buffer = Some(SampleBuffer::<f32>::new(
                decoded.capacity() as u64,
                *decoded.spec(),
            ));
        }
        let Some(samples) = buffer.as_mut() else {
            continue;
        };
        samples.copy_interleaved_ref(decoded);
        let frames = samples.samples().len() / channels;
        let discard = skip_to.saturating_sub(at).min(frames as u64) as usize;
        at += frames as u64;
        if at > reader.rate as u64 * MAX_SECONDS as u64 + reader.rate as u64 {
            return Err("Track exceeds one hour.".into());
        }
        sink(&samples.samples()[discard * channels..], channels)?;
    }
    Ok(())
}
fn decode(path: &Path, start: f64, kind: &str, stream: &PcmStream) -> Result<(), String> {
    let mut reader = open(path)?;
    let mut at = 0u64;
    if start > 0.0 {
        // Accurate seeking first; a coarse seek plus trimming is far faster than
        // decoding from the start when the container cannot seek exactly.
        let wanted = (start * reader.rate as f64).round() as u64;
        let mut landed = None;
        for mode in [SeekMode::Accurate, SeekMode::Coarse] {
            if let Ok(seek) = reader.format.seek(
                mode,
                SeekTo::Time {
                    time: Time::from(start),
                    track_id: Some(reader.id),
                },
            ) {
                let frame = (seconds(reader.time_base.calc_time(seek.actual_ts))
                    * reader.rate as f64)
                    .round() as u64;
                // A seek that lands after the requested time cannot be trimmed
                // back; try the next mode, then fall back to decoding from 0.
                if frame <= wanted {
                    landed = Some(frame);
                    break;
                }
            }
        }
        match landed {
            Some(frame) => {
                at = frame;
                reader.decoder.reset();
            }
            None => reader = open(path)?,
        }
    }
    let skip_to = (start * reader.rate as f64).round() as u64;
    let mut converter = Converter::new(reader.rate, kind)?;
    read_blocks(
        &mut reader,
        skip_to,
        at,
        &|| stream.canceled.load(Ordering::Acquire),
        |block, channels| converter.push(block, channels, stream),
    )?;
    if !stream.canceled.load(Ordering::Acquire) {
        converter.finish(stream)?;
    }
    Ok(())
}

/// Peak envelope of the grip output a conversion mode would produce, for a UI
/// waveform preview. Values are in 0..=1 and `points` long at most. Runs on the
/// calling thread (decoding a long file takes time), so call it from a worker.
/// Filters are tuned for 48 kHz and run at the file's native rate, so the shape
/// is representative rather than sample-exact.
pub fn preview_envelope(path: &Path, kind: &str, points: usize) -> Result<Vec<f32>, String> {
    if !valid_kind(kind) {
        return Err("Choose a valid content type.".into());
    }
    // Previews decode whole files: share the decoder cap and survive panics.
    let _permit = acquire_file_permit(200)?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        preview_envelope_inner(path, kind, points)
    }))
    .unwrap_or_else(|_| Err("The audio preview stopped unexpectedly.".into()))
}
/// Takes a decoder slot, retrying briefly (10 ms steps) before giving up.
fn acquire_file_permit(tries: u32) -> Result<Permit, String> {
    let mut attempt = 0;
    loop {
        match Permit::acquire_max(&FILE_WORKERS, MAX_FILE_WORKERS) {
            Ok(permit) => return Ok(permit),
            Err(error) if attempt >= tries => return Err(error),
            Err(_) => {
                attempt += 1;
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
fn preview_envelope_inner(path: &Path, kind: &str, points: usize) -> Result<Vec<f32>, String> {
    let mut reader = open(path)?;
    let mut dsp = GripDsp::new(kind);
    let window = (reader.rate / 50).max(1);
    let (mut fine, mut peak, mut count) = (Vec::new(), 0.0f32, 0u32);
    read_blocks(&mut reader, 0, 0, &|| false, |block, channels| {
        for frame in block.chunks_exact(channels) {
            let out = dsp.frame(pair(frame, channels));
            peak = peak.max(out[0].abs()).max(out[1].abs());
            count += 1;
            if count >= window {
                fine.push(peak.min(1.0));
                peak = 0.0;
                count = 0;
            }
        }
        Ok(())
    })?;
    if count > 0 {
        fine.push(peak.min(1.0));
    }
    Ok(reduce_peaks(&fine, points))
}
fn reduce_peaks(fine: &[f32], points: usize) -> Vec<f32> {
    let points = points.clamp(1, 4096);
    if fine.len() <= points {
        return fine.to_vec();
    }
    (0..points)
        .map(|i| {
            let from = i * fine.len() / points;
            let to = ((i + 1) * fine.len() / points).clamp(from + 1, fine.len());
            fine[from..to].iter().copied().fold(0.0, f32::max)
        })
        .collect()
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
    // A superseded decoder exits within milliseconds of being cancelled; the cap
    // only bites if callers start tracks faster than old decoders can unwind.
    let permit = acquire_file_permit(5)?;
    thread::Builder::new()
        .name("controller-file-decoder".into())
        .spawn(move || {
            let _permit = permit;
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
    fn content_kinds_parse_validate_and_clamp_band_gains() {
        assert_eq!(parse_kind("bands:0.5,1.5,9").1, [0.5, 1.5, 2.0]);
        assert_eq!(parse_kind("bands"), (ConvMode::Bands, DEFAULT_BAND_GAINS));
        assert_eq!(parse_kind("beats").0, ConvMode::Beats);
        assert_eq!(parse_kind("haptic").0, ConvMode::Music);
        assert!(valid_kind("bands:1,1,1") && valid_kind("music") && valid_kind("beats"));
        assert!(!valid_kind("music:1") && !valid_kind("bands:1,2,3,4") && !valid_kind("x"));
        assert!(!valid_kind("bands:nan") && !valid_kind("bands:a"));
    }
    #[test]
    fn many_channel_frames_fold_to_finite_stereo() {
        assert_eq!(pair(&[0.25], 1), [0.25, 0.25]);
        assert_eq!(pair(&[0.1, 0.2], 2), [0.1, 0.2]);
        let folded = pair(&[0.5, 0.5, 0.2, 0.2, 0.2, 0.2], 6);
        assert!(
            folded
                .iter()
                .all(|v| v.is_finite() && *v > 0.39 && *v <= 1.0)
        );
    }
    #[test]
    fn envelope_preview_is_bounded_and_follows_the_audio() {
        assert_eq!(reduce_peaks(&[0.1, 0.9, 0.2, 0.3], 2), vec![0.9, 0.3]);
        assert_eq!(reduce_peaks(&[0.5], 100), vec![0.5]);
        assert!(reduce_peaks(&[], 10).is_empty());
        let fixture = wav(48000, 48000);
        let envelope = preview_envelope(&fixture.0, "bass", 25).unwrap();
        assert_eq!(envelope.len(), 25);
        assert!(envelope.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(envelope.iter().skip(5).any(|v| *v > 0.05));
        assert!(preview_envelope(&fixture.0, "nonsense", 25).is_err());
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
