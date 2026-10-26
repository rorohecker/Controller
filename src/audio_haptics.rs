//! USB audio output. Channels 1/2 carry only explicit speaker chirps; 3/4 drive the grips.
//! Audio callbacks never write HID reports or wait for the device loop.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::Instant,
};

use cpal::{
    BufferSize, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

const RATE: u32 = 48_000;
const QUEUE_FRAMES: usize = 960; // at most 20 ms buffered at 48 kHz
const MAX_AMPLITUDE: f32 = 0.22;
const SPEAKER_CHIRP_SECONDS: f32 = 0.36;
const MAX_SPEAKER_SAMPLES: usize = RATE as usize * 3;

#[derive(Clone)]
struct SpeakerPcm {
    started: Instant,
    samples: Arc<[i16]>,
}

#[derive(Clone, Copy)]
struct SpeakerChirp {
    started: Instant,
}

impl SpeakerChirp {
    fn sample(self, elapsed: f32) -> Option<f32> {
        if elapsed >= SPEAKER_CHIRP_SECONDS {
            return None;
        }
        let position = elapsed / SPEAKER_CHIRP_SECONDS;
        let envelope = (std::f32::consts::PI * position).sin().powi(2);
        let phase = 560.0 * elapsed + 450.0 * elapsed * elapsed;
        Some((phase * std::f32::consts::TAU).sin() * envelope * 0.08)
    }
}

#[derive(Clone, Copy)]
pub struct VoiceClip {
    pub frequency: f32,
    pub attack: f32,
    pub release: f32,
    pub strength: f32,
    pub pan: f32,
    pub shape: Shape,
    started: Instant,
}

#[derive(Clone, Copy)]
pub enum Shape {
    Sine,
    Square,
    Saw,
    Noise,
}

impl VoiceClip {
    pub fn new(
        shape: &str,
        frequency: u16,
        attack: u16,
        release: u16,
        strength: u8,
        pan: i16,
    ) -> Self {
        Self {
            frequency: frequency.clamp(40, 300) as f32,
            attack: attack.clamp(2, 200) as f32 / 1000.0,
            release: release.clamp(25, 650) as f32 / 1000.0,
            strength: strength.min(100) as f32 / 100.0,
            pan: pan.clamp(-100, 100) as f32 / 100.0,
            shape: match shape {
                "square" => Shape::Square,
                "saw" => Shape::Saw,
                "noise" => Shape::Noise,
                _ => Shape::Sine,
            },
            started: Instant::now(),
        }
    }

    fn sample(self, elapsed: f32) -> Option<[f32; 2]> {
        let total = self.attack + self.release;
        if elapsed >= total {
            return None;
        }
        let envelope = if elapsed < self.attack {
            elapsed / self.attack
        } else {
            (total - elapsed) / self.release
        };
        let phase = elapsed * self.frequency;
        let wave = match self.shape {
            Shape::Sine => (phase * std::f32::consts::TAU).sin(),
            Shape::Square => {
                if phase.fract() < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Shape::Saw => phase.fract() * 2.0 - 1.0,
            Shape::Noise => ((phase * 73.37).sin() * (phase * 127.91).cos()).clamp(-1.0, 1.0),
        };
        // The output limiter applies MAX_AMPLITUDE. Applying it here as well
        // made a full-strength preview reach only a small fraction of that cap.
        let amplitude = wave * envelope * self.strength;
        Some([
            amplitude * (1.0 - self.pan.max(0.0)),
            amplitude * (1.0 + self.pan.min(0.0)),
        ])
    }
}

struct Shared {
    clip: Mutex<Option<VoiceClip>>,
    speaker: Mutex<Option<SpeakerChirp>>,
    speaker_pcm: Mutex<Option<SpeakerPcm>>,
    bridge: Mutex<VecDeque<[f32; 2]>>,
    gain_bits: AtomicU32,
    preset: AtomicU32,
    bridge_on: AtomicBool,
    callbacks: AtomicU64,
    glitches: AtomicU64,
    error: Mutex<Option<String>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            clip: Mutex::new(None),
            speaker: Mutex::new(None),
            speaker_pcm: Mutex::new(None),
            bridge: Mutex::new(VecDeque::with_capacity(QUEUE_FRAMES)),
            gain_bits: AtomicU32::new(0.5f32.to_bits()),
            preset: AtomicU32::new(3),
            bridge_on: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            glitches: AtomicU64::new(0),
            error: Mutex::new(None),
        }
    }
}

pub struct AudioEngine {
    output: Option<Stream>,
    capture: Option<Stream>,
    shared: Arc<Shared>,
    endpoint: Option<String>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self {
            output: None,
            capture: None,
            shared: Arc::new(Shared::default()),
            endpoint: None,
        }
    }
}

impl AudioEngine {
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }
    pub fn callbacks(&self) -> u64 {
        self.shared.callbacks.load(Ordering::Relaxed)
    }
    pub fn glitches(&self) -> u64 {
        self.shared.glitches.load(Ordering::Relaxed)
    }
    pub fn bridge_on(&self) -> bool {
        self.shared.bridge_on.load(Ordering::Relaxed)
    }

    pub fn take_error(&self) -> Option<String> {
        self.shared.error.lock().ok()?.take()
    }

    pub fn start_output(&mut self) -> Result<&str, String> {
        if self.output.is_some() {
            return Ok(self.endpoint.as_deref().unwrap_or("DualSense"));
        }
        let host = cpal::default_host();
        let mut candidate = None;
        let mut eligible_endpoints = 0usize;
        for device in host.output_devices().map_err(|e| e.to_string())? {
            let name = device.to_string();
            if !name.to_ascii_lowercase().contains("dualsense") {
                continue;
            }
            let configs = device
                .supported_output_configs()
                .map_err(|e| e.to_string())?;
            let mut selected_format = None;
            for config in configs {
                if config.channels() == 4
                    && config.min_sample_rate() <= RATE
                    && config.max_sample_rate() >= RATE
                {
                    if config.sample_format() == SampleFormat::F32 {
                        selected_format = Some(SampleFormat::F32);
                        break;
                    }
                    if config.sample_format() == SampleFormat::I16 && selected_format.is_none() {
                        selected_format = Some(SampleFormat::I16);
                    }
                }
            }
            if let Some(format) = selected_format {
                eligible_endpoints += 1;
                if candidate.is_none() {
                    candidate = Some((device, name, format));
                }
            }
        }
        if eligible_endpoints > 1 {
            return Err("More than one DualSense USB audio endpoint is connected. Keep one USB pad connected for haptic PCM, or use motor fallback.".into());
        }
        let (device, name, format) = candidate.ok_or("No four-channel DualSense USB audio endpoint at 48 kHz. Connect by USB and enable its Speakers device in Windows.".to_string())?;
        let config = StreamConfig {
            channels: 4,
            sample_rate: RATE,
            buffer_size: BufferSize::Default,
        };
        let shared = self.shared.clone();
        let error_shared = shared.clone();
        let on_error = move |error: cpal::Error| {
            record_audio_error(&error_shared, error, "USB haptic audio");
        };
        let mut frame_position = 0u64;
        let mut duty = 0.0f32;
        let stream = match format {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    render_output(data, &shared, &mut frame_position, &mut duty);
                },
                on_error,
                None,
            ),
            SampleFormat::I16 => {
                let shared = self.shared.clone();
                let error_shared = shared.clone();
                device.build_output_stream(
                    config,
                    move |data: &mut [i16], _| {
                        // Four-channel PCM conversion is done in place without a temporary buffer.
                        render_output_i16(data, &shared, &mut frame_position, &mut duty);
                    },
                    move |error| {
                        record_audio_error(&error_shared, error, "USB haptic audio");
                    },
                    None,
                )
            }
            _ => unreachable!(),
        }
        .map_err(|e| format!("Could not open DualSense haptic endpoint: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("Could not start DualSense haptic endpoint: {e}"))?;
        self.endpoint = Some(name);
        self.output = Some(stream);
        Ok(self.endpoint.as_deref().unwrap())
    }

    pub fn play(&mut self, clip: VoiceClip) -> Result<&str, String> {
        self.start_output()?;
        *self.shared.clip.lock().map_err(|e| e.to_string())? = Some(clip);
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn play_speaker_chirp(&mut self) -> Result<&str, String> {
        self.start_output()?;
        *self.shared.speaker_pcm.lock().map_err(|e| e.to_string())? = None;
        *self.shared.speaker.lock().map_err(|e| e.to_string())? = Some(SpeakerChirp {
            started: Instant::now(),
        });
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn play_speaker_pcm(&mut self, samples: Vec<i16>) -> Result<&str, String> {
        if samples.is_empty() || samples.len() > MAX_SPEAKER_SAMPLES {
            return Err("Choose a sound between 1 sample and 3 seconds.".into());
        }
        self.start_output()?;
        *self.shared.speaker.lock().map_err(|e| e.to_string())? = None;
        *self.shared.speaker_pcm.lock().map_err(|e| e.to_string())? = Some(SpeakerPcm {
            started: Instant::now(),
            samples: samples.into(),
        });
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn set_bridge(&mut self, enabled: bool, gain: f32, preset: &str) -> Result<String, String> {
        if !enabled {
            self.shared.bridge_on.store(false, Ordering::Relaxed);
            self.capture = None;
            if let Ok(mut ring) = self.shared.bridge.lock() {
                ring.clear();
            }
            return Ok("Audio bridge off".into());
        }
        if !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
            return Err("Bridge gain must be between 0 and 1.".into());
        }
        let (preset_id, preset_gain) = match preset {
            "explosions" => (0, 0.85),
            "footsteps" => (1, 0.65),
            "music" => (2, 0.55),
            "gentle" => (3, 0.3),
            "quiet" => (4, 0.12),
            _ => return Err("Choose a valid audio bridge preset.".into()),
        };
        self.start_output()?;
        self.shared
            .gain_bits
            .store((gain * preset_gain).to_bits(), Ordering::Relaxed);
        self.shared.preset.store(preset_id, Ordering::Relaxed);
        if self.capture.is_none() {
            self.shared.glitches.store(0, Ordering::Relaxed);
            self.capture = Some(build_loopback(self.shared.clone())?);
        }
        self.shared.bridge_on.store(true, Ordering::Relaxed);
        Ok(format!(
            "Audio bridge on · {}",
            self.endpoint.as_deref().unwrap_or("DualSense")
        ))
    }

    pub fn stop(&mut self) {
        self.shared.bridge_on.store(false, Ordering::Relaxed);
        self.capture = None;
        self.output = None;
        self.endpoint = None;
        if let Ok(mut clip) = self.shared.clip.lock() {
            *clip = None;
        }
        if let Ok(mut chirp) = self.shared.speaker.lock() {
            *chirp = None;
        }
        if let Ok(mut sound) = self.shared.speaker_pcm.lock() {
            *sound = None;
        }
        if let Ok(mut ring) = self.shared.bridge.lock() {
            ring.clear();
        }
    }
}

fn record_audio_error(shared: &Shared, error: cpal::Error, route: &str) {
    if error.kind() == cpal::ErrorKind::Xrun {
        // CPAL continues streaming after a WASAPI discontinuity; a glitch is not a stopped stream.
        shared.glitches.fetch_add(1, Ordering::Relaxed);
    } else if let Ok(mut slot) = shared.error.lock() {
        *slot = Some(format!("{route} stopped: {error}"));
    }
}

fn soft_limit(value: f32, duty: &mut f32) -> f32 {
    let bounded = (value * 1.5).tanh() * MAX_AMPLITUDE;
    *duty = *duty * 0.99998 + bounded.abs() * 0.00002;
    let thermal = if *duty > 0.11 { 0.11 / *duty } else { 1.0 };
    (bounded * thermal).clamp(-MAX_AMPLITUDE, MAX_AMPLITUDE)
}

fn fill_frames(
    mut write: impl FnMut(usize, [f32; 4]),
    frames: usize,
    shared: &Shared,
    position: &mut u64,
    duty: &mut f32,
) {
    let mut clip_guard = shared.clip.try_lock().ok();
    let mut speaker_guard = shared.speaker.try_lock().ok();
    let mut pcm_guard = shared.speaker_pcm.try_lock().ok();
    let mut ring_guard = shared.bridge.try_lock().ok();
    let clip = clip_guard.as_ref().and_then(|guard| **guard);
    let speaker = speaker_guard.as_ref().and_then(|guard| **guard);
    let pcm = pcm_guard.as_ref().and_then(|guard| guard.as_ref());
    let elapsed = clip
        .map(|c| c.started.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    let speaker_elapsed = speaker
        .map(|chirp| chirp.started.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    let pcm_start = pcm
        .map(|sound| (sound.started.elapsed().as_secs_f32() * RATE as f32) as usize)
        .unwrap_or(0);
    let bridge_on = shared.bridge_on.load(Ordering::Relaxed);
    for index in 0..frames {
        let tone = clip
            .and_then(|c| c.sample(elapsed + index as f32 / RATE as f32))
            .unwrap_or([0.0; 2]);
        let audio = if bridge_on {
            ring_guard
                .as_mut()
                .and_then(|ring| ring.pop_front())
                .unwrap_or([0.0; 2])
        } else {
            [0.0; 2]
        };
        let l = soft_limit(tone[0] + audio[0], duty);
        let r = soft_limit(tone[1] + audio[1], duty);
        let chirp = speaker
            .and_then(|sound| sound.sample(speaker_elapsed + index as f32 / RATE as f32))
            .unwrap_or(0.0)
            + pcm
                .and_then(|sound| sound.samples.get(pcm_start + index))
                .map(|sample| *sample as f32 / i16::MAX as f32 * 0.16)
                .unwrap_or(0.0);
        write(index, [chirp, chirp, l, r]);
    }
    if clip.is_some_and(|c| elapsed + frames as f32 / RATE as f32 >= c.attack + c.release)
        && let Some(guard) = clip_guard.as_mut()
    {
        **guard = None;
    }
    if speaker
        .is_some_and(|_| speaker_elapsed + frames as f32 / RATE as f32 >= SPEAKER_CHIRP_SECONDS)
        && let Some(guard) = speaker_guard.as_mut()
    {
        **guard = None;
    }
    if pcm.is_some_and(|sound| pcm_start + frames >= sound.samples.len())
        && let Some(guard) = pcm_guard.as_mut()
    {
        **guard = None;
    }
    *position += frames as u64;
    shared.callbacks.fetch_add(1, Ordering::Relaxed);
}

fn render_output(data: &mut [f32], shared: &Shared, position: &mut u64, duty: &mut f32) {
    let frames = data.len() / 4;
    fill_frames(
        |i, frame| data[i * 4..i * 4 + 4].copy_from_slice(&frame),
        frames,
        shared,
        position,
        duty,
    );
}

fn render_output_i16(data: &mut [i16], shared: &Shared, position: &mut u64, duty: &mut f32) {
    let frames = data.len() / 4;
    fill_frames(
        |i, frame| {
            for (out, sample) in data[i * 4..i * 4 + 4].iter_mut().zip(frame) {
                *out = (sample * i16::MAX as f32) as i16;
            }
        },
        frames,
        shared,
        position,
        duty,
    );
}

struct CaptureDsp {
    low: [f32; 2],
    envelope: [f32; 2],
    resample: f32,
    ratio: f32,
    gain: f32,
    rate: u32,
    preset_id: u32,
    low_alpha: f32,
    bass_gain: f32,
    transient_gain: f32,
    gate: f32,
}

impl CaptureDsp {
    fn new(rate: u32, gain: f32) -> Self {
        let mut dsp = Self {
            low: [0.0; 2],
            envelope: [0.0; 2],
            resample: 0.0,
            ratio: RATE as f32 / rate as f32,
            gain,
            rate,
            preset_id: u32::MAX,
            low_alpha: 0.0,
            bass_gain: 0.0,
            transient_gain: 0.0,
            gate: 0.0,
        };
        dsp.set_preset(3);
        dsp
    }
    fn set_preset(&mut self, id: u32) {
        if self.preset_id == id {
            return;
        }
        self.preset_id = id;
        let (cutoff, bass, transient, gate) = match id {
            0 => (160.0, 1.7, 0.25, 0.035), // explosions: heavy body, quiet passages suppressed
            1 => (260.0, 0.45, 1.2, 0.012), // footsteps: favor short impacts
            2 => (220.0, 1.1, 0.18, 0.0),   // music: even bass response
            4 => (140.0, 0.4, 0.2, 0.02),   // quiet hours: low gain and a gentle gate
            _ => (180.0, 0.7, 0.25, 0.0),   // everything gentle
        };
        self.low_alpha = (std::f32::consts::TAU * cutoff / self.rate as f32).clamp(0.001, 0.5);
        self.bass_gain = bass;
        self.transient_gain = transient;
        self.gate = gate;
    }
    fn frame(&mut self, input: [f32; 2], ring: &mut VecDeque<[f32; 2]>) {
        let mut out = [0.0; 2];
        for channel in 0..2 {
            let x = input[channel].clamp(-1.0, 1.0);
            self.low[channel] += self.low_alpha * (x - self.low[channel]);
            let target = x.abs();
            let alpha = if target > self.envelope[channel] {
                0.2
            } else {
                0.004
            };
            self.envelope[channel] += alpha * (target - self.envelope[channel]);
            let transient = (target - self.envelope[channel]).max(0.0);
            out[channel] = if self.envelope[channel] >= self.gate {
                (self.low[channel] * self.bass_gain + transient * self.transient_gain) * self.gain
            } else {
                0.0
            };
        }
        self.resample += self.ratio;
        while self.resample >= 1.0 {
            self.resample -= 1.0;
            if ring.len() == QUEUE_FRAMES {
                ring.pop_front();
            }
            ring.push_back(out);
        }
    }
}

fn build_loopback(shared: Arc<Shared>) -> Result<Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No Windows default audio output is available for loopback.")?;
    if device
        .to_string()
        .to_ascii_lowercase()
        .contains("dualsense")
    {
        return Err("Set Windows system audio to your speakers or headphones before enabling the bridge; capturing the controller endpoint would create feedback.".into());
    }
    let format = device
        .default_output_config()
        .map_err(|e| format!("Could not inspect system audio: {e}"))?;
    let config = StreamConfig {
        channels: format.channels(),
        sample_rate: format.sample_rate(),
        buffer_size: BufferSize::Default,
    };
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let error_shared = shared.clone();
    let on_error = move |error: cpal::Error| {
        record_audio_error(&error_shared, error, "System audio capture");
    };
    let stream = match format.sample_format() {
        SampleFormat::F32 => {
            let mut dsp = CaptureDsp::new(rate, 0.5);
            let shared = shared.clone();
            device.build_input_stream(config, move |data: &[f32], _| {
                dsp.gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));
                dsp.set_preset(shared.preset.load(Ordering::Relaxed));
                if let Ok(mut ring) = shared.bridge.try_lock() {
                    for frame in data.chunks_exact(channels) { dsp.frame([frame[0], frame[channels.min(2)-1]], &mut ring); }
                }
            }, on_error, None)
        }
        SampleFormat::I16 => {
            let mut dsp = CaptureDsp::new(rate, 0.5);
            let shared = shared.clone();
            let error_shared = shared.clone();
            device.build_input_stream(config, move |data: &[i16], _| {
                dsp.gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));
                dsp.set_preset(shared.preset.load(Ordering::Relaxed));
                if let Ok(mut ring) = shared.bridge.try_lock() {
                    for frame in data.chunks_exact(channels) { dsp.frame([frame[0] as f32 / 32768.0, frame[channels.min(2)-1] as f32 / 32768.0], &mut ring); }
                }
            }, move |error| {
                record_audio_error(&error_shared, error, "System audio capture");
            }, None)
        }
        other => return Err(format!("System audio uses unsupported sample format {other:?}. Choose a 16-bit or float output format in Windows.")),
    }.map_err(|e| format!("Could not start WASAPI loopback on {}: {e}", device))?;
    stream
        .play()
        .map_err(|e| format!("Could not play WASAPI capture: {e}"))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buffer_glitch_does_not_stop_a_running_stream() {
        let shared = Shared::default();
        record_audio_error(
            &shared,
            cpal::Error::new(cpal::ErrorKind::Xrun),
            "System audio capture",
        );
        assert_eq!(shared.glitches.load(Ordering::Relaxed), 1);
        assert!(shared.error.lock().unwrap().is_none());
        record_audio_error(
            &shared,
            cpal::Error::new(cpal::ErrorKind::DeviceNotAvailable),
            "System audio capture",
        );
        assert!(
            shared
                .error
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .contains("stopped")
        );
    }
    #[test]
    fn front_channels_silent_and_output_bounded() {
        let shared = Shared::default();
        *shared.clip.lock().unwrap() = Some(VoiceClip::new("square", 100, 2, 650, 100, 0));
        let mut pcm = [0.0f32; 4096];
        render_output(&mut pcm, &shared, &mut 0, &mut 0.0);
        assert!(pcm.as_chunks::<4>().0.iter().all(|f| f[0] == 0.0
            && f[1] == 0.0
            && f[2].abs() <= MAX_AMPLITUDE
            && f[3].abs() <= MAX_AMPLITUDE));
        assert!(pcm.as_chunks::<4>().0.iter().any(|f| f[2] != 0.0));
    }
    #[test]
    fn full_strength_preview_reaches_useful_level_without_exceeding_cap() {
        let shared = Shared::default();
        let mut clip = VoiceClip::new("square", 90, 2, 100, 100, 0);
        clip.started = Instant::now() - std::time::Duration::from_millis(15);
        *shared.clip.lock().unwrap() = Some(clip);
        let mut frames = [0.0f32; 4096];
        render_output(&mut frames, &shared, &mut 0, &mut 0.0);
        let peak = frames
            .as_chunks::<4>()
            .0
            .iter()
            .map(|frame| frame[2].abs())
            .fold(0.0f32, f32::max);
        assert!(peak > 0.1, "peak {peak} should be audible at full strength");
        assert!(peak <= MAX_AMPLITUDE);
    }
    #[test]
    fn custom_speaker_pcm_stays_on_front_channels() {
        let shared = Shared::default();
        *shared.speaker_pcm.lock().unwrap() = Some(SpeakerPcm {
            started: Instant::now(),
            samples: vec![i16::MAX; 2048].into(),
        });
        let mut frames = [0.0f32; 4096];
        render_output(&mut frames, &shared, &mut 0, &mut 0.0);
        assert!(frames.as_chunks::<4>().0.iter().any(|frame| frame[0] > 0.1));
        assert!(
            frames
                .as_chunks::<4>()
                .0
                .iter()
                .all(|frame| frame[0].abs() <= 0.16
                    && frame[1] == frame[0]
                    && frame[2] == 0.0
                    && frame[3] == 0.0)
        );
    }
    #[test]
    fn explicit_chirp_uses_only_speaker_channels_and_stays_quiet() {
        let shared = Shared::default();
        *shared.speaker.lock().unwrap() = Some(SpeakerChirp {
            started: Instant::now(),
        });
        let mut pcm = [0.0f32; 4096];
        render_output(&mut pcm, &shared, &mut 0, &mut 0.0);
        let frames = pcm.as_chunks::<4>().0;
        assert!(frames.iter().any(|frame| frame[0].abs() > 0.0));
        assert!(frames.iter().all(|frame| frame[0].abs() <= 0.08
            && frame[0] == frame[1]
            && frame[2] == 0.0
            && frame[3] == 0.0));
    }
    #[test]
    fn capture_queue_stays_bounded() {
        let mut ring = VecDeque::with_capacity(QUEUE_FRAMES);
        let mut dsp = CaptureDsp::new(48_000, 1.0);
        for _ in 0..QUEUE_FRAMES * 2 {
            dsp.frame([1.0, 1.0], &mut ring);
        }
        assert_eq!(ring.len(), QUEUE_FRAMES);
    }
    #[test]
    fn footsteps_preset_emphasizes_short_impacts() {
        let mut footsteps = CaptureDsp::new(RATE, 1.0);
        footsteps.set_preset(1);
        let mut explosions = CaptureDsp::new(RATE, 1.0);
        explosions.set_preset(0);
        let mut quick = VecDeque::new();
        let mut heavy = VecDeque::new();
        footsteps.frame([1.0, 1.0], &mut quick);
        explosions.frame([1.0, 1.0], &mut heavy);
        assert!(quick[0][0] > heavy[0][0]);
    }
}
