//! USB audio output. Channels 1/2 carry only explicit speaker chirps; 3/4 drive the grips.
//! Audio callbacks never write HID reports or wait for the device loop.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};
#[cfg(windows)]
use std::{sync::mpsc, thread, time::Duration};

use cpal::{
    BufferSize, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

const RATE: u32 = 48_000;
const QUEUE_FRAMES: usize = 960; // at most 20 ms buffered at 48 kHz
const MAX_AMPLITUDE: f32 = 0.32;
const SPEAKER_CHIRP_SECONDS: f32 = 0.8;
const SPEAKER_FILE_GAIN: f32 = 0.42;
const MAX_SPEAKER_SAMPLES: usize = RATE as usize * 3;

#[derive(Clone)]
struct SpeakerPcm {
    start_frame: u64,
    samples: Vec<i16>,
    speaker: bool,
    grips: Option<Vec<f32>>,
    grip_filter: GripFilter,
    paused_at: Option<u64>,
}

impl SpeakerPcm {
    fn position(&self, frame: u64) -> usize {
        self.paused_at
            .unwrap_or(frame)
            .saturating_sub(self.start_frame) as usize
    }
}

pub struct SpeakerPlayback {
    pub paused: bool,
    pub position: f32,
    pub remaining: f32,
    pub speaker: bool,
}

#[derive(Clone, Copy)]
struct SpeakerChirp {
    start_frame: u64,
}

impl SpeakerChirp {
    fn sample(self, elapsed: f32) -> Option<f32> {
        if elapsed >= SPEAKER_CHIRP_SECONDS {
            return None;
        }
        let position = elapsed / SPEAKER_CHIRP_SECONDS;
        let envelope = (std::f32::consts::PI * position).sin().powi(2);
        let phase = 560.0 * elapsed + 450.0 * elapsed * elapsed;
        Some((phase * std::f32::consts::TAU).sin() * envelope * 0.30)
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
    start_frame: u64,
}

#[derive(Clone, Copy)]
pub enum Shape {
    Sine,
    Square,
    Saw,
    Noise,
    Sniper,
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
                "sniper" => Shape::Sniper,
                _ => Shape::Sine,
            },
            start_frame: 0,
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
            Shape::Sniper => {
                let low = if phase.fract() < 0.5 { 1.0 } else { -1.0 };
                let crack = (1.0 - elapsed / 0.055).max(0.0)
                    * (elapsed * 175.0 * std::f32::consts::TAU).sin();
                low * 0.95 + crack * 0.8
            }
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
    rendered_frames: AtomicU64,
    speaker_frames: AtomicU64,
    grip_frames: AtomicU64,
    bridge_frames: AtomicU64,
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
            rendered_frames: AtomicU64::new(0),
            speaker_frames: AtomicU64::new(0),
            grip_frames: AtomicU64::new(0),
            bridge_frames: AtomicU64::new(0),
            glitches: AtomicU64::new(0),
            error: Mutex::new(None),
        }
    }
}

pub struct AudioEngine {
    #[cfg(not(windows))]
    output: Option<Stream>,
    #[cfg(windows)]
    output: Option<ExclusiveOutput>,
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

#[cfg(windows)]
struct ExclusiveOutput {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl Drop for ExclusiveOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
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
    pub fn speaker_frames(&self) -> u64 {
        self.shared.speaker_frames.load(Ordering::Relaxed)
    }
    pub fn grip_frames(&self) -> u64 {
        self.shared.grip_frames.load(Ordering::Relaxed)
    }
    pub fn bridge_frames(&self) -> u64 {
        self.shared.bridge_frames.load(Ordering::Relaxed)
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

    #[cfg(windows)]
    pub fn start_output(&mut self) -> Result<&str, String> {
        if self.output.is_some() {
            return Ok(self.endpoint.as_deref().unwrap_or("DualSense"));
        }
        self.shared.rendered_frames.store(0, Ordering::Relaxed);
        let shared = self.shared.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("dualsense-exclusive-audio".into())
            .spawn(move || {
                let result = run_exclusive_output(shared.clone(), thread_stop, sender);
                if let Err(error) = result
                    && let Ok(mut slot) = shared.error.lock()
                {
                    *slot = Some(format!("USB haptic audio stopped: {error}"));
                }
            })
            .map_err(|e| format!("Could not start USB audio thread: {e}"))?;
        let name = receiver
            .recv_timeout(Duration::from_secs(4))
            .map_err(|e| format!("DualSense audio did not open: {e}"))??;
        self.output = Some(ExclusiveOutput {
            stop,
            thread: Some(thread),
        });
        self.endpoint = Some(name);
        Ok(self.endpoint.as_deref().unwrap())
    }

    #[cfg(not(windows))]
    pub fn start_output(&mut self) -> Result<&str, String> {
        if self.output.is_some() {
            return Ok(self.endpoint.as_deref().unwrap_or("DualSense"));
        }
        self.shared.rendered_frames.store(0, Ordering::Relaxed);
        let host = cpal::default_host();
        let mut candidate = None;
        let mut eligible_endpoints = 0usize;
        for device in host.output_devices().map_err(|e| e.to_string())? {
            let name = device.to_string();
            let lower = name.to_ascii_lowercase();
            if !lower.contains("dualsense") && !lower.contains("wireless controller") {
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
        let (device, name, format) = candidate.ok_or("No four-channel DualSense USB audio endpoint at 48 kHz. Connect by USB and enable its audio output in system settings.".to_string())?;
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

    pub fn play(&mut self, mut clip: VoiceClip) -> Result<&str, String> {
        self.start_output()?;
        clip.start_frame = self.shared.rendered_frames.load(Ordering::Relaxed);
        *self.shared.clip.lock().map_err(|e| e.to_string())? = Some(clip);
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn play_speaker_chirp(&mut self) -> Result<&str, String> {
        self.start_output()?;
        *self.shared.speaker_pcm.lock().map_err(|e| e.to_string())? = None;
        *self.shared.speaker.lock().map_err(|e| e.to_string())? = Some(SpeakerChirp {
            start_frame: self.shared.rendered_frames.load(Ordering::Relaxed),
        });
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn play_speaker_pcm(
        &mut self,
        samples: Vec<i16>,
        route: &str,
        append: bool,
    ) -> Result<&str, String> {
        if samples.is_empty() || samples.len() > MAX_SPEAKER_SAMPLES {
            return Err("Choose a sound between 1 sample and 3 seconds.".into());
        }
        if !matches!(route, "speaker" | "both" | "grips") {
            return Err("Choose a valid sound route.".into());
        }
        let use_grips = matches!(route, "both" | "grips");
        self.start_output()?;
        *self.shared.speaker.lock().map_err(|e| e.to_string())? = None;
        let mut current = self.shared.speaker_pcm.lock().map_err(|e| e.to_string())?;
        if append {
            let active = current
                .as_mut()
                .ok_or("The track stopped. Discarding queued audio.")?;
            if active.speaker != (route != "grips") || active.grips.is_some() != use_grips {
                return Err("Sound route changed during playback. Start the track again.".into());
            }
            if active.samples.len() + samples.len() > RATE as usize * 600 {
                return Err("The controller audio queue is full.".into());
            }
            if let Some(grips) = active.grips.as_mut() {
                grips.extend(active.grip_filter.process(&samples));
            }
            active.samples.extend(samples);
            return Ok(self.endpoint.as_deref().unwrap_or("DualSense"));
        }
        let mut grip_filter = GripFilter::default();
        let grips = use_grips.then(|| grip_filter.process(&samples));
        *current = Some(SpeakerPcm {
            start_frame: self.shared.rendered_frames.load(Ordering::Relaxed),
            samples,
            speaker: route != "grips",
            grips,
            grip_filter,
            paused_at: None,
        });
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn stop_speaker_pcm(&mut self) {
        if let Ok(mut chirp) = self.shared.speaker.lock() {
            *chirp = None;
        }
        if let Ok(mut current) = self.shared.speaker_pcm.lock() {
            *current = None;
        }
    }

    pub fn pause_speaker_pcm(&mut self, paused: bool) -> Result<SpeakerPlayback, String> {
        let mut current = self.shared.speaker_pcm.lock().map_err(|e| e.to_string())?;
        let sound = current.as_mut().ok_or("No track is playing.")?;
        let frame = self.shared.rendered_frames.load(Ordering::Relaxed);
        if paused && sound.paused_at.is_none() {
            sound.paused_at = Some(frame);
        } else if !paused && let Some(at) = sound.paused_at.take() {
            sound.start_frame += frame.saturating_sub(at);
        }
        let position = sound.position(frame).min(sound.samples.len());
        Ok(SpeakerPlayback {
            paused,
            position: position as f32 / RATE as f32,
            remaining: (sound.samples.len() - position) as f32 / RATE as f32,
            speaker: sound.speaker,
        })
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
            "explosions" => (0, 1.5),
            "footsteps" => (1, 1.3),
            "music" => (2, 1.4),
            "gentle" => (3, 0.8),
            "quiet" => (4, 0.25),
            _ => return Err("Choose a valid audio bridge preset.".into()),
        };
        self.start_output()?;
        self.shared
            .gain_bits
            .store((gain * preset_gain).to_bits(), Ordering::Relaxed);
        self.shared.preset.store(preset_id, Ordering::Relaxed);
        if self.capture.is_none() {
            self.shared.glitches.store(0, Ordering::Relaxed);
            self.shared.bridge_frames.store(0, Ordering::Relaxed);
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

#[cfg(windows)]
fn run_exclusive_output(
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<String, String>>,
) -> Result<(), String> {
    use wasapi::{DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WaveFormat};

    let setup = (|| {
        wasapi::initialize_mta()
            .ok()
            .map_err(|e| format!("COM audio setup failed: {e}"))?;
        let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
        let devices = enumerator
            .get_device_collection(&Direction::Render)
            .map_err(|e| e.to_string())?;
        let format = WaveFormat::new(16, 16, &SampleType::Int, RATE as usize, 4, Some(0));
        let mut selected = None;
        for device in &devices {
            let device = device.map_err(|e| e.to_string())?;
            let name = device.get_friendlyname().map_err(|e| e.to_string())?;
            let lower = name.to_ascii_lowercase();
            if !lower.contains("dualsense") && !lower.contains("wireless controller") {
                continue;
            }
            let client = device.get_iaudioclient().map_err(|e| e.to_string())?;
            if client.is_supported(&format, &ShareMode::Exclusive).is_err() {
                continue;
            }
            if selected.is_some() {
                return Err(
                    "More than one compatible DualSense USB audio endpoint is connected.".into(),
                );
            }
            selected = Some((name, client));
        }
        let (name, mut client) = selected.ok_or(
            "No four-channel DualSense USB audio endpoint at 48 kHz. Connect the pad by USB."
                .to_string(),
        )?;
        let (_, minimum_period) = client.get_device_period().map_err(|e| e.to_string())?;
        let period = minimum_period.max(100_000);
        client
            .initialize_client(
                &format,
                &Direction::Render,
                &StreamMode::PollingExclusive {
                    buffer_duration_hns: period * 2,
                    period_hns: period,
                },
            )
            .map_err(|e| format!("Exclusive DualSense audio setup failed: {e}"))?;
        let render = client.get_audiorenderclient().map_err(|e| e.to_string())?;
        Ok::<_, String>((name, client, render))
    })();
    let (name, client, render) = match setup {
        Ok(value) => value,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let mut position = 0;
    let mut duty = 0.0;
    let mut samples = Vec::new();
    let mut bytes = Vec::new();
    let mut write_available = || -> Result<(), String> {
        let frames = client
            .get_available_space_in_frames()
            .map_err(|e| e.to_string())? as usize;
        if frames == 0 {
            return Ok(());
        }
        samples.resize(frames * 4, 0);
        render_output_i16(&mut samples, &shared, &mut position, &mut duty);
        bytes.clear();
        bytes.reserve(samples.len() * 2 - bytes.capacity().min(samples.len() * 2));
        for sample in &samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        render
            .write_to_device(frames, &bytes, None)
            .map_err(|e| format!("DualSense USB audio write failed: {e}"))
    };
    if let Err(error) = write_available().and_then(|_| {
        client
            .start_stream()
            .map_err(|e| format!("Could not start exclusive DualSense audio: {e}"))
    }) {
        let _ = ready.send(Err(error.clone()));
        return Err(error);
    }
    let _ = ready.send(Ok(name));
    while !stop.load(Ordering::Acquire) {
        write_available()?;
        thread::sleep(Duration::from_millis(3));
    }
    client.stop_stream().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Clone, Default)]
struct GripFilter {
    low: f32,
    previous_low: f32,
    high: f32,
    energy: f32,
    treble_energy: f32,
    phase: f32,
}

impl GripFilter {
    fn process(&mut self, samples: &[i16]) -> Vec<f32> {
        let lp_alpha = std::f32::consts::TAU * 250.0 / RATE as f32;
        let hp_alpha = RATE as f32 / (RATE as f32 + std::f32::consts::TAU * 35.0);
        samples
            .iter()
            .map(|sample| {
                let x = *sample as f32 / i16::MAX as f32;
                self.low += lp_alpha * (x - self.low);
                self.high = hp_alpha * (self.high + self.low - self.previous_low);
                self.previous_low = self.low;
                self.energy += 0.002 * (x * x - self.energy);
                let treble = x - self.low;
                self.treble_energy += 0.002 * (treble * treble - self.treble_energy);
                let high_ratio = (self.treble_energy / (self.energy + 0.0001)).clamp(0.0, 1.0);
                self.phase = (self.phase
                    + std::f32::consts::TAU * (85.0 + 85.0 * high_ratio) / RATE as f32)
                    % std::f32::consts::TAU;
                let carrier = self.phase.sin()
                    * (self.energy.sqrt() * 0.75 + self.treble_energy.sqrt() * 1.2);
                (self.high * 3.0 + carrier).clamp(-1.0, 1.0)
            })
            .collect()
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
    let thermal = if *duty > 0.16 { 0.16 / *duty } else { 1.0 };
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
    let pcm = pcm_guard
        .as_ref()
        .and_then(|guard| guard.as_ref())
        .filter(|sound| sound.paused_at.is_none());
    let bridge_on = shared.bridge_on.load(Ordering::Relaxed);
    let mut speaker_frames = 0u64;
    let mut grip_frames = 0u64;
    for index in 0..frames {
        let frame = *position + index as u64;
        let tone = clip
            .and_then(|c| {
                frame
                    .checked_sub(c.start_frame)
                    .and_then(|at| c.sample(at as f32 / RATE as f32))
            })
            .unwrap_or([0.0; 2]);
        let audio = if bridge_on {
            ring_guard
                .as_mut()
                .and_then(|ring| ring.pop_front())
                .unwrap_or([0.0; 2])
        } else {
            [0.0; 2]
        };
        let grip = pcm
            .and_then(|sound| sound.grips.as_ref())
            .and_then(|samples| {
                pcm.and_then(|sound| {
                    frame
                        .checked_sub(sound.start_frame)
                        .and_then(|at| samples.get(at as usize))
                })
            })
            .copied()
            .unwrap_or(0.0);
        let l = soft_limit(tone[0] + audio[0] + grip, duty);
        let r = soft_limit(tone[1] + audio[1] + grip, duty);
        let chirp = speaker
            .and_then(|sound| {
                frame
                    .checked_sub(sound.start_frame)
                    .and_then(|at| sound.sample(at as f32 / RATE as f32))
            })
            .unwrap_or(0.0)
            + pcm
                .filter(|sound| sound.speaker)
                .and_then(|sound| {
                    frame
                        .checked_sub(sound.start_frame)
                        .and_then(|at| sound.samples.get(at as usize))
                })
                .map(|sample| *sample as f32 / i16::MAX as f32 * SPEAKER_FILE_GAIN)
                .unwrap_or(0.0);
        speaker_frames += u64::from(chirp.abs() > 0.005);
        grip_frames += u64::from(l.abs() > 0.005 || r.abs() > 0.005);
        write(index, [chirp, chirp, l, r]);
    }
    shared
        .speaker_frames
        .fetch_add(speaker_frames, Ordering::Relaxed);
    shared.grip_frames.fetch_add(grip_frames, Ordering::Relaxed);
    if clip.is_some_and(|c| {
        (*position + frames as u64).saturating_sub(c.start_frame) as f32 / RATE as f32
            >= c.attack + c.release
    }) && let Some(guard) = clip_guard.as_mut()
    {
        **guard = None;
    }
    if speaker.is_some_and(|c| {
        (*position + frames as u64).saturating_sub(c.start_frame) as f32 / RATE as f32
            >= SPEAKER_CHIRP_SECONDS
    }) && let Some(guard) = speaker_guard.as_mut()
    {
        **guard = None;
    }
    if pcm.is_some_and(|sound| {
        (*position + frames as u64).saturating_sub(sound.start_frame) as usize
            >= sound.samples.len()
    }) && let Some(guard) = pcm_guard.as_mut()
    {
        **guard = None;
    }
    *position += frames as u64;
    shared.rendered_frames.store(*position, Ordering::Relaxed);
    shared.callbacks.fetch_add(1, Ordering::Relaxed);
}

#[cfg(any(test, not(windows)))]
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
    high: [f32; 2],
    previous_low: [f32; 2],
    phase: f32,
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
            high: [0.0; 2],
            previous_low: [0.0; 2],
            phase: 0.0,
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
    fn frame(&mut self, input: [f32; 2], ring: &mut VecDeque<[f32; 2]>) -> bool {
        let mut out = [0.0; 2];
        let carrier = self.phase.cos();
        self.phase =
            (self.phase + std::f32::consts::TAU * 85.0 / self.rate as f32) % std::f32::consts::TAU;
        let hp_alpha = self.rate as f32 / (self.rate as f32 + std::f32::consts::TAU * 35.0);
        for channel in 0..2 {
            let x = input[channel].clamp(-1.0, 1.0);
            self.low[channel] += self.low_alpha * (x - self.low[channel]);
            self.high[channel] =
                hp_alpha * (self.high[channel] + self.low[channel] - self.previous_low[channel]);
            self.previous_low[channel] = self.low[channel];
            let target = x.abs();
            let alpha = if target > self.envelope[channel] {
                0.2
            } else {
                0.004
            };
            self.envelope[channel] += alpha * (target - self.envelope[channel]);
            let transient = (target - self.envelope[channel]).max(0.0);
            let level_gain = (0.16 / (self.envelope[channel] + 0.02)).clamp(1.0, 3.5);
            out[channel] = if self.envelope[channel] >= self.gate {
                (self.high[channel] * self.bass_gain + transient * self.transient_gain * carrier)
                    * self.gain
                    * level_gain
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
        out.iter().any(|sample| sample.abs() > 0.002)
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
                    let active = data.chunks_exact(channels).filter(|frame| dsp.frame([frame[0], frame[channels.min(2)-1]], &mut ring)).count();
                    shared.bridge_frames.fetch_add(active as u64, Ordering::Relaxed);
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
                    let active = data.chunks_exact(channels).filter(|frame| dsp.frame([frame[0] as f32 / 32768.0, frame[channels.min(2)-1] as f32 / 32768.0], &mut ring)).count();
                    shared.bridge_frames.fetch_add(active as u64, Ordering::Relaxed);
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
        let clip = VoiceClip::new("square", 90, 2, 100, 100, 0);
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
            start_frame: 0,
            samples: vec![i16::MAX; 2048],
            speaker: true,
            grips: None,
            grip_filter: GripFilter::default(),
            paused_at: None,
        });
        let mut frames = [0.0f32; 4096];
        render_output(&mut frames, &shared, &mut 0, &mut 0.0);
        assert!(frames.as_chunks::<4>().0.iter().any(|frame| frame[0] > 0.1));
        assert!(
            frames
                .as_chunks::<4>()
                .0
                .iter()
                .all(|frame| frame[0].abs() <= SPEAKER_FILE_GAIN
                    && frame[1] == frame[0]
                    && frame[2] == 0.0
                    && frame[3] == 0.0)
        );
    }
    #[test]
    fn pause_is_silent_and_resume_continues_at_the_same_sample() {
        let mut engine = AudioEngine::default();
        *engine.shared.speaker_pcm.lock().unwrap() = Some(SpeakerPcm {
            start_frame: 0,
            samples: (0..6000).map(|n| n as i16).collect(),
            speaker: true,
            grips: None,
            grip_filter: GripFilter::default(),
            paused_at: None,
        });
        engine.shared.rendered_frames.store(100, Ordering::Relaxed);
        let paused = engine.pause_speaker_pcm(true).unwrap();
        let mut silent = [1.0f32; 64];
        render_output(&mut silent, &engine.shared, &mut 100, &mut 0.0);
        assert!(silent.iter().all(|sample| *sample == 0.0));
        engine.shared.rendered_frames.store(2100, Ordering::Relaxed);
        let resumed = engine.pause_speaker_pcm(false).unwrap();
        assert_eq!(paused.position, resumed.position);
        let mut output = [0.0f32; 64];
        render_output(&mut output, &engine.shared, &mut 2100, &mut 0.0);
        assert!((output[0] - 100.0 / i16::MAX as f32 * SPEAKER_FILE_GAIN).abs() < 0.000001);
    }
    #[test]
    fn stop_clears_paused_pcm_and_test_tone() {
        let mut engine = AudioEngine::default();
        *engine.shared.speaker_pcm.lock().unwrap() = Some(SpeakerPcm {
            start_frame: 0,
            samples: vec![20000; 6000],
            speaker: true,
            grips: Some(vec![0.2; 6000]),
            grip_filter: GripFilter::default(),
            paused_at: Some(100),
        });
        *engine.shared.speaker.lock().unwrap() = Some(SpeakerChirp { start_frame: 0 });
        engine.stop_speaker_pcm();
        assert!(engine.pause_speaker_pcm(false).is_err());
        let mut output = [1.0f32; 64];
        render_output(&mut output, &engine.shared, &mut 100, &mut 0.0);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }
    #[test]
    fn grip_audio_maps_high_notes_into_the_tactile_band() {
        let treble: Vec<i16> = (0..4800)
            .map(|i| {
                ((i as f32 * std::f32::consts::TAU * 3000.0 / RATE as f32).sin() * 20_000.0) as i16
            })
            .collect();
        let samples = GripFilter::default().process(&treble);
        let energy = |hz: f32| {
            let (mut real, mut imaginary) = (0.0f32, 0.0f32);
            for (i, sample) in samples.iter().enumerate().skip(960) {
                let phase = i as f32 * std::f32::consts::TAU * hz / RATE as f32;
                real += sample * phase.cos();
                imaginary += sample * phase.sin();
            }
            real.hypot(imaginary)
        };
        assert!(energy(170.0) > energy(3000.0) * 3.0);
        assert!(samples.iter().any(|sample| sample.abs() > 0.5));
    }
    #[test]
    fn grip_filter_keeps_its_phase_between_file_chunks() {
        let samples: Vec<i16> = (0..2000)
            .map(|i| ((i as f32 * 0.017).sin() * 16000.0) as i16)
            .collect();
        let complete = GripFilter::default().process(&samples);
        let mut filter = GripFilter::default();
        let mut split = filter.process(&samples[..777]);
        split.extend(filter.process(&samples[777..]));
        assert_eq!(complete, split);
    }
    #[test]
    fn explicit_chirp_uses_only_speaker_channels_and_stays_quiet() {
        let shared = Shared::default();
        *shared.speaker.lock().unwrap() = Some(SpeakerChirp { start_frame: 0 });
        let mut pcm = [0.0f32; 4096];
        render_output(&mut pcm, &shared, &mut 0, &mut 0.0);
        let frames = pcm.as_chunks::<4>().0;
        assert!(frames.iter().any(|frame| frame[0].abs() > 0.0));
        assert!(frames.iter().all(|frame| frame[0].abs() <= 0.30
            && frame[0] == frame[1]
            && frame[2] == 0.0
            && frame[3] == 0.0));
    }
    #[test]
    fn audio_phase_is_continuous_across_output_buffers() {
        let single = Shared::default();
        let split = Shared::default();
        *single.speaker.lock().unwrap() = Some(SpeakerChirp { start_frame: 0 });
        *split.speaker.lock().unwrap() = Some(SpeakerChirp { start_frame: 0 });
        let mut one = [0.0f32; 4096];
        let mut first = [0.0f32; 2048];
        let mut second = [0.0f32; 2048];
        render_output(&mut one, &single, &mut 0, &mut 0.0);
        let mut position = 0;
        render_output(&mut first, &split, &mut position, &mut 0.0);
        render_output(&mut second, &split, &mut position, &mut 0.0);
        assert_eq!(&one[..2048], &first);
        assert_eq!(&one[2048..], &second);
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
