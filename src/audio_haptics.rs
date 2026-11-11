//! USB audio output. Channels 1/2 carry speaker audio; 3/4 drive the grips.
//! Audio callbacks never write HID reports or wait for the device loop.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[path = "audio_haptics/authoring.rs"]
pub mod authoring;
#[path = "audio_haptics/capture.rs"]
pub mod capture;
#[path = "audio_haptics/file.rs"]
pub mod file;
use capture::{CaptureHandle, CaptureSource};
#[path = "audio_haptics/bluetooth.rs"]
mod bluetooth;
#[path = "audio_haptics/capture_math.rs"]
mod capture_math;
#[path = "audio_haptics/dsp.rs"]
mod dsp;
#[path = "audio_haptics/io_runtime.rs"]
mod io_runtime;
#[path = "audio_haptics/stream.rs"]
mod stream;
use arc_swap::{ArcSwap, ArcSwapOption};
use authoring::{CompiledClip, EffectClip, PersonalSettings};
use crossbeam_queue::ArrayQueue;
use dsp::{ConvMode, OnsetDetector, Texture, ThreeBand};
// Used by the motor fallback in haptics.rs; example builds that include this file alone do not use them.
#[allow(unused_imports)]
pub use dsp::{DOUBLE_GAP, swell_gain};
use serde::{Deserialize, Serialize};
pub use stream::MixerSettings;
use stream::{PcmStream, RenderState, routed_frame, solo_code};

use cpal::{
    BufferSize, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

const RATE: u32 = 48_000;
const QUEUE_FRAMES: usize = 960; // Normal bridge latency: at most 20 ms buffered at 48 kHz
const BRIDGE_RING_MAX: usize = 1920; // Safe latency (40 ms) is the largest the ring can hold
const PRESET_CUSTOM: u32 = 5;
/// A retired control-side `Arc` is kept this long so the audio callback has
/// certainly released its guard before the last reference is dropped.
const RETIRE_AFTER: Duration = Duration::from_secs(2);
const GAME_RUMBLE_SECONDS: u64 = 10;
const VOICE_NOISE_SEED: u32 = 0x5EED_0001;
const OUTPUT_RETRY: Duration = Duration::from_secs(5);
const MAX_CAPTURE_REOPENS: u32 = 6;
/// Ceiling of the "Normal" haptic strength and the Bluetooth PCM reference level.
const MAX_AMPLITUDE: f32 = 0.32;
/// Soft / Normal / Strong / Max output ceilings (peak amplitude, 1.0 = full scale).
/// Roughly 2x apart at the bottom and 1.5x apart above, so every step is felt.
const LEVEL_CEILINGS: [f32; 4] = [0.16, MAX_AMPLITUDE, 0.50, 0.75];
/// Thermal average-drive caps (about 1 s window) that scale WITH the level, so
/// sustained effects (rumble, heartbeat, long sines) step as clearly as short peaks.
/// Normal keeps the historical 0.16.
const LEVEL_DUTY_CAPS: [f32; 4] = [0.09, 0.16, 0.24, 0.34];
/// Absolute hard safety cap on the sustained average drive, whatever the level.
/// The limiter tracks the mean |x| of the drive (not power): a continuous full-scale
/// sine at Max therefore settles at ~0.34 mean |x| (~0.38 RMS), about twice the
/// sustained RMS of Normal (cap 0.16). That is why Strong and Max are opt-in; Normal is
/// the shipped default, and thermal limiting still applies at every level.
const HARD_DUTY_CAP: f32 = 0.34;
const LEVEL_NAMES: [&str; 4] = ["soft", "normal", "strong", "max"];
/// Normal: the historical, shipped behaviour. Strong and Max are opt-in.
const DEFAULT_LEVEL: u32 = 1;
/// One-pole smoothing of the ceiling and thermal cap (about 15 ms at 48 kHz), so a level
/// change mid-effect ramps instead of stepping.
const LEVEL_SMOOTH: f32 = 0.0014;
/// Fraction of the ceiling below which the limiter is exactly linear.
const KNEE: f32 = 0.7;
const SPEAKER_CHIRP_SECONDS: f32 = 0.8;
const SPEAKER_FILE_GAIN: f32 = 0.42;
const MAX_SPEAKER_SAMPLES: usize = RATE as usize * 3;

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
    pub raw_test: bool,
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
    /// Triangle, pulse train, lub-dub and swell (see `dsp::Texture`).
    Tex(Texture),
    /// Internal weapon-preset voice (`device.rs` WeaponKind::Sniper). Deliberately not in
    /// `is_known_shape`: users and profiles cannot pick it.
    Sniper,
    LinkTest,
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
            raw_test: false,
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
                other => Texture::parse(other).map_or(Shape::Sine, Shape::Tex),
            },
            start_frame: 0,
        }
    }

    /// Shapes a user or profile may choose. `VoiceClip::new` itself is lenient
    /// (unknown names play as sine); callers that take user input check this first.
    pub fn is_known_shape(name: &str) -> bool {
        matches!(name, "sine" | "square" | "saw" | "noise") || Texture::parse(name).is_some()
    }

    fn sample(self, elapsed: f32) -> Option<[f32; 2]> {
        let total = self.attack + self.release;
        if elapsed >= total {
            return None;
        }
        let envelope = if matches!(self.shape, Shape::LinkTest) {
            1.0
        } else if elapsed < self.attack {
            elapsed / self.attack
        } else {
            (total - elapsed) / self.release
        };
        // A swell rises and falls on a raised-cosine curve instead of straight ramps.
        let envelope = if matches!(self.shape, Shape::Tex(Texture::Swell)) {
            dsp::swell_gain(envelope)
        } else {
            envelope
        };
        let phase = elapsed * self.frequency;
        let wave = match self.shape {
            Shape::LinkTest => {
                if (elapsed * 4.0).fract() < 0.5 {
                    (phase * std::f32::consts::TAU).sin()
                } else {
                    0.0
                }
            }
            Shape::Sine => (phase * std::f32::consts::TAU).sin(),
            Shape::Square => {
                if phase.fract() < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Shape::Saw => phase.fract() * 2.0 - 1.0,
            Shape::Noise => dsp::band_noise(elapsed, self.frequency, VOICE_NOISE_SEED),
            // The swell's slow rise comes from the envelope above, so only its carrier is taken here.
            Shape::Tex(Texture::Swell) => (phase * std::f32::consts::TAU).sin(),
            Shape::Tex(texture) => texture.sample(elapsed, total, self.frequency),
            Shape::Sniper => {
                let low = if phase.fract() < 0.5 { 1.0 } else { -1.0 };
                let crack = (1.0 - elapsed / 0.055).max(0.0)
                    * (elapsed * 175.0 * std::f32::consts::TAU).sin();
                low * 0.95 + crack * 0.8
            }
        };
        // The output limiter applies the output ceiling. Applying it here as well
        // made a full-strength preview reach only a small fraction of that cap.
        let amplitude = wave * envelope * self.strength;
        Some([
            amplitude * (1.0 - self.pan.max(0.0)),
            amplitude * (1.0 + self.pan.min(0.0)),
        ])
    }
}

/// XInput exposes two motor envelopes, not authored DualSense waveforms.
/// Keep their low/high character and stereo separation within the output limiter.
struct GameRumble {
    motors: [u8; 2],
    start_frame: u64,
}
impl GameRumble {
    fn sample(&self, frame: u64) -> [f32; 2] {
        let elapsed = frame.saturating_sub(self.start_frame);
        // Lost clients must not leave the controller vibrating indefinitely.
        if elapsed >= GAME_RUMBLE_SECONDS * u64::from(RATE) {
            return [0.0; 2];
        }
        let seconds = (frame % u64::from(RATE)) as f32 / RATE as f32;
        std::array::from_fn(|i| {
            (seconds * [70.0, 150.0][i] * std::f32::consts::TAU).sin() * f32::from(self.motors[i])
                / 255.0
        })
    }
}

impl GameRumble {
    /// The single definition of "still rumbling": sources_idle, release_idle_output
    /// and diagnostics all go through it, so an expired rumble releases the endpoint.
    fn active_at(&self, frame: u64) -> bool {
        frame.saturating_sub(self.start_frame) < GAME_RUMBLE_SECONDS * u64::from(RATE)
    }
}

struct TimelineAudio {
    clip: EffectClip,
    /// String lanes and shapes flattened once, at store time.
    compiled: CompiledClip,
    start_frame: u64,
}
impl TimelineAudio {
    fn new(clip: EffectClip, start_frame: u64) -> Self {
        let compiled = clip.compile();
        Self {
            clip,
            compiled,
            start_frame,
        }
    }
}

/// Settable bridge character. `latency_frames` is 480 / 960 / 1920 (10 / 20 / 40 ms).
struct BridgeTuning {
    latency_frames: AtomicU32,
    custom: [AtomicU32; 4], // cutoff Hz, bass, transient, gate (f32 bits)
    mode: AtomicU32,        // 0 standard, 1 three-band
    bands: [AtomicU32; 3],  // f32 bits
    onset: AtomicU32,       // f32 bits, 0 = off
    pan: AtomicU32,         // f32 bits, 0 = off
    /// Overall intensity: index into `LEVEL_CEILINGS` (0 soft, 1 normal, 2 strong, 3 max).
    level: AtomicU32,
}
impl Default for BridgeTuning {
    fn default() -> Self {
        let custom = BridgeCustom::default();
        Self {
            latency_frames: AtomicU32::new(QUEUE_FRAMES as u32),
            custom: [custom.cutoff, custom.bass, custom.transient, custom.gate]
                .map(|v| AtomicU32::new(v.to_bits())),
            mode: AtomicU32::new(0),
            bands: [1.0f32, 1.0, 0.6].map(|v| AtomicU32::new(v.to_bits())),
            onset: AtomicU32::new(0),
            pan: AtomicU32::new(0),
            level: AtomicU32::new(DEFAULT_LEVEL),
        }
    }
}
impl BridgeTuning {
    fn snapshot(&self) -> Self {
        let copy = |a: &AtomicU32| AtomicU32::new(a.load(Ordering::Relaxed));
        Self {
            latency_frames: copy(&self.latency_frames),
            custom: std::array::from_fn(|i| copy(&self.custom[i])),
            mode: copy(&self.mode),
            bands: std::array::from_fn(|i| copy(&self.bands[i])),
            onset: copy(&self.onset),
            pan: copy(&self.pan),
            level: copy(&self.level),
        }
    }
    fn level_index(&self) -> usize {
        (self.level.load(Ordering::Relaxed) as usize).min(LEVEL_CEILINGS.len() - 1)
    }
    fn latency_name(&self) -> &'static str {
        let frames = self.latency_frames.load(Ordering::Relaxed);
        if frames <= 480 {
            "low"
        } else if frames <= 960 {
            "normal"
        } else {
            "safe"
        }
    }
}

/// A user-defined bridge preset in addition to the five fixed ones.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeCustom {
    pub cutoff: f32,
    pub bass: f32,
    pub transient: f32,
    pub gate: f32,
}
impl Default for BridgeCustom {
    fn default() -> Self {
        Self {
            cutoff: 180.0,
            bass: 0.7,
            transient: 0.25,
            gate: 0.0,
        }
    }
}
impl BridgeCustom {
    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f32, low: f32, high: f32| v.is_finite() && (low..=high).contains(&v);
        if ok(self.cutoff, 60.0, 400.0)
            && ok(self.bass, 0.0, 3.0)
            && ok(self.transient, 0.0, 3.0)
            && ok(self.gate, 0.0, 0.2)
        {
            Ok(())
        } else {
            Err("Custom bridge: cutoff 60-400 Hz, bass 0-3, transient 0-3, gate 0-0.2.".into())
        }
    }
}

/// Optional bridge conversion extras; every field defaults to off.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeOptions {
    /// "standard" or "threeBand" (sub / punch / texture split).
    pub mode: String,
    pub band_gains: [f32; 3],
    /// 0 = off; 0-1 scales onset-triggered impact pulses added to the bridge.
    pub onset: f32,
    /// 0 = off; 0-4 widens left/right differences so direction is felt.
    pub pan_emphasis: f32,
}
impl Default for BridgeOptions {
    fn default() -> Self {
        Self {
            mode: "standard".into(),
            band_gains: [1.0, 1.0, 0.6],
            onset: 0.0,
            pan_emphasis: 0.0,
        }
    }
}
impl BridgeOptions {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.mode.as_str(), "standard" | "threeBand")
            || !self
                .band_gains
                .iter()
                .all(|g| g.is_finite() && (0.0..=2.0).contains(g))
            || !(self.onset.is_finite() && (0.0..=1.0).contains(&self.onset))
            || !(self.pan_emphasis.is_finite() && (0.0..=4.0).contains(&self.pan_emphasis))
        {
            return Err("Bridge options: mode standard or threeBand, band gains 0-2, onset 0-1, pan emphasis 0-4.".into());
        }
        Ok(())
    }
}

struct Shared {
    timeline: ArcSwapOption<TimelineAudio>,
    personal: ArcSwap<PersonalSettings>,
    game: ArcSwapOption<GameRumble>,
    clip: ArcSwapOption<VoiceClip>,
    speaker: ArcSwapOption<SpeakerChirp>,
    speaker_pcm: ArcSwapOption<PcmStream>,
    mixer: ArcSwap<MixerSettings>,
    bridge: ArrayQueue<CaptureFrame>,
    file_underruns: AtomicU64,
    bridge_underruns: AtomicU64,
    bridge_hold_frames: AtomicU64,
    bridge_dropped: AtomicU64,
    peaks: [AtomicU32; 3],
    reduction: AtomicU32,
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
    capture_error: Mutex<Option<String>>,
    capture_generation: AtomicU64,
    tuning: BridgeTuning,
    /// Thermal duty per grip, carried across stream restarts (decays with time).
    duty: [AtomicU32; 2],
    thermal_active: AtomicBool,
    /// Running sums of |grip| (x1e6) and grip^2 (x1e9) per side, for mean/RMS measurements.
    grip_abs: [AtomicU64; 2],
    grip_sq: [AtomicU64; 2],
    /// Windows exclusive output ran dry (device buffer fully drained at write time).
    output_underruns: AtomicU64,
    capture_in_peak: AtomicU32,
    capture_in_rms: AtomicU32,
    capture_device: Mutex<Option<(String, u32)>>,
    capture_status: Mutex<Option<String>>,
    /// True while a system-default capture worker follows device changes itself.
    /// Capture generation + 1 of the worker following the default device (0 = none).
    capture_reopen: AtomicU64,
    capture_lost: AtomicBool,
    /// Replaced `Arc`s wait here so the audio thread never frees them.
    retired: Mutex<Vec<(Instant, Box<dyn std::any::Any + Send>)>>,
}

impl Shared {
    /// Control-thread only. Keeps a replaced value alive for `RETIRE_AFTER` so the
    /// last reference is never dropped inside an audio callback holding a guard.
    fn retire<T: Send + Sync + 'static>(&self, old: Option<Arc<T>>) {
        let Ok(mut retired) = self.retired.lock() else {
            return;
        };
        retired.retain(|(at, _)| at.elapsed() < RETIRE_AFTER);
        if let Some(old) = old {
            retired.push((Instant::now(), Box::new(old)));
        }
    }
    fn put<T: Send + Sync + 'static>(&self, slot: &ArcSwapOption<T>, value: Option<Arc<T>>) {
        self.retire(slot.swap(value));
    }
    /// Output ceiling for the selected haptic strength (read once per buffer).
    fn ceiling(&self) -> f32 {
        LEVEL_CEILINGS[self.tuning.level_index()]
    }
    fn duty_load(&self) -> [f32; 2] {
        std::array::from_fn(|i| f32::from_bits(self.duty[i].load(Ordering::Relaxed)))
    }
    fn duty_store(&self, duty: [f32; 2]) {
        for (slot, value) in self.duty.iter().zip(duty) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }
    fn game_active(&self, now: u64) -> bool {
        self.game.load().as_ref().is_some_and(|g| g.active_at(now))
    }
    /// Pre-DSP capture level for the status display.
    fn record_capture_input(&self, peak: f32, rms: f32) {
        self.capture_in_peak
            .fetch_max(peak.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.capture_in_rms
            .store(rms.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    /// Publishes capture info for one generation; a retired worker write is dropped.
    fn set_capture_info(
        &self,
        generation: u64,
        device: Option<(String, u32)>,
        status: Option<String>,
    ) {
        if self.capture_generation.load(Ordering::Acquire) != generation {
            return;
        }
        if let Some(device) = device
            && let Ok(mut slot) = self.capture_device.lock()
        {
            *slot = Some(device);
        }
        if let Ok(mut slot) = self.capture_status.lock()
            && self.capture_generation.load(Ordering::Acquire) == generation
        {
            *slot = status;
        }
    }
    fn clear_capture_info(&self) {
        if let Ok(mut slot) = self.capture_device.lock() {
            *slot = None;
        }
        if let Ok(mut slot) = self.capture_status.lock() {
            *slot = None;
        }
        self.capture_in_peak.store(0, Ordering::Relaxed);
        self.capture_in_rms.store(0, Ordering::Relaxed);
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            timeline: ArcSwapOption::from(None),
            personal: ArcSwap::from_pointee(PersonalSettings::default()),
            game: ArcSwapOption::from(None),
            clip: ArcSwapOption::from(None),
            speaker: ArcSwapOption::from(None),
            speaker_pcm: ArcSwapOption::from(None),
            mixer: ArcSwap::from_pointee(MixerSettings::default()),
            bridge: ArrayQueue::new(BRIDGE_RING_MAX),
            file_underruns: AtomicU64::new(0),
            bridge_underruns: AtomicU64::new(0),
            bridge_hold_frames: AtomicU64::new(0),
            bridge_dropped: AtomicU64::new(0),
            peaks: Default::default(),
            reduction: AtomicU32::new(0),
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
            capture_error: Mutex::new(None),
            capture_generation: AtomicU64::new(0),
            tuning: BridgeTuning::default(),
            duty: Default::default(),
            thermal_active: AtomicBool::new(false),
            grip_abs: Default::default(),
            grip_sq: Default::default(),
            output_underruns: AtomicU64::new(0),
            capture_in_peak: AtomicU32::new(0),
            capture_in_rms: AtomicU32::new(0),
            capture_device: Mutex::new(None),
            capture_status: Mutex::new(None),
            capture_reopen: AtomicU64::new(0),
            capture_lost: AtomicBool::new(false),
            retired: Mutex::new(Vec::new()),
        }
    }
}

pub struct AudioEngine {
    bluetooth_enabled: bool,
    bluetooth_output: Option<bluetooth::Output>,
    output: Option<io_runtime::OutputWorker>,
    retired_outputs: Vec<Arc<AtomicBool>>,
    output_startup_ms: Option<u64>,
    output_failure: Option<(String, std::time::Instant)>,
    capture: Option<CaptureHandle>,
    capture_source: Option<CaptureSource>,
    /// The Shared the running capture worker was started with.
    capture_shared: Option<Arc<Shared>>,
    shared: Arc<Shared>,
    endpoint: Option<String>,
    endpoint_id: Option<String>,
    endpoint_error: Option<String>,
    legacy_filter: GripFilter,
    /// Thermal duty and when it was saved, so a restarted stream continues from a decayed value.
    duty_saved: Option<([f32; 2], Instant)>,
    process_reattach: bool,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self {
            bluetooth_enabled: false,
            bluetooth_output: None,
            output: None,
            retired_outputs: Vec::new(),
            output_startup_ms: None,
            output_failure: None,
            capture: None,
            capture_source: None,
            capture_shared: None,
            shared: Arc::new(Shared::default()),
            endpoint: None,
            endpoint_id: None,
            endpoint_error: None,
            legacy_filter: GripFilter::default(),
            duty_saved: None,
            process_reattach: false,
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

impl AudioEngine {
    fn retire_output(&mut self) {
        if self.output.is_some() || self.bluetooth_output.is_some() {
            self.duty_saved = Some((self.shared.duty_load(), Instant::now()));
        }
        self.bluetooth_output = None;
        self.retired_outputs
            .retain(|done| !done.load(Ordering::Acquire));
        if let Some(worker) = self.output.take() {
            self.retired_outputs.push(worker.completion());
            drop(worker);
        }
    }
    fn fresh_output_state(&mut self) {
        // Kept until the output actually starts (see start_output).
        let carried = self.duty_saved.map_or([0.0; 2], |(duty, saved)| {
            let seconds = saved.elapsed().as_secs_f32();
            duty.map(|d| dsp::decay_duty(d, seconds))
        });
        let shared = Shared {
            personal: ArcSwap::from(self.shared.personal.load_full()),
            mixer: ArcSwap::from(self.shared.mixer.load_full()),
            tuning: self.shared.tuning.snapshot(),
            ..Shared::default()
        };
        shared.duty_store(carried);
        self.shared = Arc::new(shared);
    }
    pub fn set_personal(&self, settings: PersonalSettings) -> Result<(), String> {
        settings.validate().map_err(str::to_string)?;
        self.shared
            .retire(Some(self.shared.personal.swap(Arc::new(settings))));
        Ok(())
    }
    pub fn play_timeline(&mut self, clip: EffectClip) -> Result<(), String> {
        clip.validate().map_err(str::to_string)?;
        if clip.has_grips() {
            self.start_output()?;
            let timeline =
                TimelineAudio::new(clip, self.shared.rendered_frames.load(Ordering::Relaxed));
            self.shared
                .put(&self.shared.timeline, Some(Arc::new(timeline)));
        }
        Ok(())
    }
    pub fn stop_timeline(&self) {
        self.shared.put(&self.shared.timeline, None);
    }
    pub fn cancel_cue(&self) {
        self.shared.put(&self.shared.clip, None);
        self.stop_timeline();
    }

    pub fn has_sources(&self) -> bool {
        !self.sources_idle()
    }
    /// Everything that currently needs the output endpoint to stay open.
    fn active_sources(&self) -> Vec<&'static str> {
        let frame = self.shared.rendered_frames.load(Ordering::Relaxed);
        let mut active = Vec::new();
        if self.bridge_on() {
            active.push("bridge");
        }
        if self.shared.timeline.load().as_ref().is_some_and(|t| {
            frame.saturating_sub(t.start_frame) < u64::from(t.clip.duration_ms()) * 48
        }) {
            active.push("timeline");
        }
        if self.shared.game_active(frame) {
            active.push("gameRumble");
        }
        if self.shared.clip.load().as_ref().is_some_and(|c| {
            c.sample(frame.saturating_sub(c.start_frame) as f32 / RATE as f32)
                .is_some()
        }) {
            active.push("cue");
        }
        if self.shared.speaker.load().as_ref().is_some_and(|c| {
            c.sample(frame.saturating_sub(c.start_frame) as f32 / RATE as f32)
                .is_some()
        }) {
            active.push("speakerChirp");
        }
        if self
            .shared
            .speaker_pcm
            .load()
            .as_ref()
            .is_some_and(|s| !matches!(s.mode(), "finished" | "stopped"))
        {
            active.push("track");
        }
        active
    }
    fn sources_idle(&self) -> bool {
        self.active_sources().is_empty()
    }

    /// Give native games their exclusive endpoint back after app feedback ends.
    /// Keep paused tracks and live bridges intact.
    pub fn release_idle_output(&mut self) {
        if self.output_ready() && self.sources_idle() {
            self.retire_output();
            self.endpoint = None;
            self.shared.put(&self.shared.clip, None);
            self.shared.put(&self.shared.game, None);
            self.stop_timeline();
            self.shared.put(&self.shared.speaker, None);
        }
    }
    pub fn set_game_rumble(&mut self, motors: [u8; 2]) -> Result<(), String> {
        if motors == [0, 0] {
            self.shared.put(&self.shared.game, None);
        } else {
            self.start_output()?;
            let rumble = GameRumble {
                motors,
                start_frame: self.shared.rendered_frames.load(Ordering::Relaxed),
            };
            self.shared.put(&self.shared.game, Some(Arc::new(rumble)));
        }
        Ok(())
    }
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }
    /// Bridge latency: "low" 10 ms, "normal" 20 ms, "safe" 40 ms. The jitter ring
    /// follows at once; the USB output period changes the next time the stream opens.
    pub fn set_bridge_latency(&self, latency: &str) -> Result<String, String> {
        let frames = match latency {
            "low" => 480,
            "normal" => 960,
            "safe" => 1920,
            _ => return Err("Choose low, normal or safe bridge latency.".into()),
        };
        self.shared
            .tuning
            .latency_frames
            .store(frames, Ordering::Relaxed);
        Ok(format!("Bridge latency {latency} · {} ms", frames / 48))
    }
    pub fn bridge_latency(&self) -> &'static str {
        self.shared.tuning.latency_name()
    }
    /// Overall intensity "soft" / "normal" / "strong" / "max": the grip output ceiling.
    /// Applies at once, to every grip source, and survives stream restarts.
    pub fn set_output_level(&self, level: &str) -> Result<&'static str, String> {
        let index = LEVEL_NAMES
            .iter()
            .position(|name| *name == level)
            .ok_or("Choose soft, normal, strong or max overall intensity.")?;
        self.shared
            .tuning
            .level
            .store(index as u32, Ordering::Relaxed);
        Ok(LEVEL_NAMES[index])
    }
    pub fn output_level(&self) -> &'static str {
        LEVEL_NAMES[self.shared.tuning.level_index()]
    }
    /// Parameters of the "custom" bridge preset. Takes effect live when that preset is chosen.
    pub fn set_bridge_custom(&self, custom: BridgeCustom) -> Result<(), String> {
        custom.validate()?;
        for (slot, value) in self.shared.tuning.custom.iter().zip([
            custom.cutoff,
            custom.bass,
            custom.transient,
            custom.gate,
        ]) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        Ok(())
    }
    pub fn bridge_custom(&self) -> BridgeCustom {
        let t = &self.shared.tuning;
        let get = |i: usize| f32::from_bits(t.custom[i].load(Ordering::Relaxed));
        BridgeCustom {
            cutoff: get(0),
            bass: get(1),
            transient: get(2),
            gate: get(3),
        }
    }
    /// Three-band mode, onset pulses and pan emphasis for the bridge (all off by default).
    pub fn set_bridge_options(&self, options: BridgeOptions) -> Result<(), String> {
        options.validate()?;
        let t = &self.shared.tuning;
        t.mode
            .store(u32::from(options.mode == "threeBand"), Ordering::Relaxed);
        for (slot, gain) in t.bands.iter().zip(options.band_gains) {
            slot.store(gain.to_bits(), Ordering::Relaxed);
        }
        t.onset.store(options.onset.to_bits(), Ordering::Relaxed);
        t.pan
            .store(options.pan_emphasis.to_bits(), Ordering::Relaxed);
        Ok(())
    }
    pub fn bridge_options(&self) -> BridgeOptions {
        let t = &self.shared.tuning;
        let get = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
        BridgeOptions {
            mode: if t.mode.load(Ordering::Relaxed) == 1 {
                "threeBand"
            } else {
                "standard"
            }
            .into(),
            band_gains: std::array::from_fn(|i| get(&t.bands[i])),
            onset: get(&t.onset),
            pan_emphasis: get(&t.pan),
        }
    }
    /// When a captured app exits and starts again, attach to the new process
    /// (same executable name) for up to 30 seconds. Off by default; applies to the
    /// next process capture.
    pub fn set_process_reattach(&mut self, enabled: bool) {
        self.process_reattach = enabled;
    }
    pub fn bluetooth_enabled(&self) -> bool {
        self.bluetooth_enabled
    }
    pub fn set_bluetooth(&mut self, enabled: bool) {
        self.stop();
        self.bluetooth_enabled = enabled;
    }
    pub fn take_bluetooth_packet(
        &mut self,
        now: std::time::Instant,
    ) -> Result<Option<[i8; 64]>, String> {
        self.bluetooth_output
            .as_mut()
            .map_or(Ok(None), |output| output.take(now))
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
    pub fn configure_endpoint(&mut self, id: Option<String>, error: Option<String>) {
        if self.endpoint_id != id || self.endpoint_error != error {
            self.stop();
            self.endpoint_id = id;
            self.endpoint_error = error;
        }
    }
    pub fn configured_endpoint(&self) -> Option<&str> {
        self.endpoint_id.as_deref()
    }
    pub fn capture_source(&self) -> Option<&CaptureSource> {
        self.capture_source.as_ref()
    }

    pub fn take_error(&self) -> Option<String> {
        self.shared.error.lock().ok()?.take()
    }

    /// Opening and closing the OS stream never wait on this caller.
    pub fn remember_output_failure(&mut self, error: String) {
        self.output_failure = Some((error, std::time::Instant::now()));
    }
    pub fn allow_output_retry(&mut self) {
        self.output_failure = None;
    }
    pub fn output_failed(&self) -> bool {
        self.output_failure.is_some()
    }
    /// Seconds until a failed endpoint open is retried (0 when not waiting).
    pub fn output_retry_seconds(&self) -> u64 {
        self.output_failure.as_ref().map_or(0, |(_, at)| {
            OUTPUT_RETRY
                .saturating_sub(at.elapsed())
                .as_secs_f64()
                .ceil() as u64
        })
    }
    pub fn start_output(&mut self) -> Result<&str, String> {
        if self.bluetooth_enabled {
            if self.bluetooth_output.is_none() {
                self.fresh_output_state();
                self.bluetooth_output = Some(bluetooth::Output::start(self.shared.clone())?);
                self.duty_saved = None;
                self.endpoint = Some("Bluetooth grip PCM · experimental".into());
                self.output_startup_ms = Some(0);
            }
            return Ok(self.endpoint.as_deref().unwrap_or("Bluetooth grips"));
        }
        if let Some((error, failed_at)) = &self.output_failure
            && failed_at.elapsed() < OUTPUT_RETRY
        {
            return Err(error.clone());
        }
        self.output_failure = None;
        if let Some(error) = &self.endpoint_error {
            return Err(error.clone());
        }
        if self.output.is_none() {
            // A cancelled worker retains its old shared state. It cannot consume a
            // replacement stream's cues, frames, errors, or file queues.
            self.fresh_output_state();
            self.retired_outputs
                .retain(|done| !done.load(Ordering::Acquire));
            self.output_startup_ms = None;
            self.output = Some(io_runtime::OutputWorker::start(
                self.shared.clone(),
                self.endpoint_id.clone(),
                self.retired_outputs.clone(),
            )?);
            self.duty_saved = None;
        }
        Ok(self
            .endpoint
            .as_deref()
            .unwrap_or("Opening controller audio"))
    }
    pub fn output_ready(&self) -> bool {
        (self.output.is_some() || self.bluetooth_output.is_some()) && self.endpoint.is_some()
    }
    pub fn output_opening(&self) -> bool {
        self.output.is_some() && self.endpoint.is_none()
    }
    pub fn poll_output(&mut self) -> Result<(), String> {
        if let Some(worker) = self.output.as_mut()
            && let Some(result) = worker.poll()
        {
            self.output_startup_ms = Some(worker.elapsed_ms());
            match result {
                Ok(name) => self.endpoint = Some(name),
                Err(error) => {
                    self.retire_output();
                    self.endpoint = None;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub fn play(&mut self, mut clip: VoiceClip) -> Result<&str, String> {
        self.start_output()?;
        clip.start_frame = self.shared.rendered_frames.load(Ordering::Relaxed);
        self.shared.put(&self.shared.clip, Some(Arc::new(clip)));
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn play_link_test(&mut self, strength: u8) -> Result<(), String> {
        let mut clip = VoiceClip::new("sine", 80, 2, 650, strength.min(25), 0);
        clip.shape = Shape::LinkTest;
        clip.attack = 0.0;
        clip.release = 2.0;
        self.play(clip).map(|_| ())
    }
    pub fn play_speaker_chirp(&mut self) -> Result<&str, String> {
        if self.bluetooth_enabled {
            return Err(
                "The Bluetooth PCM route supports grips only. Use USB for the speaker.".into(),
            );
        }
        self.start_output()?;
        self.stop_speaker_pcm();
        let chirp = SpeakerChirp {
            start_frame: self.shared.rendered_frames.load(Ordering::Relaxed),
        };
        self.shared.put(&self.shared.speaker, Some(Arc::new(chirp)));
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
        if self.bluetooth_enabled && route != "grips" {
            return Err("Choose Grips only for Bluetooth playback.".into());
        }
        self.start_output()?;
        let stream = if append {
            let active = self
                .shared
                .speaker_pcm
                .load_full()
                .ok_or("The track stopped. Discarding queued audio.")?;
            if active.native
                || active.speaker != (route != "grips")
                || active.grips != (route != "speaker")
            {
                return Err("Sound route changed. Start the track again.".into());
            }
            active
        } else {
            self.stop_speaker_pcm();
            self.legacy_filter = GripFilter::default();
            Arc::new(PcmStream::new(0, route, false, 0))
        };
        if stream.queue.capacity() - stream.queue.len() < samples.len() {
            return Err("The audio queue is full. Wait before appending another chunk.".into());
        }
        let added = samples.len() as u64;
        for sample in samples {
            let x = sample as f32 / i16::MAX as f32;
            let grip = self.legacy_filter.frame(x);
            stream
                .queue
                .push(routed_frame([x, x], [grip, grip], &stream))
                .map_err(|_| "Audio queue is full".to_string())?;
        }
        stream.total.fetch_add(added, Ordering::Release);
        stream
            .tail_frames
            .store(RATE as u64 / 10, Ordering::Release);
        if !append {
            self.shared.put(&self.shared.speaker_pcm, Some(stream));
        }
        Ok(self.endpoint.as_deref().unwrap_or("DualSense"))
    }

    pub fn play_file(
        &mut self,
        path: String,
        range: (f64, f64),
        route: &str,
        kind: &str,
        track_id: u64,
    ) -> Result<(), String> {
        let (start, duration) = range;
        if self.bluetooth_enabled && route != "grips" {
            return Err("Choose Grips only for Bluetooth playback.".into());
        }
        if !start.is_finite()
            || !(0.0..3600.0).contains(&start)
            || !duration.is_finite()
            || duration <= start
            || duration > 3600.0
        {
            return Err("Choose a position inside a track up to one hour long.".into());
        }
        if !matches!(route, "speaker" | "both" | "grips") || !file::valid_kind(kind) {
            return Err("Choose a valid audio route and content type.".into());
        }
        self.start_output()?;
        self.stop_speaker_pcm();
        let stream = Arc::new(PcmStream::new(
            track_id,
            route,
            true,
            ((duration - start) * RATE as f64).round() as u64,
        ));
        file::spawn_conversion(path, start, kind.into(), stream.clone())?;
        self.shared.put(&self.shared.speaker_pcm, Some(stream));
        Ok(())
    }
    pub fn set_mixer(&self, settings: MixerSettings) -> Result<(), String> {
        settings.validate()?;
        self.shared
            .retire(Some(self.shared.mixer.swap(Arc::new(settings))));
        Ok(())
    }
    pub fn stop_speaker_pcm(&mut self) {
        self.shared.put(&self.shared.speaker, None);
        if let Some(stream) = self.shared.speaker_pcm.swap(None) {
            stream.canceled.store(true, Ordering::Release);
            self.shared.retire(Some(stream));
        }
    }
    pub fn pause_speaker_pcm(&mut self, paused: bool) -> Result<SpeakerPlayback, String> {
        let stream = self.shared.speaker_pcm.load();
        let stream = stream.as_ref().ok_or("No track is playing.")?;
        stream.paused.store(paused, Ordering::Release);
        Ok(SpeakerPlayback {
            paused,
            position: stream.position() as f32,
            remaining: stream.remaining(),
            speaker: stream.speaker,
        })
    }
    pub fn take_file_error(&self) -> Option<(u64, String)> {
        let stream = self.shared.speaker_pcm.load();
        let stream = stream.as_ref()?;
        let error = stream.error.lock().ok()?.take()?;
        Some((stream.track_id, error))
    }
    pub fn file_status(&self) -> Option<serde_json::Value> {
        let stream = self.shared.speaker_pcm.load();
        let stream = stream.as_ref()?;
        Some(
            serde_json::json!({"trackId":stream.track_id,"native":stream.native,
            "mode":stream.mode(),"position":stream.position(),"remaining":stream.remaining(),
            "duration":stream.total.load(Ordering::Relaxed) as f64/RATE as f64,
            "speaker":stream.speaker,"grips":stream.grips,"queueMs":stream.queue.len() as f64/48.0}),
        )
    }
    /// Status snapshot for the 250 ms emitter. Reading resets the peak and
    /// reduction meters, so only the emitter should call it; everyone else uses
    /// `peek_diagnostics`.
    pub fn diagnostics(&self) -> serde_json::Value {
        self.snapshot(true)
    }
    /// Same data as `diagnostics` without touching the peak meters.
    #[allow(dead_code)] // read-only snapshot for tests and future non-emitter callers
    pub fn peek_diagnostics(&self) -> serde_json::Value {
        self.snapshot(false)
    }
    fn snapshot(&self, reset: bool) -> serde_json::Value {
        let clip = self.shared.clip.load();
        let now = self.shared.rendered_frames.load(Ordering::Relaxed);
        let cue_active = clip.as_ref().is_some_and(|clip| {
            clip.strength > 0.0
                && clip
                    .sample(now.saturating_sub(clip.start_frame) as f32 / RATE as f32)
                    .is_some()
        });
        let read = |meter: &AtomicU32| {
            f32::from_bits(if reset {
                meter.swap(0, Ordering::Relaxed)
            } else {
                meter.load(Ordering::Relaxed)
            })
        };
        let peaks: Vec<f32> = self.shared.peaks.iter().map(&read).collect();
        let mut value = serde_json::json!({"type":"audioMeters","outputStage":if self.output_ready(){"ready"}else if self.output_opening(){"starting"}else{"off"},"outputStartupMs":self.output_startup_ms,"personal":&**self.shared.personal.load(),"mixer":&**self.shared.mixer.load(),"endpoint":self.endpoint,"sampleRate":RATE,
            "active":self.output.is_some(),"captureSource":self.capture_source,"captureState":self.capture.as_ref().map(|c|c.state()).unwrap_or("off"),"bridge":self.bridge_on(),"file":self.file_status(),"cueActive":cue_active,
            "timelineActive":self.shared.timeline.load().as_ref().is_some_and(|t|now.saturating_sub(t.start_frame)<u64::from(t.clip.duration_ms())*48),
            "gameActive":self.shared.game_active(now),
            "peaks":peaks,"reduction":read(&self.shared.reduction),
            "fileUnderruns":self.shared.file_underruns.load(Ordering::Relaxed),
            "bridgeUnderruns":self.shared.bridge_underruns.load(Ordering::Relaxed),
            "bridgeHoldFrames":self.shared.bridge_hold_frames.load(Ordering::Relaxed),
            "bridgeDropped":self.shared.bridge_dropped.load(Ordering::Relaxed),"glitches":self.glitches(),
            "bridgeQueueMs":self.shared.bridge.len() as f64/48.0});
        let capture_device = self
            .shared
            .capture_device
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .map(|(name, rate)| serde_json::json!({"name":name,"sampleRate":rate}));
        let capture_status = self
            .shared
            .capture_status
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        let bluetooth = self.bluetooth_output.as_ref().map(
            |output| serde_json::json!({"dropped":output.dropped_total(),"underruns":output.underruns}),
        );
        let held = self.output_ready() || self.output_opening();
        let extra = serde_json::json!({
            "captureDevice":capture_device,
            "captureStatus":capture_status,
            "captureInput":{"peak":read(&self.shared.capture_in_peak),
                "rms":f32::from_bits(self.shared.capture_in_rms.load(Ordering::Relaxed))},
            "endpointHolder":if held {Some("controller")} else {None},
            "keepOpenBy":self.active_sources(),
            "bluetoothOutput":bluetooth,
            "thermalLimitActive":self.shared.thermal_active.load(Ordering::Relaxed),
            "outputRetrySeconds":self.output_retry_seconds(),
            "outputFailure":self.output_failure.as_ref().map(|(error, _)| error),
            "outputUnderruns":self.shared.output_underruns.load(Ordering::Relaxed),
            "bridgeLatency":self.bridge_latency(),
            "hapticOutputLevel":self.output_level(),
            "outputCeiling":self.shared.ceiling(),
            "gripSums":{"abs":self.shared.grip_abs.iter().map(|a|a.load(Ordering::Relaxed)).collect::<Vec<_>>(),
                "sq":self.shared.grip_sq.iter().map(|a|a.load(Ordering::Relaxed)).collect::<Vec<_>>(),
                "frames":self.shared.rendered_frames.load(Ordering::Relaxed)},
            "bridgeCustom":self.bridge_custom(),
            "bridgeOptions":self.bridge_options(),
            "processReattach":self.process_reattach,
        });
        if let (Some(base), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            for (key, field) in extra {
                base.insert(key.clone(), field.clone());
            }
        }
        value
    }

    pub fn set_bridge(&mut self, enabled: bool, gain: f32, preset: &str) -> Result<String, String> {
        self.set_capture(enabled, gain, preset, CaptureSource::System)
    }
    fn drop_capture(&mut self) {
        self.shared.bridge_on.store(false, Ordering::Release);
        self.shared
            .capture_generation
            .fetch_add(1, Ordering::AcqRel);
        self.capture = None;
        self.capture_source = None;
        if let Some(old) = self.capture_shared.take()
            && !Arc::ptr_eq(&old, &self.shared)
        {
            // A worker bound to a retired Shared: end it too.
            old.bridge_on.store(false, Ordering::Release);
            old.capture_generation.fetch_add(1, Ordering::AcqRel);
        }
        while self.shared.bridge.pop().is_some() {}
        self.shared.clear_capture_info();
    }
    pub fn take_capture_error(&mut self) -> Option<String> {
        let error = if self.capture.as_ref().is_some_and(CaptureHandle::timed_out) {
            Some(
                "Process audio activation timed out; capture stopped without switching sources"
                    .into(),
            )
        } else {
            self.shared.capture_error.lock().ok()?.take()
        };
        if error.is_some() {
            self.drop_capture();
        }
        error
    }
    pub fn set_capture(
        &mut self,
        enabled: bool,
        gain: f32,
        preset: &str,
        source: CaptureSource,
    ) -> Result<String, String> {
        if !enabled {
            self.drop_capture();
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
            "custom" => (PRESET_CUSTOM, 1.0),
            _ => return Err("Choose a valid audio bridge preset.".into()),
        };
        source.validate_runtime()?;
        self.start_output()?;
        self.shared.gain_bits.store(
            (gain
                * if matches!(source, CaptureSource::NativeEndpoint { .. }) {
                    1.0
                } else {
                    preset_gain
                })
            .to_bits(),
            Ordering::Relaxed,
        );
        self.shared.preset.store(preset_id, Ordering::Relaxed);
        // A restarted output replaces Shared; a worker feeding the old one is dead weight.
        let capture_stale = self
            .capture_shared
            .as_ref()
            .is_some_and(|started| !Arc::ptr_eq(started, &self.shared));
        if self.capture.is_none() || capture_stale || self.capture_source.as_ref() != Some(&source)
        {
            self.drop_capture();
            if let Ok(mut slot) = self.shared.capture_error.lock() {
                *slot = None;
            }
            self.shared.glitches.store(0, Ordering::Relaxed);
            self.shared.bridge_frames.store(0, Ordering::Relaxed);
            // The feedback guard resolves the paired endpoint's name itself (see
            // build_loopback), so a renamed controller endpoint is rejected even
            // before the output stream has reported its name.
            self.capture = Some(match &source {
                CaptureSource::System => CaptureHandle::System {
                    _worker: io_runtime::CaptureWorker::start(
                        self.shared.clone(),
                        None,
                        self.endpoint.clone(),
                        self.endpoint_id.clone(),
                    )?,
                },
                CaptureSource::NativeEndpoint { id } => CaptureHandle::System {
                    _worker: io_runtime::CaptureWorker::start(
                        self.shared.clone(),
                        Some(id.clone()),
                        self.endpoint.clone(),
                        self.endpoint_id.clone(),
                    )?,
                },
                #[cfg(windows)]
                CaptureSource::Process { pid } => CaptureHandle::Process {
                    _worker: capture::process::ProcessCapture::start(
                        self.shared.clone(),
                        *pid,
                        self.process_reattach,
                    )?,
                },
                #[cfg(not(windows))]
                CaptureSource::Process { .. } => {
                    return Err("Process audio capture requires Windows".into());
                }
            });
            self.capture_source = Some(source);
            self.capture_shared = Some(self.shared.clone());
        }
        self.shared.bridge_on.store(true, Ordering::Relaxed);
        Ok(format!(
            "Audio bridge on · {}",
            self.endpoint.as_deref().unwrap_or("DualSense")
        ))
    }

    pub fn stop(&mut self) {
        self.output_failure = None;
        self.stop_timeline();
        self.shared.put(&self.shared.game, None);
        self.shared.bridge_on.store(false, Ordering::Relaxed);
        self.shared
            .capture_generation
            .fetch_add(1, Ordering::AcqRel);
        self.capture = None;
        self.capture_source = None;
        self.capture_shared = None;
        self.shared.clear_capture_info();
        self.retire_output();
        if let Ok(mut error) = self.shared.error.lock() {
            *error = None;
        }
        self.endpoint = None;
        self.shared.put(&self.shared.clip, None);
        self.stop_speaker_pcm();
        while self.shared.bridge.pop().is_some() {}
    }
}

#[cfg(not(windows))]
fn run_portable_output(
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<String, String>>,
    endpoint_id: Option<String>,
) -> Result<(), String> {
    let host = cpal::default_host();
    let mut candidate = None;
    let mut eligible_endpoints = 0usize;
    for device in host.output_devices().map_err(|e| e.to_string())? {
        let name = device.to_string();
        let lower = name.to_ascii_lowercase();
        if let Some(id) = &endpoint_id {
            if !device.id().is_ok_and(|d| d.to_string() == *id) {
                continue;
            }
        } else if !lower.contains("dualsense") && !lower.contains("wireless controller") {
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
    let (device,name,format)=candidate.ok_or_else(||if endpoint_id.is_some(){"The paired audio endpoint is unavailable or its format changed. Refresh the pairing in Settings > Controllers.".to_string()}else{"No four-channel DualSense USB audio endpoint at 48 kHz. Connect by USB and enable its audio output in system settings.".to_string()})?;
    let config = StreamConfig {
        channels: 4,
        sample_rate: RATE,
        buffer_size: BufferSize::Default,
    };
    let shared = shared.clone();
    let error_shared = shared.clone();
    let on_error = move |error: cpal::Error| {
        record_audio_error(&error_shared, error, "USB haptic audio");
    };
    let mut frame_position = 0u64;
    let mut duty = RenderState::with_duty(shared.duty_load());
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
            let shared = shared.clone();
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
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let _ = ready.send(Ok(name));
    while !stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(10));
    }
    drop(stream);
    Ok(())
}

#[cfg(windows)]
fn run_exclusive_output(
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<String, String>>,
    endpoint_id: Option<String>,
) -> Result<(), String> {
    use wasapi::{DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WaveFormat};

    /// Balances `initialize_mta`; declared first so it drops after the COM objects below.
    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            wasapi::deinitialize();
        }
    }
    /// Raises this thread to the MMCSS "Pro Audio" class for the render loop.
    struct Mmcss(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Mmcss {
        fn drop(&mut self) {
            unsafe {
                windows_sys::Win32::System::Threading::AvRevertMmThreadCharacteristics(self.0)
            };
        }
    }
    if let Err(error) = wasapi::initialize_mta()
        .ok()
        .map_err(|e| format!("COM audio setup failed: {e}"))
    {
        let _ = ready.send(Err(error.clone()));
        return Err(error);
    }
    let _com = ComGuard;

    let latency_frames = shared.tuning.latency_frames.load(Ordering::Relaxed);
    let setup = (|| {
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
            if let Some(id) = &endpoint_id {
                if device.get_id().map_err(|e| e.to_string())? != *id {
                    continue;
                }
            } else if !lower.contains("dualsense") && !lower.contains("wireless controller") {
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
        let (name, mut client) = selected.ok_or_else(||if endpoint_id.is_some(){"The paired audio endpoint is unavailable or its format changed. Refresh the pairing in Settings > Controllers.".to_string()}else{"No four-channel DualSense USB audio endpoint at 48 kHz. Connect the pad by USB.".to_string()})?;
        let (_, minimum_period) = client.get_device_period().map_err(|e| e.to_string())?;
        // Bridge latency also sets the USB period: low 5 ms, normal 10 ms, safe 20 ms.
        let wanted = if latency_frames <= 480 {
            50_000
        } else if latency_frames <= 960 {
            100_000
        } else {
            200_000
        };
        let period = minimum_period.max(wanted);
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
        let buffer_frames = client.get_buffer_size().map_err(|e| e.to_string())? as usize;
        Ok::<_, String>((name, client, render, buffer_frames, period))
    })();
    let (name, client, render, buffer_frames, period) = match setup {
        Ok(value) => value,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let mut position = 0;
    let mut duty = RenderState::with_duty(shared.duty_load());
    let mut samples = Vec::new();
    let mut bytes = Vec::new();
    let mut first_write = true;
    let mut write_available = || -> Result<(), String> {
        let frames = client
            .get_available_space_in_frames()
            .map_err(|e| e.to_string())? as usize;
        if frames == 0 {
            return Ok(());
        }
        // Everything free means the device played its whole buffer dry. The first
        // write legitimately sees an empty buffer, so it is not counted.
        if !first_write && frames >= buffer_frames {
            shared.output_underruns.fetch_add(1, Ordering::Relaxed);
        }
        first_write = false;
        samples.resize(frames * 4, 0);
        render_output_i16(&mut samples, &shared, &mut position, &mut duty);
        bytes.clear();
        bytes.reserve(samples.len() * 2);
        for sample in &samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        render
            .write_to_device(frames, &bytes, None)
            .map_err(|e| format!("DualSense USB audio write failed: {e}"))
    };
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    if let Err(error) = write_available().and_then(|_| {
        client
            .start_stream()
            .map_err(|e| format!("Could not start exclusive DualSense audio: {e}"))
    }) {
        let _ = ready.send(Err(error.clone()));
        return Err(error);
    }
    let _ = ready.send(Ok(name));
    let name_wide: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
    let mut task_index = 0u32;
    let handle = unsafe {
        windows_sys::Win32::System::Threading::AvSetMmThreadCharacteristicsW(
            name_wide.as_ptr(),
            &mut task_index,
        )
    };
    let _mmcss = (!handle.is_null()).then(|| Mmcss(handle));
    // Poll about three times per period instead of a fixed 3 ms.
    let wait = Duration::from_micros((period as u64 / 10 / 3).clamp(1_500, 6_000));
    while !stop.load(Ordering::Acquire) {
        write_available()?;
        thread::sleep(wait);
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
    bass: f32,
    impact: f32,
    slow_energy: f32,
}

impl GripFilter {
    fn frame_conv(&mut self, x: f32, mode: ConvMode) -> f32 {
        let full = self.frame(x);
        self.bass += 0.012 * (self.high * 3.0 - self.bass);
        self.slow_energy += 0.00008 * (self.energy - self.slow_energy);
        self.impact = (self.impact * 0.99965)
            .max((self.energy - self.slow_energy * 1.4).max(0.0).sqrt() * 2.4);
        match mode {
            ConvMode::Bass => self.bass,
            ConvMode::Impact => self.phase.sin() * self.impact,
            ConvMode::Texture => {
                (self.phase * 1.3).sin()
                    * (self.treble_energy.sqrt() * 2.0 + self.energy.sqrt() * 0.25)
            }
            _ => full,
        }
        .clamp(-1.0, 1.0)
    }
    #[cfg(test)]
    fn frame_mode(&mut self, x: f32, mode: &str) -> f32 {
        self.frame_conv(x, ConvMode::parse(mode))
    }
    #[cfg(test)]
    fn process(&mut self, samples: &[i16]) -> Vec<f32> {
        samples
            .iter()
            .map(|x| self.frame(*x as f32 / i16::MAX as f32))
            .collect()
    }
    fn frame(&mut self, x: f32) -> f32 {
        let x = if x.is_finite() {
            x.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let lp_alpha = std::f32::consts::TAU * 250.0 / RATE as f32;
        let hp_alpha = RATE as f32 / (RATE as f32 + std::f32::consts::TAU * 35.0);
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
        let carrier =
            self.phase.sin() * (self.energy.sqrt() * 0.75 + self.treble_energy.sqrt() * 1.2);
        (self.high * 3.0 + carrier).clamp(-1.0, 1.0)
    }
}

#[cfg(any(test, not(windows)))]
fn record_audio_error(shared: &Shared, error: cpal::Error, route: &str) {
    if error.kind() == cpal::ErrorKind::Xrun {
        // CPAL continues streaming after a WASAPI discontinuity; a glitch is not a stopped stream.
        shared.glitches.fetch_add(1, Ordering::Relaxed);
    } else if let Ok(mut slot) = shared.error.lock() {
        *slot = Some(format!("{route} stopped: {error}"));
    }
}

fn record_capture_error(shared: &Shared, generation: u64, error: cpal::Error) {
    if shared.capture_generation.load(Ordering::Acquire) != generation {
        return;
    }
    if error.kind() == cpal::ErrorKind::Xrun {
        shared.glitches.fetch_add(1, Ordering::Relaxed);
    } else if shared.capture_reopen.load(Ordering::Acquire) == generation + 1 {
        // The capture worker is following the default device and re-opens it.
        shared.capture_lost.store(true, Ordering::Release);
    } else if let Ok(mut slot) = shared.capture_error.lock()
        && shared.capture_generation.load(Ordering::Acquire) == generation
    {
        *slot = Some(format!("Audio capture stopped: {error}"));
    }
}

/// Soft-knee limiter on a normalized value: exactly linear up to `KNEE`, then
/// rounded smoothly (continuous slope) into +/-1. Monotonic, odd, and never
/// reaches 1, so 30/60/100 % strengths stay proportional instead of saturating.
fn knee_curve(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    let room = 1.0 - KNEE;
    (KNEE + room * ((a - KNEE) / room).tanh()).copysign(x)
}

/// Sustained average-drive cap for an output ceiling (see `LEVEL_DUTY_CAPS`).
/// Unknown ceilings fall back to half the ceiling. Never above `HARD_DUTY_CAP`.
fn duty_cap(ceiling: f32) -> f32 {
    LEVEL_CEILINGS
        .iter()
        .position(|c| (*c - ceiling).abs() < 1e-6)
        .map_or(ceiling * 0.5, |i| LEVEL_DUTY_CAPS[i])
        .clamp(0.02, HARD_DUTY_CAP)
}

/// `value` of 1.0 is a full-strength source. The result never exceeds `ceiling`.
/// The thermal limiter watches the average drive (about 1 s window) and its cap
/// scales with the ceiling (`duty_cap`), bounded by `HARD_DUTY_CAP`.
#[cfg(test)]
fn soft_limit(value: f32, duty: &mut f32, ceiling: f32) -> f32 {
    soft_limit_capped(value, duty, ceiling, duty_cap(ceiling))
}

/// `soft_limit` with an explicit (possibly smoothed) thermal cap.
fn soft_limit_capped(value: f32, duty: &mut f32, ceiling: f32, cap: f32) -> f32 {
    let value = if value.is_finite() { value } else { 0.0 };
    let bounded = knee_curve(value) * ceiling;
    *duty = *duty * 0.99998 + bounded.abs() * 0.00002;
    let thermal = if *duty > cap { cap / *duty } else { 1.0 };
    (bounded * thermal).clamp(-ceiling, ceiling)
}

fn fill_frames(
    mut write: impl FnMut(usize, [f32; 4]),
    frames: usize,
    shared: &Shared,
    position: &mut u64,
    state: &mut RenderState,
) {
    let clip_guard = shared.clip.load();
    let timeline_guard = shared.timeline.load();
    let personal = shared.personal.load();
    let game_guard = shared.game.load();
    let speaker_guard = shared.speaker.load();
    let pcm_guard = shared.speaker_pcm.load();
    let mixer = shared.mixer.load();
    let clip = clip_guard.as_ref().map(|c| **c);
    let speaker = speaker_guard.as_ref().map(|c| **c);
    let pcm = pcm_guard.as_ref().filter(|sound| {
        !sound.paused.load(Ordering::Acquire)
            && sound.ready.load(Ordering::Acquire)
            && !sound.canceled.load(Ordering::Acquire)
    });
    let bridge_on = shared.bridge_on.load(Ordering::Relaxed);
    let prefill = (shared.tuning.latency_frames.load(Ordering::Relaxed) as usize / 2).max(1);
    if !bridge_on {
        state.bridge_primed = false;
    }
    let solo = solo_code(&mixer.solo);
    let mut speaker_frames = 0;
    let mut grip_frames = 0;
    let mut consumed = 0;
    let mut starved = false;
    let mut missing = 0;
    let mut bridge_missing = 0u64;
    let mut peaks = [0.0f32; 3];
    let mut reduction = 0.0f32;
    let target_ceiling = shared.ceiling();
    let target_cap = duty_cap(target_ceiling);
    if state.ceiling <= 0.0 {
        state.ceiling = target_ceiling;
        state.cap = target_cap;
    }
    let (mut sums, mut squares) = ([0.0f64; 2], [0.0f64; 2]);
    for index in 0..frames {
        let frame = *position + index as u64;
        let tone = clip.and_then(|c| {
            frame
                .checked_sub(c.start_frame)
                .and_then(|at| c.sample(at as f32 / RATE as f32))
        });
        let audio = if bridge_on {
            let (samples, missed) = bridge_sample(shared, state, prefill);
            bridge_missing += u64::from(missed);
            samples
        } else {
            [0.0; 2]
        };
        let file = pcm
            .and_then(|sound| {
                if starved {
                    return None;
                }
                match sound.queue.pop() {
                    Some(frame) => {
                        consumed += 1;
                        Some(frame)
                    }
                    None => {
                        if !sound.eof.load(Ordering::Acquire) {
                            starved = true;
                            sound.ready.store(false, Ordering::Release);
                            missing += 1;
                        }
                        None
                    }
                }
            })
            .unwrap_or([0.0; 4]);
        let game = game_guard.as_ref().map_or([0.0; 2], |g| g.sample(frame));
        let background = [
            audio[0] * mixer.bridge_gain + file[2] * mixer.file_gain + game[0] * mixer.game_gain,
            audio[1] * mixer.bridge_gain + file[3] * mixer.file_gain + game[1] * mixer.game_gain,
        ];
        let timeline_seconds = timeline_guard
            .as_ref()
            .map(|t| frame.saturating_sub(t.start_frame) as f32 / RATE as f32);
        let timeline = timeline_guard
            .as_ref()
            .zip(timeline_seconds)
            .map_or([0.0; 2], |(t, seconds)| {
                t.compiled.sample(seconds, &personal)
            });
        let authored = std::array::from_fn(|side| {
            let strength = clip.map_or(0.0, |c| c.strength);
            tone.unwrap_or([0.0; 2])[side]
                * if strength > 0.0 {
                    personal.authored_strength(strength, side) / strength
                } else {
                    1.0
                }
                + timeline[side]
        });
        let timeline_active = timeline_guard
            .as_ref()
            .zip(timeline_seconds)
            .is_some_and(|(t, seconds)| t.compiled.active(seconds));
        let mixed = state.mix_grips_solo(
            authored,
            background,
            &mixer,
            mixer.cue_gain > 0.0
                && ((tone.is_some() && clip.is_some_and(|c| c.strength > 0.0)) || timeline_active),
            solo,
        );
        let raw_test = tone.is_some() && clip.is_some_and(|c| c.raw_test);
        state.ceiling += (target_ceiling - state.ceiling) * LEVEL_SMOOTH;
        state.cap += (target_cap - state.cap) * LEVEL_SMOOTH;
        let (ceiling, cap) = (state.ceiling, state.cap);
        let l = soft_limit_capped(
            if raw_test { tone.unwrap()[0] } else { mixed[0] },
            &mut state.duty[0],
            ceiling,
            cap,
        ) * if raw_test {
            1.0
        } else {
            personal.side_limit(0)
        };
        let r = soft_limit_capped(
            if raw_test { tone.unwrap()[1] } else { mixed[1] },
            &mut state.duty[1],
            ceiling,
            cap,
        ) * if raw_test {
            1.0
        } else {
            personal.side_limit(1)
        };
        let chirp = speaker
            .and_then(|c| {
                frame
                    .checked_sub(c.start_frame)
                    .and_then(|at| c.sample(at as f32 / RATE as f32))
            })
            .unwrap_or(0.0);
        let front = ((chirp + file[0]) * mixer.speaker_gain).clamp(-0.75, 0.75);
        peaks[0] = peaks[0].max(front.abs() / SPEAKER_FILE_GAIN);
        // Meters read as a fraction of the selected ceiling.
        peaks[1] = peaks[1].max(l.abs() / ceiling);
        peaks[2] = peaks[2].max(r.abs() / ceiling);
        for (side, (input, output)) in mixed.into_iter().zip([l, r]).enumerate() {
            let requested = input.abs() * ceiling;
            if requested > 0.01 {
                reduction = reduction.max(1.0 - output.abs() / requested);
            }
            let (abs, square) = (f64::from(output.abs()), f64::from(output * output));
            sums[side] += abs;
            squares[side] += square;
        }
        speaker_frames += u64::from(front.abs() > 0.005);
        grip_frames += u64::from(l.abs() > 0.005 || r.abs() > 0.005);
        write(index, [front, front, l, r]);
    }
    if let Some(sound) = pcm {
        sound.consumed.fetch_add(consumed, Ordering::Relaxed);
        if sound.eof.load(Ordering::Acquire) && sound.queue.is_empty() {
            let silence = (frames as u64).saturating_sub(consumed);
            // CAS keeps this compatible with Rust versions on either side of
            // the fetch_update -> try_update API transition.
            let mut tail = sound.tail_frames.load(Ordering::Relaxed);
            loop {
                match sound.tail_frames.compare_exchange_weak(
                    tail,
                    tail.saturating_sub(silence),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(current) => tail = current,
                }
            }
        }
    }
    shared.file_underruns.fetch_add(missing, Ordering::Relaxed);
    // One underrun per gap (primed -> hold transition); held frames are
    // tracked separately so a silent source does not inflate the counter.
    shared
        .bridge_underruns
        .fetch_add(bridge_missing, Ordering::Relaxed);
    let held = std::mem::take(&mut state.bridge_held_frames);
    if held > 0 {
        shared.bridge_hold_frames.fetch_add(held, Ordering::Relaxed);
    }
    for side in 0..2 {
        shared.grip_abs[side].fetch_add((sums[side] * 1e6) as u64, Ordering::Relaxed);
        shared.grip_sq[side].fetch_add((squares[side] * 1e9) as u64, Ordering::Relaxed);
    }
    // USB and Bluetooth renderers each keep their own duty; both store here, so the last
    // writer wins. They only run together while both outputs are enabled, which is rare.
    shared.duty_store(state.duty);
    shared
        .thermal_active
        .store(state.duty.iter().any(|d| *d > state.cap), Ordering::Relaxed);
    for (peak, level) in shared.peaks.iter().zip(peaks) {
        peak.fetch_max(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    shared
        .reduction
        .fetch_max(reduction.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    shared
        .speaker_frames
        .fetch_add(speaker_frames, Ordering::Relaxed);
    shared.grip_frames.fetch_add(grip_frames, Ordering::Relaxed);
    *position += frames as u64;
    shared.rendered_frames.store(*position, Ordering::Relaxed);
    shared.callbacks.fetch_add(1, Ordering::Relaxed);
}

/// One bridge frame with a jitter buffer: playback starts once the ring is half
/// full, an empty ring fades the last frame out instead of cutting to zero, and
/// every 512 frames a nearly full/empty ring drops/repeats one frame to follow
/// clock drift. Returns the frame and whether this frame starts a new gap (an
/// underrun is counted once per gap; held frames go to `bridge_held_frames`).
fn bridge_sample(shared: &Shared, state: &mut RenderState, prefill: usize) -> ([f32; 2], bool) {
    if !state.bridge_primed {
        let queued = shared.bridge.len();
        if queued < prefill {
            // A quiet source may never fill the prefill. Prime anyway once a
            // non-empty queue has stopped growing for ~10 ms of output.
            if queued == 0 || queued > state.bridge_wait_len {
                state.bridge_wait = 0;
            } else {
                state.bridge_wait += 1;
            }
            state.bridge_wait_len = queued;
            if queued == 0 || state.bridge_wait < 480 {
                return ([0.0; 2], false);
            }
        }
        state.bridge_wait = 0;
        state.bridge_wait_len = 0;
        state.bridge_primed = true;
        state.bridge_gap = 0;
        state.bridge_hold = 1.0;
        state.drift_tick = 0;
    }
    state.drift_tick += 1;
    if state.drift_tick >= 512 {
        state.drift_tick = 0;
        let queued = shared.bridge.len();
        if queued > prefill + prefill / 2 + 64 {
            let _ = capture_sample(shared);
        } else if queued > 0 && queued < prefill / 2 {
            return (state.bridge_last, false);
        }
    }
    match capture_sample(shared) {
        Some(frame) => {
            state.bridge_last = frame;
            state.bridge_gap = 0;
            state.bridge_hold = 1.0;
            (frame, false)
        }
        None => {
            state.bridge_gap += 1;
            let new_gap = state.bridge_gap == 1;
            if state.bridge_gap > 960 {
                state.bridge_primed = false;
                state.bridge_last = [0.0; 2];
                return ([0.0; 2], false);
            }
            state.bridge_held_frames += 1;
            state.bridge_hold *= 0.995;
            (state.bridge_last.map(|v| v * state.bridge_hold), new_gap)
        }
    }
}

fn render_output(data: &mut [f32], shared: &Shared, position: &mut u64, duty: &mut RenderState) {
    let frames = data.len() / 4;
    fill_frames(
        |i, frame| data[i * 4..i * 4 + 4].copy_from_slice(&frame),
        frames,
        shared,
        position,
        duty,
    );
}

fn render_output_i16(
    data: &mut [i16],
    shared: &Shared,
    position: &mut u64,
    duty: &mut RenderState,
) {
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct CaptureFrame {
    generation: u64,
    samples: [f32; 2],
}

fn capture_sample(shared: &Shared) -> Option<[f32; 2]> {
    let generation = shared.capture_generation.load(Ordering::Acquire);
    // A retired callback may finish after a source change. Never play its data.
    for _ in 0..64 {
        let frame = shared.bridge.pop()?;
        if frame.generation == generation {
            return Some(frame.samples);
        }
    }
    None
}

struct CaptureDsp {
    generation: u64,
    low: [f32; 2],
    high: [f32; 2],
    previous_low: [f32; 2],
    phase: f32,
    envelope: [f32; 2],
    resample: u64,
    gain: f32,
    rate: u32,
    dropped: u64,
    preset_id: u32,
    low_alpha: f32,
    bass_gain: f32,
    transient_gain: f32,
    gate: f32,
    native: bool,
    /// Ring fill above which the oldest frame is dropped (latency setting).
    limit: usize,
    previous_out: [f32; 2],
    custom: [u32; 4],
    three_band: bool,
    bands: [ThreeBand; 2],
    band_gains: [f32; 3],
    onset: OnsetDetector,
    onset_gain: f32,
    pan: f32,
}

impl CaptureDsp {
    fn new(rate: u32, gain: f32) -> Self {
        let rate = rate.clamp(8_000, 192_000);
        let gain = if gain.is_finite() {
            gain.clamp(0.0, 4.0)
        } else {
            0.0
        };
        let mut dsp = Self {
            generation: 0,
            low: [0.0; 2],
            native: false,
            high: [0.0; 2],
            previous_low: [0.0; 2],
            phase: 0.0,
            envelope: [0.0; 2],
            resample: 0,
            gain,
            rate,
            dropped: 0,
            preset_id: u32::MAX,
            low_alpha: 0.0,
            bass_gain: 0.0,
            transient_gain: 0.0,
            gate: 0.0,
            limit: usize::MAX,
            previous_out: [0.0; 2],
            custom: [0; 4],
            three_band: false,
            bands: Default::default(),
            band_gains: [1.0, 1.0, 0.6],
            onset: OnsetDetector::new(rate),
            onset_gain: 0.0,
            pan: 0.0,
        };
        dsp.set_preset(3);
        dsp
    }
    /// Pull gain, preset, latency limit and optional modes from the shared settings.
    /// Called once per capture callback; only atomic loads.
    fn apply_settings(&mut self, shared: &Shared) {
        self.gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));
        let tuning = &shared.tuning;
        let id = shared.preset.load(Ordering::Relaxed);
        if id == PRESET_CUSTOM {
            let custom: [u32; 4] =
                std::array::from_fn(|i| tuning.custom[i].load(Ordering::Relaxed));
            if self.preset_id != id || self.custom != custom {
                self.custom = custom;
                self.preset_id = id;
                let [cutoff, bass, transient, gate] = custom.map(f32::from_bits);
                self.set_params(cutoff, bass, transient, gate);
            }
        } else {
            self.set_preset(id);
        }
        self.limit =
            (tuning.latency_frames.load(Ordering::Relaxed) as usize).clamp(240, BRIDGE_RING_MAX);
        self.three_band = tuning.mode.load(Ordering::Relaxed) == 1;
        self.band_gains =
            std::array::from_fn(|i| f32::from_bits(tuning.bands[i].load(Ordering::Relaxed)));
        self.onset_gain = f32::from_bits(tuning.onset.load(Ordering::Relaxed));
        self.pan = f32::from_bits(tuning.pan.load(Ordering::Relaxed));
    }
    fn set_params(&mut self, cutoff: f32, bass: f32, transient: f32, gate: f32) {
        self.low_alpha = (std::f32::consts::TAU * cutoff / self.rate as f32).clamp(0.001, 0.5);
        self.bass_gain = bass;
        self.transient_gain = transient;
        self.gate = gate;
    }
    fn set_preset(&mut self, id: u32) {
        if self.preset_id == id {
            return;
        }
        self.preset_id = id;
        let (cutoff, bass, transient, gate): (f32, f32, f32, f32) = match id {
            0 => (160.0, 1.7, 0.25, 0.035), // explosions: heavy body, quiet passages suppressed
            1 => (260.0, 0.45, 1.2, 0.012), // footsteps: favor short impacts
            2 => (220.0, 1.1, 0.18, 0.0),   // music: even bass response
            4 => (140.0, 0.4, 0.2, 0.02),   // quiet hours: low gain and a gentle gate
            _ => (180.0, 0.7, 0.25, 0.0),   // everything gentle
        };
        self.set_params(cutoff, bass, transient, gate);
    }
    fn frame(&mut self, input: [f32; 2], ring: &ArrayQueue<CaptureFrame>) -> bool {
        let mut out = [0.0; 2];
        let carrier = self.phase.cos();
        self.phase =
            (self.phase + std::f32::consts::TAU * 85.0 / self.rate as f32) % std::f32::consts::TAU;
        let hp_alpha = self.rate as f32 / (self.rate as f32 + std::f32::consts::TAU * 35.0);
        for channel in 0..2 {
            let x = if input[channel].is_finite() {
                input[channel].clamp(-1.0, 1.0)
            } else {
                0.0
            };
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
            // Band filters must keep running while the gate is closed.
            let band = if self.three_band && !self.native {
                self.bands[channel].frame(x, self.rate as f32, self.band_gains)
                    * self.gain
                    * level_gain
            } else {
                0.0
            };
            out[channel] = if self.native {
                x * self.gain
            } else if self.envelope[channel] >= self.gate {
                if self.three_band {
                    band
                } else {
                    (self.high[channel] * self.bass_gain
                        + transient * self.transient_gain * carrier)
                        * self.gain
                        * level_gain
                }
            } else {
                0.0
            };
        }
        if !self.native {
            out = dsp::pan_emphasis(out, self.pan);
            if self.onset_gain > 0.0 {
                let pulse = self.onset.push((input[0] + input[1]) * 0.5) * self.onset_gain;
                out = out.map(|v| (v + pulse).clamp(-1.0, 1.0));
            }
        }
        self.resample += u64::from(RATE);
        while self.resample >= u64::from(self.rate) {
            // Linear interpolation between the previous and current frame; at
            // the native rate the frame passes through untouched.
            let samples = if self.rate == RATE {
                out
            } else {
                let at = dsp::interpolation_fraction(self.resample, self.rate, RATE);
                std::array::from_fn(|i| self.previous_out[i] + (out[i] - self.previous_out[i]) * at)
            };
            self.resample -= u64::from(self.rate);
            if self.limit < usize::MAX {
                while ring.len() >= self.limit && ring.pop().is_some() {
                    self.dropped += 1;
                }
            }
            if ring
                .force_push(CaptureFrame {
                    generation: self.generation,
                    samples,
                })
                .is_some()
            {
                self.dropped += 1;
            }
        }
        self.previous_out = out;
        out.iter().any(|sample| sample.abs() > 0.002)
    }
}

#[cfg(windows)]
fn push_capture_frame(shared: &Shared, dsp: &mut CaptureDsp, frame: [f32; 2]) {
    let before = dsp.dropped;
    let active = dsp.frame(frame, &shared.bridge);
    shared
        .bridge_frames
        .fetch_add(u64::from(active), Ordering::Relaxed);
    shared
        .bridge_dropped
        .fetch_add(dsp.dropped - before, Ordering::Relaxed);
}

fn capture_is_output(
    name: &str,
    id: Option<&str>,
    sink_name: Option<&str>,
    sink_id: Option<&str>,
) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("dualsense")
        || name.contains("wireless controller")
        || sink_name.is_some_and(|s| s.to_ascii_lowercase() == name)
        || id.zip(sink_id).is_some_and(|(id, sink)| id.ends_with(sink))
}
pub fn capture_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2::available!(macos = 14.6)
    }
    #[cfg(not(target_os = "macos"))]
    {
        cfg!(any(windows, target_os = "linux"))
    }
}
pub(crate) fn capture_host() -> Result<cpal::Host, String> {
    #[cfg(target_os = "linux")]
    {
        cpal::host_from_id(cpal::HostId::PulseAudio).map_err(|e|format!("Start PipeWire's PulseAudio service or PulseAudio before capturing system audio: {e}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        if !capture_supported() {
            return Err("System audio capture requires macOS 14.6 or newer".into());
        }
        Ok(cpal::default_host())
    }
}
/// A running capture stream plus what it is attached to.
struct Opened {
    stream: Stream,
    id: Option<String>,
    name: String,
    rate: u32,
}
/// Id of the system default output, used to notice default-device changes.
fn default_output_id(host: &cpal::Host) -> Option<String> {
    host.default_output_device()?
        .id()
        .ok()
        .map(|id| id.to_string())
}
fn build_loopback(
    host: &cpal::Host,
    shared: Arc<Shared>,
    source: Option<&str>,
    sink_name: Option<&str>,
    sink_id: Option<&str>,
) -> Result<Opened, String> {
    let output = if let Some(id) = source {
        host.output_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.id().is_ok_and(|key| key.to_string() == id))
            .ok_or("Selected source endpoint is no longer available")?
    } else {
        host.default_output_device()
            .ok_or("No default output is available for system audio capture")?
    };
    let id = output.id().ok().map(|id| id.to_string());
    let name = output.to_string();
    // The paired endpoint's name may not be known yet (the output stream opens
    // asynchronously, and the user may have renamed it): look it up by id here.
    let sink_name = sink_name.map(str::to_owned).or_else(|| {
        let wanted = sink_id?;
        host.output_devices()
            .ok()?
            .find(|d| d.id().is_ok_and(|key| key.to_string().ends_with(wanted)))
            .map(|d| d.to_string())
    });
    if capture_is_output(&name, id.as_deref(), sink_name.as_deref(), sink_id) {
        return Err("Choose PC speakers or headphones as the system output; capturing the controller would create feedback.".into());
    }
    let format = output
        .default_output_config()
        .map_err(|e| format!("Could not inspect system audio: {e}"))?;
    let native = source.is_some();
    capture_math::validate(format.channels(), format.sample_rate(), native)?;
    #[cfg(target_os = "macos")]
    if native {
        return Err(
            "CoreAudio's stereo loopback does not expose a four-channel native relay".into(),
        );
    }
    #[cfg(target_os = "linux")]
    let device = {
        let name = output.id().map_err(|e| e.to_string())?;
        let monitor = format!("{}.monitor", name.id());
        host.input_devices().map_err(|e|e.to_string())?.find(|d|d.id().is_ok_and(|id|id.id()==monitor)).ok_or("The selected output has no monitor source; microphone capture is not used as a fallback")?
    };
    #[cfg(not(target_os = "linux"))]
    let device = output;
    let config = StreamConfig {
        channels: if cfg!(target_os = "macos") {
            2
        } else {
            format.channels()
        },
        sample_rate: format.sample_rate(),
        buffer_size: BufferSize::Default,
    };
    let stream = match format.sample_format() {
        SampleFormat::F32 => build_capture::<f32>(&device, config, shared, native),
        SampleFormat::F64 => build_capture::<f64>(&device, config, shared, native),
        SampleFormat::I16 => build_capture::<i16>(&device, config, shared, native),
        SampleFormat::I32 => build_capture::<i32>(&device, config, shared, native),
        SampleFormat::U16 => build_capture::<u16>(&device, config, shared, native),
        SampleFormat::U32 => build_capture::<u32>(&device, config, shared, native),
        SampleFormat::I8 => build_capture::<i8>(&device, config, shared, native),
        SampleFormat::U8 => build_capture::<u8>(&device, config, shared, native),
        other => {
            return Err(format!(
                "Capture sample format {other:?} is not supported; choose PCM16 or float output"
            ));
        }
    }
    .map_err(|e| format!("Could not start system audio capture: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("Could not play system audio capture: {e}"))?;
    Ok(Opened {
        stream,
        id,
        name,
        rate: format.sample_rate(),
    })
}
fn build_capture<T>(
    device: &cpal::Device,
    config: StreamConfig,
    shared: Arc<Shared>,
    native: bool,
) -> Result<Stream, cpal::Error>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    let generation = shared.capture_generation.load(Ordering::Acquire);
    let errors = shared.clone();
    let mut dsp = CaptureDsp::new(config.sample_rate, 0.5);
    dsp.generation = generation;
    dsp.native = native;
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            if shared.capture_generation.load(Ordering::Acquire) != generation {
                return;
            }
            dsp.apply_settings(&shared);
            let before = dsp.dropped;
            let mut active = 0;
            let (mut peak, mut square_sum, mut counted) = (0.0f32, 0.0f32, 0u32);
            for frame in data.chunks_exact(channels) {
                if shared.capture_generation.load(Ordering::Acquire) != generation {
                    break;
                }
                let mut samples = [0f32; 32];
                for (i, s) in frame.iter().enumerate() {
                    samples[i] = s.to_sample::<f32>();
                }
                let stereo = capture_math::stereo(&samples[..channels], native);
                for value in stereo {
                    peak = peak.max(value.abs());
                    square_sum += value * value;
                }
                counted += 1;
                active += u64::from(dsp.frame(stereo, &shared.bridge));
            }
            if counted > 0 {
                shared.record_capture_input(peak, (square_sum / (counted as f32 * 2.0)).sqrt());
            }
            shared.bridge_frames.fetch_add(active, Ordering::Relaxed);
            shared
                .bridge_dropped
                .fetch_add(dsp.dropped - before, Ordering::Relaxed);
        },
        move |e| record_capture_error(&errors, generation, e),
        Some(Duration::from_secs(3)),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn cached_start_failure_allows_motor_fallback_without_reopening_audio() {
        let mut engine = AudioEngine::default();
        engine.remember_output_failure("endpoint unavailable".into());
        assert_eq!(engine.start_output().unwrap_err(), "endpoint unavailable");
        assert!(!engine.output_opening());
        engine.allow_output_retry();
        assert!(!engine.output_failed());
    }
    #[test]
    fn retired_audio_callbacks_cannot_touch_replacement_cues_or_counters() {
        let mut engine = AudioEngine::default();
        let old = engine.shared.clone();
        let personal = engine.shared.personal.load_full();
        engine.fresh_output_state();
        assert!(std::sync::Arc::ptr_eq(
            &personal,
            &engine.shared.personal.load_full()
        ));
        *old.error.lock().unwrap() = Some("stale endpoint failure".into());
        let mut samples = [0f32; 192];
        let mut position = 0;
        let mut render = RenderState::default();
        render_output(&mut samples, &old, &mut position, &mut render);
        assert_eq!(engine.callbacks(), 0);
        assert!(engine.take_error().is_none());
        assert_eq!(old.callbacks.load(Ordering::Relaxed), 1);
    }

    use super::*;
    #[test]
    fn renamed_paired_output_cannot_be_its_own_loopback_source() {
        assert!(capture_is_output(
            "Desk pad",
            Some("wasapi:target-id"),
            Some("Desk pad"),
            Some("target-id")
        ));
        assert!(capture_is_output(
            "Different friendly alias",
            Some("wasapi:target-id"),
            None,
            Some("target-id")
        ));
        assert!(!capture_is_output(
            "Virtual relay",
            Some("wasapi:relay-id"),
            Some("Desk pad"),
            Some("target-id")
        ));
    }
    #[test]
    fn capture_failure_is_scoped_and_stale_errors_are_ignored() {
        let mut engine = AudioEngine::default();
        engine
            .shared
            .speaker_pcm
            .store(Some(Arc::new(PcmStream::new(7, "grips", false, 0))));
        engine.shared.bridge_on.store(true, Ordering::Relaxed);
        engine.capture_source = Some(CaptureSource::Process { pid: 12345 });
        engine.shared.capture_generation.store(2, Ordering::Relaxed);
        record_capture_error(
            &engine.shared,
            1,
            cpal::ErrorKind::DeviceNotAvailable.into(),
        );
        assert!(engine.take_capture_error().is_none());
        assert!(engine.bridge_on());
        *engine.shared.capture_error.lock().unwrap() = Some("Process ended".into());
        assert_eq!(
            engine.take_capture_error().as_deref(),
            Some("Process ended")
        );
        assert!(!engine.bridge_on());
        assert!(engine.capture_source.is_none());
        assert!(engine.shared.speaker_pcm.load().is_some());
    }
    #[test]
    fn native_relay_bypasses_music_conversion_and_keeps_stereo_bounded() {
        let mut dsp = CaptureDsp::new(48_000, 0.5);
        dsp.native = true;
        let ring = ArrayQueue::new(4);
        dsp.frame([0.25, -0.5], &ring);
        assert_eq!(ring.pop().map(|f| f.samples), Some([0.125, -0.25]));
        dsp.frame([f32::NAN, f32::INFINITY], &ring);
        assert_eq!(ring.pop().map(|f| f.samples), Some([0.0; 2]));
        let mut clip = VoiceClip::new("sine", 80, 2, 650, 25, 0);
        clip.shape = Shape::LinkTest;
        clip.attack = 0.0;
        clip.release = 2.0;
        assert_eq!(clip.sample(0.2), Some([0.0; 2]));
        assert!(clip.sample(2.0).is_none());
        assert!(clip.sample(0.05).unwrap().iter().all(|v| v.abs() <= 0.25));
    }
    #[test]
    fn timeline_stereo_mix_obeys_personal_caps_and_raw_test_is_bounded() {
        let mut engine = AudioEngine::default();
        let clip: EffectClip =
            serde_json::from_value(serde_json::json!({"id":"stereo","name":"Stereo","cues":[
                {"lane":"leftGrip","kind":"pulse","startMs":0,"durationMs":100,"strength":100.0},
                {"lane":"rightGrip","kind":"pulse","startMs":0,"durationMs":100,"strength":100.0}
            ]}))
            .unwrap();
        engine
            .shared
            .timeline
            .store(Some(Arc::new(TimelineAudio::new(clip, 0))));
        engine
            .set_personal(PersonalSettings {
                enabled: true,
                maximum: [25, 50],
                ..Default::default()
            })
            .unwrap();
        let mut output = [0.0; 4096];
        render_output(
            &mut output,
            &engine.shared,
            &mut 0,
            &mut RenderState::default(),
        );
        assert!(output.as_chunks::<4>().0.iter().all(|f| f[0] == 0.0
            && f[1] == 0.0
            && f[2].abs() <= engine.shared.ceiling() * 0.25
            && f[3].abs() <= engine.shared.ceiling() * 0.5));
        assert_eq!(engine.diagnostics()["timelineActive"], true);
        engine.cancel_cue();
        assert!(engine.shared.timeline.load().is_none());
        assert_eq!(engine.diagnostics()["timelineActive"], false);
        let mut voice = VoiceClip::new("sine", 90, 10, 180, 100, -100);
        voice.raw_test = true;
        engine.shared.clip.store(Some(Arc::new(voice)));
        render_output(
            &mut output,
            &engine.shared,
            &mut 0,
            &mut RenderState::default(),
        );
        assert!(
            output
                .as_chunks::<4>()
                .0
                .iter()
                .any(|f| f[2].abs() > engine.shared.ceiling() * 0.25)
        );
        assert!(
            output
                .as_chunks::<4>()
                .0
                .iter()
                .all(|f| f[2].abs() <= engine.shared.ceiling() && f[3] == 0.0)
        );
        engine.stop();
        assert!(engine.shared.timeline.load().is_none());
    }
    #[test]
    fn conversion_modes_distinguish_bass_texture_and_transient_energy() {
        let energy = |frequency: f32, mode: &str| {
            let mut filter = GripFilter::default();
            (0..48000)
                .map(|i| {
                    let x = (i as f32 / 48000.0 * frequency * std::f32::consts::TAU).sin() * 0.5;
                    filter.frame_mode(x, mode).powi(2)
                })
                .collect::<Vec<_>>()
        };
        let bass = energy(60.0, "bass");
        let treble = energy(2000.0, "bass");
        let texture = energy(2000.0, "texture");
        assert!(bass.iter().sum::<f32>() > treble.iter().sum::<f32>() * 10.0);
        assert!(texture.iter().sum::<f32>() > treble.iter().sum::<f32>() * 10.0);
        let impact = energy(90.0, "impact");
        assert!(impact[..4800].iter().sum::<f32>() > impact[40000..].iter().sum::<f32>() * 10.0);
        assert!(impact.iter().all(|x| x.is_finite() && *x <= 1.0));
    }
    #[test]
    fn game_rumble_keeps_motor_channels_and_expires_without_new_commands() {
        let rumble = GameRumble {
            motors: [255, 0],
            start_frame: 12,
        };
        assert!(rumble.sample(100)[0].abs() > 0.1);
        assert_eq!(rumble.sample(100)[1], 0.0);
        assert_eq!(rumble.sample(12 + 10 * u64::from(RATE)), [0.0; 2]);
        let rumble = GameRumble {
            motors: [0, 128],
            start_frame: 0,
        };
        assert_eq!(rumble.sample(100)[0], 0.0);
        assert!(rumble.sample(100)[1].abs() <= 128.0 / 255.0);
    }
    #[test]
    fn game_rumble_is_mixed_limited_muted_and_stopped() {
        let mut engine = AudioEngine::default();
        engine.shared.game.store(Some(Arc::new(GameRumble {
            motors: [255, 255],
            start_frame: 0,
        })));
        let mut output = [0.0; 4096];
        render_output(
            &mut output,
            &engine.shared,
            &mut 0,
            &mut RenderState::default(),
        );
        assert!(
            output
                .as_chunks::<4>()
                .0
                .iter()
                .all(|f| f[0] == 0.0 && f[1] == 0.0)
        );
        assert!(output.iter().any(|x| x.abs() > 0.05));
        assert!(output.iter().all(|x| x.abs() <= engine.shared.ceiling()));
        engine
            .set_mixer(MixerSettings {
                game_gain: 0.0,
                ..Default::default()
            })
            .unwrap();
        render_output(
            &mut output,
            &engine.shared,
            &mut 0,
            &mut RenderState::default(),
        );
        assert!(output.iter().all(|x| *x == 0.0));
        engine.set_game_rumble([0, 0]).unwrap();
        assert!(engine.shared.game.load().is_none());
        engine.shared.game.store(Some(Arc::new(GameRumble {
            motors: [255, 255],
            start_frame: 0,
        })));
        engine.stop();
        assert!(engine.shared.game.load().is_none());
    }
    #[test]
    fn idle_endpoint_detection_keeps_paused_music_and_live_sources() {
        let engine = AudioEngine::default();
        assert!(engine.sources_idle());
        engine
            .shared
            .clip
            .store(Some(Arc::new(VoiceClip::new("sine", 90, 2, 100, 50, 0))));
        assert!(!engine.sources_idle());
        engine
            .shared
            .rendered_frames
            .store(u64::from(RATE), Ordering::Relaxed);
        assert!(engine.sources_idle());
        let stream = Arc::new(PcmStream::new(9, "grips", true, 1000));
        stream.paused.store(true, Ordering::Release);
        engine.shared.speaker_pcm.store(Some(stream.clone()));
        assert!(!engine.sources_idle());
        stream.canceled.store(true, Ordering::Release);
        assert!(engine.sources_idle());
        engine.shared.bridge_on.store(true, Ordering::Relaxed);
        assert!(!engine.sources_idle());
    }
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
        shared.clip.store(Some(Arc::new(VoiceClip::new(
            "square", 100, 2, 650, 100, 0,
        ))));
        let mut pcm = [0.0f32; 4096];
        render_output(&mut pcm, &shared, &mut 0, &mut RenderState::default());
        assert!(pcm.as_chunks::<4>().0.iter().all(|f| f[0] == 0.0
            && f[1] == 0.0
            && f[2].abs() <= shared.ceiling()
            && f[3].abs() <= shared.ceiling()));
        assert!(pcm.as_chunks::<4>().0.iter().any(|f| f[2] != 0.0));
    }
    #[test]
    fn full_strength_preview_reaches_useful_level_without_exceeding_cap() {
        let shared = Shared::default();
        let clip = VoiceClip::new("square", 90, 2, 100, 100, 0);
        shared.clip.store(Some(Arc::new(clip)));
        let mut frames = [0.0f32; 4096];
        render_output(&mut frames, &shared, &mut 0, &mut RenderState::default());
        let peak = frames
            .as_chunks::<4>()
            .0
            .iter()
            .map(|frame| frame[2].abs())
            .fold(0.0f32, f32::max);
        assert!(peak > 0.1, "peak {peak} should be audible at full strength");
        assert!(peak <= shared.ceiling());
    }
    #[test]
    fn custom_speaker_pcm_stays_on_front_channels() {
        let shared = Shared::default();
        let stream = Arc::new(PcmStream::new(1, "speaker", false, 2048));
        for _ in 0..2048 {
            stream
                .queue
                .push([SPEAKER_FILE_GAIN, SPEAKER_FILE_GAIN, 0.0, 0.0])
                .unwrap();
        }
        shared.speaker_pcm.store(Some(stream));
        let mut frames = [0.0; 4096];
        render_output(&mut frames, &shared, &mut 0, &mut RenderState::default());
        assert!(
            frames
                .as_chunks::<4>()
                .0
                .iter()
                .all(|f| f[0] == SPEAKER_FILE_GAIN && f[1] == f[0] && f[2] == 0.0 && f[3] == 0.0)
        );
    }
    #[test]
    fn pause_is_silent_and_resume_continues_at_the_same_sample() {
        let mut engine = AudioEngine::default();
        let stream = Arc::new(PcmStream::new(1, "speaker", false, 6000));
        stream.consumed.store(100, Ordering::Relaxed);
        for n in 100..6000 {
            let x = n as f32 / i16::MAX as f32 * SPEAKER_FILE_GAIN;
            stream.queue.push([x, x, 0.0, 0.0]).unwrap();
        }
        engine.shared.speaker_pcm.store(Some(stream));
        let paused = engine.pause_speaker_pcm(true).unwrap();
        let mut silent = [1.0; 64];
        render_output(
            &mut silent,
            &engine.shared,
            &mut 100,
            &mut RenderState::default(),
        );
        assert!(silent.iter().all(|x| *x == 0.0));
        let resumed = engine.pause_speaker_pcm(false).unwrap();
        assert_eq!(paused.position, resumed.position);
        let mut output = [0.0; 64];
        render_output(
            &mut output,
            &engine.shared,
            &mut 2100,
            &mut RenderState::default(),
        );
        assert!((output[0] - 100.0 / i16::MAX as f32 * SPEAKER_FILE_GAIN).abs() < 0.000001);
    }
    #[test]
    fn stop_clears_paused_pcm_and_test_tone() {
        let mut engine = AudioEngine::default();
        let stream = Arc::new(PcmStream::new(1, "both", false, 6000));
        stream.paused.store(true, Ordering::Relaxed);
        stream.queue.push([0.2; 4]).unwrap();
        engine.shared.speaker_pcm.store(Some(stream.clone()));
        engine
            .shared
            .speaker
            .store(Some(Arc::new(SpeakerChirp { start_frame: 0 })));
        engine.stop_speaker_pcm();
        assert!(engine.pause_speaker_pcm(false).is_err());
        assert!(stream.canceled.load(Ordering::Acquire));
        let mut output = [1.0; 64];
        render_output(
            &mut output,
            &engine.shared,
            &mut 100,
            &mut RenderState::default(),
        );
        assert!(output.iter().all(|x| *x == 0.0));
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
        shared
            .speaker
            .store(Some(Arc::new(SpeakerChirp { start_frame: 0 })));
        let mut pcm = [0.0f32; 4096];
        render_output(&mut pcm, &shared, &mut 0, &mut RenderState::default());
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
        single
            .speaker
            .store(Some(Arc::new(SpeakerChirp { start_frame: 0 })));
        split
            .speaker
            .store(Some(Arc::new(SpeakerChirp { start_frame: 0 })));
        let mut one = [0.0f32; 4096];
        let mut first = [0.0f32; 2048];
        let mut second = [0.0f32; 2048];
        render_output(&mut one, &single, &mut 0, &mut RenderState::default());
        let mut position = 0;
        render_output(
            &mut first,
            &split,
            &mut position,
            &mut RenderState::default(),
        );
        render_output(
            &mut second,
            &split,
            &mut position,
            &mut RenderState::default(),
        );
        assert_eq!(&one[..2048], &first);
        assert_eq!(&one[2048..], &second);
    }
    #[test]
    fn capture_queue_stays_bounded() {
        let ring = ArrayQueue::new(QUEUE_FRAMES);
        let mut dsp = CaptureDsp::new(48_000, 1.0);
        for _ in 0..QUEUE_FRAMES * 2 {
            dsp.frame([1.0, 1.0], &ring);
        }
        assert_eq!(ring.len(), QUEUE_FRAMES);
        assert_eq!(dsp.dropped, QUEUE_FRAMES as u64);
    }
    #[test]
    fn capture_source_switch_rejects_inflight_and_backlogged_old_frames() {
        let shared = Shared::default();
        shared.capture_generation.store(4, Ordering::Release);
        for _ in 0..128 {
            shared
                .bridge
                .push(CaptureFrame {
                    generation: 3,
                    samples: [1.0; 2],
                })
                .unwrap();
        }
        shared
            .bridge
            .push(CaptureFrame {
                generation: 4,
                samples: [0.2, -0.1],
            })
            .unwrap();
        assert_eq!(capture_sample(&shared), None);
        assert_eq!(capture_sample(&shared), None);
        assert_eq!(capture_sample(&shared), Some([0.2, -0.1]));
        shared
            .bridge
            .push(CaptureFrame {
                generation: 3,
                samples: [1.0; 2],
            })
            .unwrap();
        assert_eq!(capture_sample(&shared), None);
    }
    #[test]
    fn capture_resampling_uses_exact_frame_counts_and_rejects_nonfinite_gain() {
        for rate in [8_000, 44_100, 48_000, 96_000, 192_000] {
            let ring = ArrayQueue::new(8);
            let mut dsp = CaptureDsp::new(rate, f32::NAN);
            for _ in 0..44_101 {
                dsp.frame([f32::INFINITY, -1.0], &ring);
            }
            assert_eq!(
                ring.len() as u64 + dsp.dropped,
                44_101u64 * u64::from(RATE) / u64::from(rate)
            );
            while let Some(frame) = ring.pop() {
                assert_eq!(frame.samples, [0.0; 2]);
            }
        }
    }
    #[test]
    fn footsteps_preset_emphasizes_short_impacts() {
        let mut footsteps = CaptureDsp::new(RATE, 1.0);
        footsteps.set_preset(1);
        let mut explosions = CaptureDsp::new(RATE, 1.0);
        explosions.set_preset(0);
        let quick = ArrayQueue::new(QUEUE_FRAMES);
        let heavy = ArrayQueue::new(QUEUE_FRAMES);
        footsteps.frame([1.0, 1.0], &quick);
        explosions.frame([1.0, 1.0], &heavy);
        assert!(quick.pop().unwrap().samples[0] > heavy.pop().unwrap().samples[0]);
    }
    #[test]
    fn file_underrun_keeps_position_and_recovers_with_separate_grips() {
        let shared = Shared::default();
        let stream = Arc::new(PcmStream::new(99, "grips", true, 10000));
        stream.consumed.store(100, Ordering::Relaxed);
        stream.ready.store(true, Ordering::Release);
        shared.speaker_pcm.store(Some(stream.clone()));
        let mut output = [1.0f32; 64];
        let mut position = 0;
        let mut state = RenderState::default();
        render_output(&mut output, &shared, &mut position, &mut state);
        assert!(output.iter().all(|x| *x == 0.0));
        assert_eq!(stream.consumed.load(Ordering::Relaxed), 100);
        assert_eq!(shared.file_underruns.load(Ordering::Relaxed), 1);
        for _ in 0..4800 {
            stream.queue.push([0.0, 0.0, 0.4, 0.0]).unwrap();
        }
        stream.ready.store(true, Ordering::Release);
        render_output(&mut output, &shared, &mut position, &mut state);
        assert_eq!(stream.consumed.load(Ordering::Relaxed), 116);
        assert!(
            output
                .as_chunks::<4>()
                .0
                .iter()
                .all(|f| f[2] > 0.0 && f[3] == 0.0)
        );
    }
    #[test]
    fn mixer_ducks_background_then_releases_and_solo_is_exact() {
        let mut state = RenderState::default();
        let mut settings = MixerSettings {
            ducking: 1.0,
            ..Default::default()
        };
        let mut mixed = [0.0; 2];
        for _ in 0..9600 {
            mixed = state.mix_grips([0.4; 2], [0.5; 2], &settings, true);
        }
        assert!((mixed[0] - 0.4).abs() < 0.001);
        for _ in 0..24000 {
            mixed = state.mix_grips([0.0; 2], [0.5; 2], &settings, false);
        }
        assert!(mixed[0] > 0.49);
        settings.solo = "left".into();
        assert_eq!(state.mix_grips([1.0; 2], [0.5; 2], &settings, true)[1], 0.0);
        settings.solo = "right".into();
        assert_eq!(state.mix_grips([1.0; 2], [0.5; 2], &settings, true)[0], 0.0);
    }
    #[test]
    fn end_of_file_drains_device_buffers_before_reporting_finished() {
        let shared = Shared::default();
        let stream = Arc::new(PcmStream::new(100, "speaker", true, 16));
        for _ in 0..16 {
            stream.queue.push([0.1, 0.1, 0.0, 0.0]).unwrap();
        }
        stream.eof.store(true, Ordering::Release);
        stream.ready.store(true, Ordering::Release);
        shared.speaker_pcm.store(Some(stream.clone()));
        let mut state = RenderState::default();
        let mut position = 0;
        let mut frames = [0.0; 64];
        render_output(&mut frames, &shared, &mut position, &mut state);
        assert_eq!(stream.mode(), "playing");
        let mut tail = vec![0.0; RATE as usize / 10 * 4];
        render_output(&mut tail, &shared, &mut position, &mut state);
        assert_eq!(stream.mode(), "finished");
        assert_eq!(stream.consumed.load(Ordering::Relaxed), 16);
    }
    #[test]
    fn muted_sources_are_silent_and_meter_peaks_reset() {
        let mut engine = AudioEngine::default();
        engine.shared.clip.store(Some(Arc::new(VoiceClip::new(
            "square", 100, 2, 100, 100, 0,
        ))));
        engine
            .shared
            .speaker
            .store(Some(Arc::new(SpeakerChirp { start_frame: 0 })));
        engine
            .set_mixer(MixerSettings {
                cue_gain: 0.0,
                speaker_gain: 0.0,
                ..Default::default()
            })
            .unwrap();
        let mut output = [1.0; 4096];
        render_output(
            &mut output,
            &engine.shared,
            &mut 0,
            &mut RenderState::default(),
        );
        assert!(output.iter().all(|x| *x == 0.0));
        engine.shared.peaks[1].store(0.5f32.to_bits(), Ordering::Relaxed);
        assert_eq!(engine.diagnostics()["peaks"][1], 0.5);
        assert_eq!(engine.diagnostics()["peaks"][1], 0.0);
        engine.stop_speaker_pcm();
        assert!(
            engine
                .set_mixer(MixerSettings {
                    file_gain: f32::NAN,
                    ..Default::default()
                })
                .is_err()
        );
    }
    #[test]
    fn dropping_engine_cancels_its_file_worker() {
        let engine = AudioEngine::default();
        let stream = Arc::new(PcmStream::new(101, "grips", true, 10000));
        engine.shared.speaker_pcm.store(Some(stream.clone()));
        drop(engine);
        assert!(stream.canceled.load(Ordering::Acquire));
    }
    #[test]
    fn expired_game_rumble_releases_the_endpoint_like_diagnostics_reports() {
        let engine = AudioEngine::default();
        engine.shared.game.store(Some(Arc::new(GameRumble {
            motors: [255, 255],
            start_frame: 0,
        })));
        assert!(!engine.sources_idle());
        assert_eq!(engine.active_sources(), vec!["gameRumble"]);
        engine
            .shared
            .rendered_frames
            .store(GAME_RUMBLE_SECONDS * u64::from(RATE), Ordering::Relaxed);
        assert!(engine.sources_idle());
        assert_eq!(engine.diagnostics()["gameActive"], false);
    }
    #[test]
    fn bridge_prefills_fades_on_underrun_and_counts_each_missing_frame() {
        let shared = Shared::default();
        let mut state = RenderState::default();
        let push = |n: usize| {
            for _ in 0..n {
                shared
                    .bridge
                    .push(CaptureFrame {
                        generation: 0,
                        samples: [0.5, 0.5],
                    })
                    .unwrap();
            }
        };
        push(100);
        assert_eq!(bridge_sample(&shared, &mut state, 480), ([0.0; 2], false));
        assert_eq!(shared.bridge.len(), 100, "waits for the prefill target");
        push(400);
        assert_eq!(bridge_sample(&shared, &mut state, 480), ([0.5, 0.5], false));
        while shared.bridge.pop().is_some() {}
        let (first, missed) = bridge_sample(&shared, &mut state, 480);
        assert!(missed && first[0] > 0.4 && first[0] < 0.5);
        let (later, missed) = bridge_sample(&shared, &mut state, 480);
        assert!(!missed, "a gap is counted once, not per held frame");
        assert!(later[0] < first[0], "holds fade toward zero");
        assert_eq!(state.bridge_held_frames, 2);
        for _ in 0..1200 {
            bridge_sample(&shared, &mut state, 480);
        }
        assert!(!state.bridge_primed, "a long gap rebuffers");
        assert_eq!(bridge_sample(&shared, &mut state, 480), ([0.0; 2], false));
    }
    #[test]
    fn bridge_silent_gap_counts_one_underrun_and_quiet_queue_still_primes() {
        let shared = Shared::default();
        let mut state = RenderState::default();
        let push = |n: usize| {
            for _ in 0..n {
                shared
                    .bridge
                    .push(CaptureFrame {
                        generation: 0,
                        samples: [0.25, 0.25],
                    })
                    .unwrap();
            }
        };
        // A short queue that never reaches the prefill still starts playing.
        push(10);
        let mut started = false;
        for _ in 0..600 {
            if bridge_sample(&shared, &mut state, 480).0 == [0.25, 0.25] {
                started = true;
                break;
            }
        }
        assert!(started, "a quiet source primes after ~10 ms without growth");
        let mut underruns = 0;
        for _ in 0..2000 {
            underruns += u64::from(bridge_sample(&shared, &mut state, 480).1);
        }
        assert_eq!(underruns, 1, "a silent source is one gap, not ~960");
        assert!(state.bridge_held_frames >= 900);
    }
    #[test]
    fn capture_dsp_drops_down_to_the_latency_limit() {
        let ring = ArrayQueue::new(2048);
        let mut dsp = CaptureDsp::new(RATE, 1.0);
        dsp.limit = 240;
        for _ in 0..1000 {
            ring.push(CaptureFrame {
                generation: 0,
                samples: [0.0; 2],
            })
            .unwrap();
        }
        dsp.frame([0.0; 2], &ring);
        assert!(ring.len() <= 240, "len {}", ring.len());
        assert!(dsp.dropped >= 760);
    }
    #[test]
    fn bridge_latency_custom_and_options_are_validated_and_readable() {
        let engine = AudioEngine::default();
        assert_eq!(engine.bridge_latency(), "normal");
        engine.set_bridge_latency("low").unwrap();
        assert_eq!(engine.bridge_latency(), "low");
        engine.set_bridge_latency("safe").unwrap();
        assert_eq!(engine.bridge_latency(), "safe");
        assert!(engine.set_bridge_latency("instant").is_err());
        let custom = BridgeCustom {
            cutoff: 120.0,
            bass: 1.2,
            transient: 0.5,
            gate: 0.01,
        };
        engine.set_bridge_custom(custom).unwrap();
        assert_eq!(engine.bridge_custom(), custom);
        assert!(
            engine
                .set_bridge_custom(BridgeCustom {
                    cutoff: f32::NAN,
                    ..custom
                })
                .is_err()
        );
        assert_eq!(engine.bridge_options(), BridgeOptions::default());
        let options = BridgeOptions {
            mode: "threeBand".into(),
            band_gains: [1.0, 0.5, 0.0],
            onset: 0.5,
            pan_emphasis: 2.0,
        };
        engine.set_bridge_options(options.clone()).unwrap();
        assert_eq!(engine.bridge_options(), options);
        assert!(
            engine
                .set_bridge_options(BridgeOptions {
                    mode: "weird".into(),
                    ..options
                })
                .is_err()
        );
        let mut dsp = CaptureDsp::new(RATE, 1.0);
        engine.shared.preset.store(PRESET_CUSTOM, Ordering::Relaxed);
        dsp.apply_settings(&engine.shared);
        assert!(dsp.three_band && dsp.onset_gain == 0.5 && dsp.pan == 2.0);
        assert_eq!(dsp.limit, 1920);
        assert_eq!(dsp.bass_gain, 1.2);
        let diagnostics = engine.peek_diagnostics();
        assert_eq!(diagnostics["bridgeLatency"], "safe");
        assert_eq!(diagnostics["keepOpenBy"], serde_json::json!([]));
    }
    #[test]
    fn latency_limit_drops_oldest_frames_and_resampling_interpolates() {
        let ring = ArrayQueue::new(BRIDGE_RING_MAX);
        let mut dsp = CaptureDsp::new(RATE, 1.0);
        dsp.native = true;
        dsp.limit = 480;
        for _ in 0..1000 {
            dsp.frame([0.5, 0.5], &ring);
        }
        assert_eq!(ring.len(), 480);
        assert_eq!(dsp.dropped, 520);
        let ring = ArrayQueue::new(64);
        let mut up = CaptureDsp::new(24_000, 1.0);
        up.native = true;
        up.frame([0.0, 0.0], &ring);
        ring.pop();
        ring.pop();
        up.frame([1.0, 1.0], &ring);
        let first = ring.pop().unwrap().samples[0];
        let second = ring.pop().unwrap().samples[0];
        assert!(
            first > 0.0 && first < second && second <= 1.0,
            "{first} {second}"
        );
    }
    #[test]
    fn replaced_values_are_retired_instead_of_dropped_by_the_control_thread() {
        let shared = Shared::default();
        let clip = Arc::new(VoiceClip::new("sine", 90, 2, 100, 50, 0));
        shared.put(&shared.clip, Some(clip.clone()));
        shared.put(&shared.clip, None);
        assert_eq!(Arc::strong_count(&clip), 2, "kept alive by the retire list");
        assert_eq!(shared.retired.lock().unwrap().len(), 1);
    }
    #[test]
    fn noise_shape_is_deterministic_bounded_band_limited_noise() {
        let clip = VoiceClip::new("noise", 90, 10, 400, 100, 0);
        let samples: Vec<f32> = (0..2400)
            .map(|i| clip.sample(0.01 + i as f32 / 48_000.0).unwrap()[0])
            .collect();
        let again: Vec<f32> = (0..2400)
            .map(|i| clip.sample(0.01 + i as f32 / 48_000.0).unwrap()[0])
            .collect();
        assert_eq!(samples, again);
        assert!(samples.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        let step = samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.1, "noise must be low-passed, step {step}");
        let cue: EffectClip = serde_json::from_value(serde_json::json!({"id":"n","name":"N","cues":[
            {"lane":"leftGrip","kind":"texture","startMs":0,"durationMs":200,"strength":100.0,"shape":"noise"}]}))
        .unwrap();
        let a = cue.sample(0.05, &PersonalSettings::default());
        assert_eq!(a, cue.compile().sample(0.05, &PersonalSettings::default()));
        assert!(cue.compile().active(0.05) && !cue.compile().active(0.5));
    }
    #[test]
    fn thermal_duty_is_shared_state_and_survives_a_stream_restart() {
        let mut engine = AudioEngine::default();
        engine.shared.duty_store([0.2, 0.1]);
        engine.duty_saved = Some((engine.shared.duty_load(), Instant::now()));
        engine.fresh_output_state();
        let carried = engine.shared.duty_load();
        assert!(carried[0] > 0.19 && carried[0] <= 0.2 && carried[1] > 0.09);
        let mut state = RenderState::with_duty(carried);
        let mut out = [0.0; 64];
        render_output(&mut out, &engine.shared, &mut 0, &mut state);
        assert_eq!(engine.shared.duty_load()[0], state.duty[0]);
    }

    fn peak_of(shared: &Shared, clip: VoiceClip, frames: usize) -> f32 {
        shared.clip.store(Some(Arc::new(clip)));
        let mut state = RenderState::default();
        let mut position = 0;
        let mut peak = 0.0f32;
        for _ in 0..frames / 512 {
            let mut buffer = [0.0f32; 512 * 4];
            render_output(&mut buffer, shared, &mut position, &mut state);
            peak = buffer
                .as_chunks::<4>()
                .0
                .iter()
                .fold(peak, |p, f| p.max(f[2].abs()).max(f[3].abs()));
        }
        peak
    }
    #[test]
    fn knee_curve_is_linear_below_the_knee_monotonic_odd_and_bounded() {
        for step in 0..=70 {
            let x = step as f32 / 100.0;
            assert_eq!(knee_curve(x), x);
            assert_eq!(knee_curve(-x), -x);
        }
        let mut last = -1.0f32;
        for step in -4000..=4000 {
            let y = knee_curve(step as f32 / 1000.0);
            assert!(y >= last && y.abs() <= 1.0, "{step}");
            last = y;
        }
        assert!(knee_curve(1000.0) <= 1.0 && knee_curve(1000.0) > 0.99);
        // No jump at the knee: the slope stays continuous.
        assert!((knee_curve(KNEE + 1e-3) - KNEE - 1e-3).abs() < 1e-5);
    }
    #[test]
    fn soft_limit_scales_with_strength_stays_under_every_ceiling_and_survives_nan() {
        for ceiling in LEVEL_CEILINGS {
            let level = |strength: f32| soft_limit(strength, &mut 0.0, ceiling);
            let (low, mid, high) = (level(0.3), level(0.6), level(1.0));
            assert!((low - 0.3 * ceiling).abs() < 1e-6);
            assert!((mid - 0.6 * ceiling).abs() < 1e-6);
            assert!(low < mid && mid < high && high < ceiling);
            assert!(high > mid * 1.3, "100 % must stay clearly above 60 %");
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                assert_eq!(soft_limit(value, &mut 0.0, ceiling), 0.0);
            }
            for value in [-1e30, -5.0, 5.0, 1e30] {
                assert!(soft_limit(value, &mut 0.0, ceiling).abs() <= ceiling);
            }
        }
        // Mid strengths are not quieter than the old tanh(1.5x) limiter at the Strong level.
        for strength in [0.3f32, 0.5, 0.6, 0.8, 1.0] {
            let old = (strength * 1.5).tanh() * MAX_AMPLITUDE;
            assert!(soft_limit(strength, &mut 0.0, LEVEL_CEILINGS[2]) >= old * 0.9);
        }
    }
    #[test]
    fn thermal_cap_scales_with_the_level_and_respects_the_hard_cap() {
        let mut settled = Vec::new();
        for (i, ceiling) in LEVEL_CEILINGS.into_iter().enumerate() {
            let mut duty = 0.0;
            let mut last = 0.0;
            let mut sum = 0.0;
            for n in 0..(RATE as usize * 8) {
                last = soft_limit(1.0, &mut duty, ceiling);
                if n >= RATE as usize * 6 {
                    sum += last.abs();
                }
            }
            let average = sum / (RATE as f32 * 2.0);
            assert_eq!(duty_cap(ceiling), LEVEL_DUTY_CAPS[i]);
            assert!(duty_cap(ceiling) <= HARD_DUTY_CAP);
            assert!(
                last <= LEVEL_DUTY_CAPS[i] * 1.06 && average <= LEVEL_DUTY_CAPS[i] * 1.06,
                "level {i}: sustained drive settles near {}, got {last}",
                LEVEL_DUTY_CAPS[i]
            );
            settled.push(average);
        }
        assert_eq!(LEVEL_DUTY_CAPS[1], 0.16, "Normal keeps the historical cap");
        for pair in settled.windows(2) {
            assert!(pair[1] / pair[0] >= 1.35, "{settled:?}");
        }
        assert_eq!(duty_cap(10.0), HARD_DUTY_CAP);
    }
    #[test]
    fn new_shapes_are_finite_bounded_deterministic_and_distinct() {
        let names = ["triangle", "pulse", "double", "swell"];
        let render = |name: &str, frequency: u16| -> Vec<f32> {
            let clip = VoiceClip::new(name, frequency, 20, 600, 100, 0);
            (0..RATE as usize / 2)
                .map(|i| clip.sample(i as f32 / RATE as f32).map_or(0.0, |s| s[0]))
                .collect()
        };
        let mut signatures = Vec::new();
        for name in names {
            assert!(VoiceClip::is_known_shape(name));
            for frequency in [40u16, 90, 150, 300] {
                let a = render(name, frequency);
                assert_eq!(a, render(name, frequency), "{name} must be deterministic");
                assert!(a.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
                assert!(
                    a.iter().any(|v| v.abs() > 0.2),
                    "{name} {frequency} is silent"
                );
            }
            let a = render(name, 120);
            let mean_abs = a.iter().map(|v| v.abs()).sum::<f32>() / a.len() as f32;
            let peak = a.iter().fold(0.0f32, |p, v| p.max(v.abs()));
            signatures.push((mean_abs, peak / mean_abs.max(1e-6)));
        }
        // Mean level or crest factor differs between every pair of new shapes.
        for i in 0..signatures.len() {
            for j in i + 1..signatures.len() {
                let (a, b) = (signatures[i], signatures[j]);
                assert!(
                    (a.0 - b.0).abs() > 0.01 || (a.1 - b.1).abs() > 0.3,
                    "{} and {} look the same",
                    names[i],
                    names[j]
                );
            }
        }
        assert!(!VoiceClip::is_known_shape("wobble"));
        // The pulse train is silent between hits.
        let pulse = VoiceClip::new("pulse", 150, 2, 600, 100, 0);
        let gaps = (0..4800)
            .filter(|i| {
                pulse
                    .sample(*i as f32 / RATE as f32 + 0.1)
                    .is_some_and(|s| s[0].abs() < 1e-4)
            })
            .count();
        assert!(gaps > 1500, "pulse train needs silent gaps, got {gaps}");
        // Double: a second hit follows the first after a gap.
        let double = VoiceClip::new("double", 60, 2, 600, 100, 0);
        let at = |t: f32| {
            (0..480)
                .map(|i| {
                    double
                        .sample(t + i as f32 / RATE as f32)
                        .map_or(0.0, |s| s[0].abs())
                })
                .fold(0.0f32, f32::max)
        };
        assert!(at(0.0) > 0.2 && at(0.17) > 0.1 && at(0.1) < at(0.17) * 0.5);
    }
    #[test]
    fn output_level_validates_applies_live_and_survives_stream_restarts() {
        let mut engine = AudioEngine::default();
        assert_eq!(
            engine.output_level(),
            "normal",
            "default must stay the shipped Normal"
        );
        assert_eq!(engine.shared.ceiling(), MAX_AMPLITUDE);
        assert!(engine.set_output_level("loud").is_err());
        assert!(engine.set_output_level("").is_err());
        assert_eq!(engine.output_level(), "normal");
        assert_eq!(engine.set_output_level("strong").unwrap(), "strong");
        assert_eq!(engine.shared.ceiling(), 0.50);
        assert_eq!(engine.set_output_level("soft").unwrap(), "soft");
        assert_eq!(engine.shared.ceiling(), 0.16);
        assert_eq!(engine.set_output_level("max").unwrap(), "max");
        assert_eq!(engine.shared.ceiling(), 0.75);
        assert_eq!(engine.diagnostics()["hapticOutputLevel"], "max");
        engine.fresh_output_state();
        assert_eq!(engine.output_level(), "max");
        engine.set_output_level("normal").unwrap();
        assert_eq!(engine.shared.ceiling(), MAX_AMPLITUDE);
    }
    #[test]
    fn grip_output_scales_with_level_and_strength_and_never_exceeds_the_ceiling() {
        let engine = AudioEngine::default();
        let mut by_level = Vec::new();
        for level in LEVEL_NAMES {
            engine.set_output_level(level).unwrap();
            let ceiling = engine.shared.ceiling();
            let peak = |strength: u8| {
                peak_of(
                    &engine.shared,
                    VoiceClip::new("sine", 90, 2, 300, strength, 0),
                    4096,
                )
            };
            let (low, mid, high) = (peak(30), peak(60), peak(100));
            assert!(
                low < mid && mid < high && high <= ceiling,
                "{level}: {low} {mid} {high}"
            );
            assert!(
                mid / low > 1.7 && high / mid > 1.3,
                "{level}: {low} {mid} {high}"
            );
            by_level.push(high);
        }
        // Four clearly stepped levels in peak.
        for pair in by_level.windows(2) {
            assert!(pair[1] / pair[0] >= 1.35, "{by_level:?}");
        }
    }
    #[test]
    fn sustained_rms_is_stepped_across_the_four_levels_through_the_render_path() {
        let engine = AudioEngine::default();
        let mut rms = Vec::new();
        for level in LEVEL_NAMES {
            engine.set_output_level(level).unwrap();
            let ceiling = engine.shared.ceiling();
            let clip = VoiceClip::new("sine", 90, 2, 650, 100, 0);
            let mut state = RenderState::default();
            let mut position = 0u64;
            engine.shared.clip.store(Some(Arc::new(clip)));
            let (mut square, mut count, mut peak) = (0.0f64, 0u64, 0.0f32);
            // Whole 0.65 s clip through the render path (voice clips are capped at 0.85 s).
            for _ in 0..(RATE as usize * 650 / 1000 / 512) {
                let mut data = [0.0f32; 512 * 4];
                render_output(&mut data, &engine.shared, &mut position, &mut state);
                for frame in data.as_chunks::<4>().0 {
                    square += f64::from(frame[2] * frame[2]);
                    count += 1;
                    peak = peak.max(frame[2].abs());
                }
            }
            assert!(peak <= ceiling, "{level}: {peak} > {ceiling}");
            rms.push((square / count as f64).sqrt());
        }
        for pair in rms.windows(2) {
            assert!(pair[1] / pair[0] >= 1.3, "RMS not stepped: {rms:?}");
        }
    }
    #[test]
    fn changing_level_mid_stream_ramps_without_a_step() {
        for (from, to) in [("soft", "max"), ("max", "soft"), ("normal", "strong")] {
            let engine = AudioEngine::default();
            engine.set_output_level(from).unwrap();
            engine
                .shared
                .clip
                .store(Some(Arc::new(VoiceClip::new("sine", 90, 2, 700, 100, 0))));
            let mut state = RenderState::default();
            let mut position = 0u64;
            let mut last = 0.0f32;
            let (mut worst, mut changed_at) = (0.0f32, None);
            for buffer in 0..40 {
                if buffer == 8 {
                    engine.set_output_level(to).unwrap();
                    changed_at = Some(position);
                }
                let mut data = [0.0f32; 512 * 4];
                render_output(&mut data, &engine.shared, &mut position, &mut state);
                for frame in data.as_chunks::<4>().0 {
                    if buffer > 0 {
                        worst = worst.max((frame[2] - last).abs());
                    }
                    last = frame[2];
                }
            }
            assert!(changed_at.is_some());
            // A 90 Hz sine at the Max ceiling moves ~0.008 per sample; an unsmoothed
            // level change would jump by several tenths at once.
            assert!(worst < 0.03, "{from}->{to}: largest step {worst}");
            let target = engine.shared.ceiling();
            assert!(
                (state.ceiling - target).abs() < target * 0.05,
                "ceiling settles"
            );
        }
    }
    /// `cargo test -p controller level_table -- --ignored --nocapture` prints absolute
    /// peak / RMS (whole clip window, render path) per level for four reference effects.
    #[test]
    #[ignore = "prints the measurement table"]
    fn level_table_prints_peak_and_rms_per_level() {
        let engine = AudioEngine::default();
        let cases = [
            ("click", "square", 240u16, 2u16, 28u16),
            ("tick120", "sine", 170, 6, 114),
            ("rumble600", "sine", 42, 120, 480),
            ("sine2s", "sine", 90, 5, 1995),
        ];
        for (name, shape, f, a, r) in cases {
            for level in LEVEL_NAMES {
                engine.set_output_level(level).unwrap();
                let clip = VoiceClip::new(shape, f, a, r, 100, 0);
                engine.shared.clip.store(Some(Arc::new(clip)));
                let mut state = RenderState::default();
                let mut position = 0u64;
                let frames = (u64::from(a + r) * u64::from(RATE) / 1000) as usize;
                let (mut square, mut count, mut peak) = (0.0f64, 0usize, 0.0f32);
                while count < frames {
                    let mut data = [0.0f32; 512 * 4];
                    render_output(&mut data, &engine.shared, &mut position, &mut state);
                    for frame in data.as_chunks::<4>().0 {
                        if count < frames {
                            square += f64::from(frame[2] * frame[2]);
                            peak = peak.max(frame[2].abs());
                            count += 1;
                        }
                    }
                }
                println!(
                    "{name:10} {level:7} peak {peak:.3} rms {:.3}",
                    (square / count as f64).sqrt()
                );
            }
        }
        // Continuous full-scale 90 Hz drive (game rumble, bridge): thermal limiter engaged.
        for (i, ceiling) in LEVEL_CEILINGS.into_iter().enumerate() {
            let mut duty = 0.0;
            let (mut sq, mut mean, mut n, mut peak) =
                ([0.0f64; 2], [0.0f64; 2], [0usize; 2], 0.0f32);
            for k in 0..RATE as usize * 8 {
                let x = (k as f32 * 90.0 * std::f32::consts::TAU / RATE as f32).sin();
                let y = soft_limit(x * 1.2, &mut duty, ceiling);
                peak = peak.max(y.abs());
                let w = match k / RATE as usize {
                    1 => Some(0),
                    7 => Some(1),
                    _ => None,
                };
                if let Some(w) = w {
                    sq[w] += f64::from(y * y);
                    mean[w] += f64::from(y.abs());
                    n[w] += 1;
                }
            }
            println!(
                "sustained {:7} peak {peak:.3} rms@2s {:.3} mean@2s {:.3} rms@8s {:.3} mean@8s {:.3}",
                LEVEL_NAMES[i],
                (sq[0] / n[0] as f64).sqrt(),
                mean[0] / n[0] as f64,
                (sq[1] / n[1] as f64).sqrt(),
                mean[1] / n[1] as f64
            );
        }
    }
    #[test]
    fn timeline_cues_accept_new_shapes_and_reject_unknown_ones() {
        let cue = |shape: &str| -> EffectClip {
            serde_json::from_value(serde_json::json!({"id":"x","name":"X","cues":[
                {"lane":"leftGrip","kind":"texture","startMs":0,"durationMs":300,"strength":100.0,"frequency":120,"shape":shape}
            ]}))
            .unwrap()
        };
        for shape in ["triangle", "pulse", "double", "swell"] {
            let clip = cue(shape);
            assert!(clip.validate().is_ok(), "{shape}");
            let compiled = clip.compile();
            let peak = (0..14_400)
                .map(|i| {
                    compiled.sample(i as f32 / RATE as f32, &PersonalSettings::default())[0].abs()
                })
                .fold(0.0f32, f32::max);
            assert!(
                peak.is_finite() && peak > 0.1 && peak <= 1.0,
                "{shape} {peak}"
            );
        }
        assert!(cue("wobble").validate().is_err());
    }
}
