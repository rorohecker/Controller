use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::audio_haptics::authoring::{EffectClip, PersonalSettings, limit_trigger};
use crate::input_map::{
    controls::{ControlProfile, RadialMapper},
    motion::{FactoryCalibration, MotionProcessor},
};
use crate::{
    audio_haptics::capture::CaptureSource,
    integrations::{DongleProbe, LinkExperiment},
};
use crate::{
    audio_haptics::{AudioEngine, VoiceClip},
    game_actions::{ActionMapper, ActionRule},
    game_link::{GameLink, LinkSettings},
    haptics::HapticClip,
    input_map::{
        DesktopMapper, GyroActivation, GyroAimConfig, GyroAimMapper, InputCalibration, MouseMapper,
        PcLaunchBinding, RemapBehavior, RemapBinding, RemapMapper, TouchCue,
    },
    processes,
    screen_color::{self, ScreenSampler},
    script::{ScriptCue, ScriptProgram},
    telemetry::{self, TelemetryBridge, TelemetryCue},
    xinput_bridge::XInputBridge,
};
use aftertouch_protocol::{self as protocol, TriggerSetting};
use base64::Engine;
use hidapi::{BusType, HidApi, HidDevice};
use serde::Deserialize;
use serde_json::json;
#[cfg(windows)]
static GUI_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn set_gui_pid(pid: u32) {
    #[cfg(windows)]
    GUI_PID.store(pid, std::sync::atomic::Ordering::Release);
    #[cfg(not(windows))]
    let _ = pid;
}
#[cfg(windows)]
fn gui_pid() -> Option<u32> {
    let pid = GUI_PID.load(std::sync::atomic::Ordering::Acquire);
    (pid != 0).then_some(pid)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum DeviceCommand {
    SetHidRecording {
        enabled: bool,
    },
    GetHidTrace,
    InspectHidTrace {
        json: String,
        #[serde(default, rename = "requestId")]
        request_id: u32,
    },
    SetRuntimeDiagnostics {
        enabled: bool,
    },
    LatencyProbe {
        id: u32,
    },
    SetNativeHandoff {
        enabled: bool,
    },
    NativeProviderControl {
        operation: crate::native_provider::Operation,
        #[serde(default, rename = "controllerId")]
        controller_id: Option<String>,
        #[serde(default, rename = "requestId")]
        request_id: u32,
        #[serde(default)]
        confirmed: bool,
    },
    CheckDesktopInput,
    ReadEdgeProfiles {
        #[serde(rename = "controllerId")]
        controller_id: String,
        #[serde(rename = "requestId")]
        request_id: u32,
    },
    WriteEdgeProfile {
        data: crate::hardware::EdgeSnapshot,
        slot: usize,
        #[serde(default)]
        edit: Option<crate::hardware::EdgeEdit>,
        #[serde(rename = "requestId")]
        request_id: u32,
    },
    ApplyControllerAssignment {
        #[serde(rename = "controllerId")]
        controller_id: String,
    },
    SetWeaponOverride {
        weapon: Option<String>,
    },
    StartLinkExperiment {
        strength: u8,
    },
    StopLinkExperiment,
    SetBluetoothAudio {
        enabled: bool,
        #[serde(rename = "controllerId")]
        controller_id: String,
    },
    ProfileCheckpoint {
        #[serde(rename = "requestId")]
        request_id: u32,
    },
    Scan,
    ListDevices,
    SelectDevice {
        path: String,
    },
    ApplyTriggers {
        left: TriggerSetting,
        right: TriggerSetting,
    },
    RenewTriggers,
    SetControlProfile {
        settings: ControlProfile,
    },
    /// Slow background gyro bias tracking while the controller is perfectly still.
    SetAutoBias {
        enabled: bool,
    },
    SetMotionBias {
        #[serde(rename = "biasDps")]
        bias_dps: [f32; 3],
    },
    CalibrateMotion {
        enabled: bool,
    },
    CancelRadial,
    PlayEffectClip {
        clip: EffectClip,
        #[serde(rename = "playId")]
        play_id: u64,
    },
    StopEffectClip {
        #[serde(rename = "playId")]
        play_id: u64,
    },
    SetPersonalHaptics {
        settings: PersonalSettings,
        #[serde(default, rename = "requestId")]
        request_id: u64,
    },
    PlayPersonalTest {
        side: u8,
        strength: u8,
    },
    SetGameActions {
        enabled: bool,
        rules: Vec<ActionRule>,
    },
    TestGameAction {
        rule: Box<ActionRule>,
    },
    SetGameLink {
        settings: LinkSettings,
    },
    SetLightbar {
        rgb: [u8; 3],
    },
    SetPlayerLeds {
        mask: u8,
    },
    ReleaseLightbar,
    BatteryLightbar {
        enabled: bool,
    },
    SetTemperatureLightbar {
        enabled: bool,
    },
    SetScreenLightbar {
        enabled: bool,
        /// Monitor index to sample (0 = primary). Omitted keeps the current choice.
        #[serde(default)]
        monitor: Option<u32>,
    },
    SetTouchMouse {
        enabled: bool,
        /// Touchpad pointer gain, 0.2..=3.0. Omitted keeps the current value.
        #[serde(default)]
        sensitivity: Option<f32>,
        /// Pointer acceleration, 0..=1. Omitted keeps the current value.
        #[serde(default)]
        acceleration: Option<f32>,
    },
    SetTouchGestures {
        enabled: bool,
    },
    SetGyroAim {
        enabled: bool,
        #[serde(default = "default_sens")]
        sensitivity: f32,
        #[serde(default = "default_axis")]
        #[serde(rename = "yawScale", alias = "yaw_scale")]
        yaw_scale: f32,
        #[serde(default = "default_axis", rename = "pitchScale", alias = "pitch_scale")]
        pitch_scale: f32,
        #[serde(default, rename = "invertY", alias = "invert_y")]
        invert_y: bool,
        #[serde(default = "default_activation")]
        activation: String,
        #[serde(default)]
        bias: [i16; 3],
        #[serde(default = "default_gyro_deadzone")]
        deadzone: u16,
        #[serde(default = "default_gyro_smoothing")]
        smoothing: f32,
        /// "hold" (default), "disable" (ratchet) or "toggle".
        #[serde(default = "default_gyro_gate")]
        gate: String,
        /// Gyro pauses while the right stick is deflected past this radius (0 = never).
        #[serde(default, rename = "stickOffThreshold", alias = "stick_off_threshold")]
        stick_off_threshold: f32,
        /// Smoothing applies only below this speed in deg/s (0 = always).
        #[serde(default, rename = "smoothThreshold", alias = "smooth_threshold")]
        smooth_threshold: f32,
        /// Extra gain on fast turns, 0..=2 (0 = linear).
        #[serde(default)]
        acceleration: f32,
        /// Speed in deg/s where acceleration starts (0 = 75).
        #[serde(default, rename = "accelThreshold", alias = "accel_threshold")]
        accel_threshold: f32,
        /// Let flick stick run while gyro aim itself is off.
        #[serde(default, rename = "flickWithoutGyro", alias = "flick_without_gyro")]
        flick_without_gyro: bool,
    },
    SetRemap {
        enabled: bool,
        #[serde(default)]
        bindings: Vec<RemapBindingDto>,
    },
    SetUiFocused {
        focused: bool,
    },
    SetPcMode {
        enabled: bool,
    },
    SetPcBindings {
        bindings: Vec<RemapBindingDto>,
        click_threshold: f32,
        pointer_sensitivity: f32,
        reverse_scroll: bool,
        /// PC pointer acceleration, 0..=1.
        #[serde(default, alias = "pointerAcceleration")]
        pointer_acceleration: f32,
        /// Left stick in PC mode: "off", "mouse" or "wasd".
        #[serde(default, alias = "leftStick")]
        left_stick: crate::input_map::LeftStickMode,
    },
    /// Remap key hold and inter-key gap in milliseconds (15..=500 / 5..=500).
    SetRemapTiming {
        #[serde(rename = "holdMs", alias = "hold_ms")]
        hold_ms: u16,
        #[serde(rename = "gapMs", alias = "gap_ms")]
        gap_ms: u16,
    },
    /// Map touchpad click / mic button onto virtual-pad buttons (A B X Y Lb Rb Back
    /// Start LeftStick RightStick Guide); null leaves it unmapped.
    SetPadExtras {
        #[serde(flatten)]
        extras: crate::virtual_gamepad::PadExtras,
    },
    SetCalibration {
        calibration: InputCalibration,
        #[serde(default)]
        measuring: bool,
    },
    ReloadOutputCalibration {
        #[serde(rename = "controllerId", alias = "controller_id")]
        controller_id: String,
    },
    SetXinput {
        enabled: bool,
    },
    PlayHaptic {
        shape: String,
        frequency: u16,
        attack: u16,
        release: u16,
        strength: u8,
        pan: i16,
    },
    PlaySpeakerChirp,
    SetMicrophoneMute {
        enabled: bool,
    },
    PlayAudioFile {
        path: String,
        position: f64,
        duration: f64,
        route: String,
        kind: String,
        #[serde(rename = "trackId")]
        track_id: u64,
    },
    SetAudioMixer {
        settings: crate::audio_haptics::MixerSettings,
    },
    PlaySpeakerSound {
        #[serde(default, rename = "trackId")]
        track_id: u64,
        #[serde(rename = "pcmBase64")]
        pcm_base64: String,
        route: String,
        #[serde(default)]
        append: bool,
    },
    StopSpeakerSound {
        #[serde(default, rename = "trackId")]
        track_id: u64,
    },
    SetSpeakerPaused {
        #[serde(default, rename = "trackId")]
        track_id: u64,
        paused: bool,
    },
    SetAudioBridge {
        enabled: bool,
        gain: f32,
        /// Fixed preset name or "custom" (see SetBridgeCustom).
        preset: String,
        #[serde(default)]
        source: CaptureSource,
    },
    /// Bridge latency: "low" 10 ms, "normal" 20 ms, "safe" 40 ms.
    SetBridgeLatency {
        value: String,
    },
    /// Overall intensity: "soft", "normal", "strong" or "max" grip output ceiling.
    SetHapticOutputLevel {
        level: String,
    },
    /// Parameters of the "custom" bridge preset; live while that preset runs.
    SetBridgeCustom {
        cutoff: f32,
        bass: f32,
        transient: f32,
        gate: f32,
    },
    /// Three-band mode, onset pulses and pan emphasis for the bridge.
    SetBridgeOptions {
        /// "standard" or "threeBand".
        #[serde(default = "default_bridge_mode")]
        mode: String,
        #[serde(
            default = "default_band_gains",
            rename = "bandGains",
            alias = "band_gains"
        )]
        band_gains: [f32; 3],
        #[serde(default)]
        onset: f32,
        #[serde(default, rename = "panEmphasis", alias = "pan_emphasis")]
        pan_emphasis: f32,
    },
    /// Re-attach to a restarted captured app for up to 30 s (next capture).
    SetProcessReattach {
        enabled: bool,
    },
    /// Waveform preview of a conversion kind for a file; answered with an `envelope`
    /// event. Runs on a worker thread.
    PreviewEnvelope {
        path: String,
        kind: String,
        #[serde(default = "default_envelope_points")]
        points: usize,
        #[serde(default, alias = "requestId")]
        id: u64,
    },
    SetTelemetry {
        enabled: bool,
        port: u16,
        #[serde(default)]
        lan: bool,
        #[serde(default = "default_telemetry_format")]
        format: String,
    },
    SetScript {
        enabled: bool,
        #[serde(default)]
        source: String,
    },
    SetInputEffects {
        enabled: bool,
        threshold: f32,
        strength: u8,
        #[serde(default = "default_weapon_kind")]
        kind: String,
        #[serde(default = "default_weapon_rate")]
        rate: u8,
        #[serde(rename = "spacingMs", default)]
        spacing_ms: u16,
        #[serde(rename = "kickMs", default)]
        kick_ms: u16,
        #[serde(rename = "kickStyle", default = "default_kick_style")]
        kick_style: String,
        #[serde(flatten)]
        tuning: protocol::BurstTuning,
    },
    SetLowBatteryAlert {
        enabled: bool,
    },
    /// Warning and critical battery levels (percent) for the low-battery alert.
    SetLowBatteryThresholds {
        warn: u8,
        critical: u8,
    },
    /// Unverified-on-hardware trigger modes; a `None` side is left unchanged.
    SetAdaptiveTrigger {
        #[serde(default)]
        left: Option<protocol::ExtraTrigger>,
        #[serde(default)]
        right: Option<protocol::ExtraTrigger>,
    },
    /// Microphone LED mode. UNVERIFIED on hardware: report layout is from community
    /// documentation and has not been confirmed on a physical controller.
    SetMicrophoneLed {
        mode: u8,
    },
    /// Headphone / speaker / microphone volumes. UNVERIFIED on hardware.
    SetAudioVolumes {
        #[serde(default)]
        headphone: Option<u8>,
        #[serde(default)]
        speaker: Option<u8>,
        #[serde(default)]
        microphone: Option<u8>,
    },
    /// Lightbar brightness and setup. UNVERIFIED on hardware.
    SetLedBrightness {
        #[serde(default)]
        brightness: Option<u8>,
        #[serde(default, rename = "lightbarSetup")]
        lightbar_setup: Option<u8>,
    },
    SetLightbarAnimation {
        mode: String,
        #[serde(default)]
        rgb: Option<[u8; 3]>,
        #[serde(default, rename = "periodMs")]
        period_ms: Option<u32>,
    },
    SetHapticTimer {
        seconds: u32,
    },
    Rumble {
        left: u8,
        right: u8,
        #[serde(rename = "durationMs")]
        duration_ms: u64,
    },
    SetGameRecording {
        enabled: bool,
    },
    StopMacros,
    Reset,
    Shutdown,
}

impl DeviceCommand {
    fn primes_audio(&self) -> bool {
        match self {
            Self::PlayEffectClip { clip, .. } => clip.has_grips(),
            Self::PlayHaptic { .. }
            | Self::PlaySpeakerChirp
            | Self::PlayAudioFile { .. }
            | Self::PlaySpeakerSound { .. }
            | Self::PlayPersonalTest { .. }
            | Self::TestGameAction { .. }
            | Self::StartLinkExperiment { .. } => true,
            Self::SetAudioBridge { enabled, .. } => *enabled,
            _ => false,
        }
    }
    fn motor_fallback(&self) -> bool {
        matches!(self, Self::PlayHaptic { .. } | Self::TestGameAction { .. })
    }
    fn cancels_audio_wait(&self) -> bool {
        matches!(
            self,
            Self::ReadEdgeProfiles { .. }
                | Self::Reset
                | Self::Shutdown
                | Self::Scan
                | Self::SelectDevice { .. }
                | Self::SetNativeHandoff { .. }
        )
    }
    fn bypasses_audio_wait(&self) -> bool {
        matches!(
            self,
            Self::SetMicrophoneMute { .. }
                | Self::SetPersonalHaptics { .. }
                | Self::SetAudioMixer { .. }
                | Self::SetInputEffects { enabled: false, .. }
                | Self::SetGameActions { enabled: false, .. }
                | Self::SetRemap { enabled: false, .. }
                | Self::SetXinput { enabled: false }
                | Self::SetTouchMouse { enabled: false, .. }
                | Self::SetGyroAim { enabled: false, .. }
                | Self::SetUiFocused { .. }
                | Self::SetPcMode { .. }
                | Self::SetRemapTiming { .. }
                | Self::SetPadExtras { .. }
                | Self::PreviewEnvelope { .. }
                | Self::StopMacros
                | Self::StopEffectClip { .. }
                | Self::StopLinkExperiment
                | Self::StopSpeakerSound { .. }
                | Self::SetAudioBridge { enabled: false, .. }
        )
    }
    fn cancels_queued(&self, queued: &Self) -> bool {
        match (self, queued) {
            (
                Self::StopSpeakerSound { track_id: stop },
                Self::PlayAudioFile { track_id, .. }
                | Self::PlaySpeakerSound { track_id, .. }
                | Self::SetSpeakerPaused { track_id, .. },
            ) => *stop == 0 || stop == track_id,
            (Self::StopEffectClip { play_id: stop }, Self::PlayEffectClip { play_id, .. }) => {
                *stop == 0 || stop == play_id
            }
            (Self::StopLinkExperiment, Self::StartLinkExperiment { .. }) => true,
            (
                Self::SetAudioBridge { enabled: false, .. },
                Self::SetAudioBridge { enabled: true, .. },
            ) => true,
            _ => false,
        }
    }
}
fn audio_failure(
    connected: &mut Connected,
    queue: &mut std::collections::VecDeque<DeviceCommand>,
    proxy: &Sender<serde_json::Value>,
    error: &str,
) {
    if let Some(status) = connected.audio.file_status() {
        emit(
            proxy,
            json!({"type":"speakerStatus","source":"error","trackId":status["trackId"],"message":error}),
        );
    }
    connected.audio.stop();
    connected.audio.remember_output_failure(error.to_owned());
    let _ = connected.set_speaker_route(false);
    fail_audio_sources(queue, proxy, error);
    emit(
        proxy,
        json!({"type":"audioStatus","route":"off","bridge":false,"message":error}),
    );
    if !queue.iter().any(DeviceCommand::motor_fallback) {
        emit(proxy, json!({"type":"error","message":error}));
    }
}
fn fail_audio_sources(
    queue: &mut std::collections::VecDeque<DeviceCommand>,
    proxy: &Sender<serde_json::Value>,
    message: &str,
) {
    let mut preserved = std::collections::VecDeque::new();
    let mut failed = std::collections::VecDeque::new();
    for command in queue.drain(..) {
        if command.primes_audio() && !command.motor_fallback() {
            failed.push_back(command);
        } else {
            preserved.push_back(command);
        }
    }
    cancel_audio_commands(&mut failed, proxy, message);
    *queue = preserved;
}
fn cancel_audio_commands(
    queue: &mut std::collections::VecDeque<DeviceCommand>,
    proxy: &Sender<serde_json::Value>,
    message: &str,
) {
    if queue.is_empty() {
        return;
    }
    emit(proxy, json!({"type":"error","message":message}));
    for command in queue.drain(..) {
        match command {
            DeviceCommand::ProfileCheckpoint { request_id } => emit(
                proxy,
                json!({"type":"profileCheckpoint","requestId":request_id,"connected":false,"error":message}),
            ),
            DeviceCommand::PlayAudioFile { track_id, .. }
            | DeviceCommand::PlaySpeakerSound { track_id, .. } => emit(
                proxy,
                json!({"type":"speakerStatus","source":"error","trackId":track_id,"message":message}),
            ),
            DeviceCommand::PlayEffectClip { play_id, .. } => emit(
                proxy,
                json!({"type":"effectTimeline","playing":false,"playId":play_id,"durationMs":0,"positionMs":0,"error":message}),
            ),
            DeviceCommand::StartLinkExperiment { .. } => emit(
                proxy,
                json!({"type":"linkExperiment","running":false,"error":message}),
            ),
            DeviceCommand::SetAudioBridge { .. } => emit(
                proxy,
                json!({"type":"audioStatus","route":"off","bridge":false,"message":message}),
            ),
            _ => {}
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RemapBindingDto {
    pub button: u8,
    pub vk: u16,
    #[serde(default)]
    pub modifier: Option<u8>,
    #[serde(default = "default_remap_behavior")]
    pub behavior: String,
    #[serde(default)]
    pub sequence: Vec<u16>,
    #[serde(default)]
    pub chord: Vec<u16>,
    #[serde(default, rename = "launchPath")]
    pub launch_path: Option<String>,
    #[serde(default, rename = "launchArgs")]
    pub launch_args: Vec<String>,
    #[serde(default)]
    pub program: Option<crate::macros::Program>,
    /// What pressing a program button does while it is running (PC mode only).
    #[serde(default, rename = "programMode", alias = "program_mode")]
    pub program_mode: Option<crate::input_map::ProgramMode>,
    /// Layer modifiers, long-press / double-tap triggers, turbo and consume.
    #[serde(flatten)]
    pub extras: crate::input_map::RemapExtras,
}

impl RemapBindingDto {
    /// Distinct bindings differ in button, layer or trigger.
    fn identity(&self) -> (u8, Vec<u8>, u16, u16) {
        let mut modifiers: Vec<u8> = self
            .modifier
            .into_iter()
            .chain(self.extras.modifiers.iter().copied())
            .collect();
        modifiers.sort_unstable();
        (
            self.button,
            modifiers,
            self.extras.long_press_ms,
            self.extras.double_tap_ms,
        )
    }
    fn into_binding(self, modifier: Option<u8>) -> RemapBinding {
        self.extras.apply(RemapBinding {
            button: self.button,
            vk: self.vk,
            modifier,
            behavior: RemapBehavior::from_label(&self.behavior).unwrap_or(RemapBehavior::Hold),
            sequence: self.sequence,
            chord: self.chord,
            ..Default::default()
        })
    }
}

/// Sequence/chord rules shared by PC shortcut and remap bindings.
fn macro_fields_valid(b: &RemapBindingDto) -> bool {
    let is_macro = b.behavior == "macro";
    let sequence_ok = if is_macro {
        !b.sequence.is_empty() && b.sequence.len() <= 5 && b.chord.is_empty()
    } else {
        b.sequence.is_empty()
    };
    sequence_ok
        && b.chord.len() <= 8
        && b.chord
            .iter()
            .all(|&vk| crate::input_map::supported_key(vk))
        && b.sequence
            .iter()
            .all(|&vk| crate::input_map::supported_key(vk))
}

fn default_remap_behavior() -> String {
    "hold".into()
}

fn default_telemetry_format() -> String {
    "forza".into()
}

fn default_sens() -> f32 {
    1.0
}
fn default_axis() -> f32 {
    1.0
}
fn default_activation() -> String {
    "always".into()
}
fn default_weapon_kind() -> String {
    "click".into()
}
fn default_weapon_rate() -> u8 {
    10
}
fn default_kick_style() -> String {
    "return".into()
}
fn default_gyro_deadzone() -> u16 {
    90
}
fn default_bridge_mode() -> String {
    "standard".into()
}
fn default_band_gains() -> [f32; 3] {
    crate::audio_haptics::BridgeOptions::default().band_gains
}
fn default_gyro_gate() -> String {
    "hold".into()
}
fn default_envelope_points() -> usize {
    256
}
fn default_gyro_smoothing() -> f32 {
    0.18
}
fn valid_launch_path(path: &str) -> bool {
    let file = std::path::Path::new(path);
    path.len() <= 1024
        && !path.contains('\0')
        && file.is_absolute()
        && if cfg!(windows) {
            file.is_file()
                && file
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        } else if cfg!(target_os = "macos") {
            file.is_file() || file.is_dir() && file.extension().is_some_and(|ext| ext == "app")
        } else {
            file.is_file()
        }
}

fn sensor_color(sensor_byte: u8) -> [u8; 3] {
    fn blend(from: [u8; 3], to: [u8; 3], step: u8, span: u8) -> [u8; 3] {
        std::array::from_fn(|index| {
            ((u16::from(from[index]) * u16::from(span - step)
                + u16::from(to[index]) * u16::from(step))
                / u16::from(span)) as u8
        })
    }
    // A visual mapping of the raw byte, not a calibrated temperature scale.
    if sensor_byte <= 127 {
        blend([60, 130, 200], [230, 165, 45], sensor_byte, 127)
    } else {
        blend([230, 165, 45], [220, 45, 50], sensor_byte - 127, 128)
    }
}

/// hidapi on Windows pads writes up to the report length; Ok(0) means unknown.
/// How long a game's last non-zero rumble is kept alive without any new update from the game.
const GAME_RUMBLE_SILENCE: Duration = Duration::from_secs(30);
#[derive(Debug, PartialEq, Eq)]
enum RumbleKeepalive {
    Wait,
    Refresh,
    Expire,
}
/// The audio side expires rumble after 10 s, so steady rumble is refreshed every 2 s, but only
/// while the game has produced a real update recently; otherwise a killed game would leave
/// the motors running forever.
fn rumble_keepalive(
    last_real: Option<Instant>,
    last_refresh: Option<Instant>,
    now: Instant,
) -> RumbleKeepalive {
    if last_real.is_none_or(|at| now.saturating_duration_since(at) >= GAME_RUMBLE_SILENCE) {
        return RumbleKeepalive::Expire;
    }
    if last_refresh.is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(2)) {
        RumbleKeepalive::Refresh
    } else {
        RumbleKeepalive::Wait
    }
}
fn full_write(written: usize, len: usize) -> bool {
    // Windows HID drivers can report 0 for a successful write; elsewhere 0 is a failure.
    (cfg!(windows) && written == 0) || written >= len
}

/// UI/clip rumble levels are 0-100; the report bytes are 0-255.
fn percent_to_motor(percent: u8) -> u8 {
    ((u16::from(percent.min(100)) * 255 + 50) / 100) as u8
}

/// Adaptive trigger "off" block.
const TRIGGER_OFF: [u8; 11] = protocol::TRIGGER_OFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LightAnimKind {
    Breathe,
    Pulse,
    Rainbow,
    LowBatteryBlink,
}

impl LightAnimKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "breathe" => Some(Self::Breathe),
            "pulse" => Some(Self::Pulse),
            "rainbow" => Some(Self::Rainbow),
            "lowBatteryBlink" => Some(Self::LowBatteryBlink),
            _ => None,
        }
    }
}

/// Core-driven lightbar animation; off unless a client starts one.
struct LightAnim {
    kind: LightAnimKind,
    rgb: [u8; 3],
    period_ms: u32,
    started: Instant,
    next: Instant,
}

fn hsv_to_rgb(hue: f32) -> [u8; 3] {
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [r, g, b].map(|channel: f32| (channel * 255.0).round() as u8)
}

/// Colour for an animation frame; pure so the shapes are testable.
fn animation_color(
    kind: LightAnimKind,
    base: [u8; 3],
    period_ms: u32,
    elapsed_ms: u64,
    battery: Option<u8>,
    warn: u8,
) -> [u8; 3] {
    let period = u64::from(period_ms.max(1));
    let phase = (elapsed_ms % period) as f32 / period as f32;
    let scale = |factor: f32| base.map(|c| (f32::from(c) * factor.clamp(0.0, 1.0)).round() as u8);
    match kind {
        LightAnimKind::Breathe => {
            scale(0.1 + 0.9 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos()))
        }
        LightAnimKind::Pulse => scale((1.0 - phase).powi(2)),
        LightAnimKind::Rainbow => hsv_to_rgb(phase * 360.0),
        LightAnimKind::LowBatteryBlink => {
            if battery.is_some_and(|level| level <= warn) {
                if phase < 0.5 {
                    [255, 40, 40]
                } else {
                    [0, 0, 0]
                }
            } else {
                base
            }
        }
    }
}

/// Output state the user explicitly set, kept per controller id across reconnects.
#[derive(Clone, Debug, PartialEq)]
struct Remembered {
    light: Option<[u8; 3]>,
    player_leds: Option<u8>,
    mic_muted: Option<bool>,
    low_battery: Option<(u8, u8)>,
}

impl Remembered {
    fn is_empty(&self) -> bool {
        self.light.is_none()
            && self.player_leds.is_none()
            && self.mic_muted.is_none()
            && self.low_battery.is_none()
    }
}

static OUTPUT_MEMORY: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Remembered>>,
> = std::sync::LazyLock::new(Default::default);

struct Connected {
    hid: HidDevice,
    path: Vec<u8>,
    path_display: String,
    controller_id: String,
    name: String,
    transport: BusType,
    product_id: u16,
    rumble_flags: (u8, u8),
    sequence: u8,
    input_sequence: Option<u8>,
    write_timing: crate::runtime_diagnostics::Rolling,
    trigger_active: bool,
    light_active: bool,
    player_leds_active: bool,
    assignment_error: Option<String>,
    rumble_active: bool,
    trigger_until: Option<Instant>,
    rumble_until: Option<Instant>,
    haptic: Option<HapticClip>,
    audio: AudioEngine,
    bluetooth_address: Option<String>,
    bt_audio_sequence: u8,
    speaker_route_until: Option<Instant>,
    speaker_track_id: u64,
    microphone_muted: Option<bool>,
    desktop: DesktopMapper,
    calibration: InputCalibration,
    calibration_measuring: bool,
    output_calibration: Option<crate::output_calibration::Record>,
    output_calibration_error: Option<String>,
    input_stale: bool,
    controls: ControlProfile,
    motion: MotionProcessor,
    radial: RadialMapper,
    last_motion_event: Instant,
    dongle: DongleProbe,
    link_experiment: Option<LinkExperiment>,
    last_input_sent: Instant,
    last_input_buttons: [bool; 23],
    last_valid_report: Instant,
    opened_at: Instant,
    panic_latched: bool,
    firmware: Option<String>,
    /// Feature version as `major.minor` (e.g. 2.21), parsed from report 0x20.
    firmware_version: Option<String>,
    /// Firmware older than 2.21 uses the legacy rumble flags.
    legacy_rumble: bool,
    /// Build date/time strings from report 0x20.
    firmware_build: Option<String>,
    last_trigger: Option<([u8; 11], [u8; 11])>,
    last_light: Option<[u8; 3]>,
    /// Colour the user explicitly set; re-applied after a reconnect.
    user_light: Option<[u8; 3]>,
    anim: Option<LightAnim>,
    battery_light: bool,
    sensor_light: bool,
    last_sensor: Option<u8>,
    screen_light: bool,
    screen_color: Option<[u8; 3]>,
    last_rumble: Option<[u8; 2]>,
    game_rumble: [u8; 2],
    /// Last time the game itself (not our keep-alive) sent rumble.
    game_feedback_at: Option<Instant>,
    game_audio_retry_at: Option<Instant>,
    input_effect: Option<InputEffect>,
    input_effect_until: Option<Instant>,
    bows: [Option<BowReturn>; 2],
    game_actions: ActionMapper,
    action_until: Option<Instant>,
    action_id: Option<String>,
    timeline: Option<TimelineRun>,
    timeline_generation: u64,
    personal: PersonalSettings,
    low_battery_alert: bool,
    /// Player-LED mask the user set explicitly (not the saved assignment).
    user_player_leds: Option<u8>,
    mic_user_set: bool,
    /// Events produced by `reset()` (it has no event channel); drained by the main loop.
    pending_events: Vec<serde_json::Value>,
    game_refresh_at: Option<Instant>,
    low_battery_latched: bool,
    low_battery_critical_latched: bool,
    /// (warn, critical) percent thresholds for the low-battery alert.
    battery_thresholds: (u8, u8),
    battery_key: Option<(Option<u8>, protocol::ChargeState)>,
    battery_unknown_sent: bool,
    jacks: Option<(bool, bool)>,
    last_battery: Option<u8>,
    haptic_timer: Option<Instant>,
    /// (brightness, lightbar setup) were changed by the user and must be restored on reset.
    led_brightness_dirty: (bool, bool),
}

struct TimelineRun {
    clip: EffectClip,
    start: Instant,
    previous: [Option<[u8; 11]>; 2],
    last_event: Instant,
}

struct InputEffect {
    threshold: f32,
    strength: u8,
    kind: WeaponKind,
    rate: u8,
    spacing_ms: u16,
    kick_ms: u16,
    remaining: u8,
    kick_style: KickStyle,
    tuning: protocol::BurstTuning,
    armed: bool,
    next_pulse: Option<Instant>,
    last_pulse: Option<Instant>,
    release_since: Option<Instant>,
}

struct BowReturn {
    peak: f32,
    unloaded: bool,
    high_point: f32,
}

impl Default for BowReturn {
    fn default() -> Self {
        Self {
            peak: 0.0,
            unloaded: false,
            high_point: 0.95,
        }
    }
}

impl BowReturn {
    fn update(&mut self, pull: f32) -> Option<bool> {
        if self.unloaded {
            if pull < 0.04 {
                self.peak = 0.0;
                self.unloaded = false;
                return Some(false);
            }
        } else {
            self.peak = self.peak.max(pull);
            if pull >= self.high_point || (self.peak > 0.12 && pull < self.peak - 0.035) {
                self.unloaded = true;
                return Some(true);
            }
        }
        None
    }
}

pub(crate) fn encode_live_trigger(setting: &TriggerSetting) -> Result<[u8; 11], &'static str> {
    setting.validate()?;
    if setting.mode == "weapon" && setting.preload_strength.unwrap_or(0.0) > 0.0 {
        let start = usize::from(setting.start_zone.unwrap_or(5));
        let end = usize::from(setting.end_zone.unwrap_or(6)).max(start + 1);
        let mut loaded = setting.clone();
        loaded.mode = "resistance".into();
        let preload = setting.preload_strength.unwrap_or(0.0) / setting.strength.max(1.0);
        loaded.zone_forces = Some(
            (0..10)
                .map(|zone| {
                    if zone < start {
                        (preload * (0.5 + 0.5 * zone as f32 / start.max(1) as f32)).min(1.0)
                    } else if zone < end {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect(),
        );
        return protocol::encode_trigger(&loaded);
    }
    if setting.mode != "bow" {
        return protocol::encode_trigger(setting);
    }
    let start = usize::from(setting.start_zone.unwrap_or(0));
    let end = usize::from(setting.end_zone.unwrap_or(9)).max(start + 1);
    let mut draw = setting.clone();
    draw.mode = "resistance".into();
    draw.zone_forces = Some(
        (0..10)
            .map(|zone| {
                if zone < start {
                    0.0
                } else if zone <= end {
                    0.25 + 0.75 * (zone - start + 1) as f32 / (end - start + 1) as f32
                } else {
                    0.0
                }
            })
            .collect(),
    );
    protocol::encode_trigger(&draw)
}

impl InputEffect {
    fn trigger_effect(&self) -> Result<[u8; 11], &'static str> {
        let mut curve = vec![0.0; 11];
        let full_travel = matches!(
            self.kind,
            WeaponKind::Sniper | WeaponKind::Burst | WeaponKind::Smg
        );
        if full_travel {
            let start = if self.kind == WeaponKind::Burst {
                usize::from(self.tuning.start_zone)
            } else {
                2
            };
            curve[start..].fill(1.0);
        } else {
            let zone = ((self.threshold * 10.0).round() as usize).clamp(2, 7);
            curve[zone + 1..].fill(1.0);
        }
        let mut setting = TriggerSetting {
            mode: if self.kind == WeaponKind::Smg {
                "vibration"
            } else if full_travel {
                "resistance"
            } else {
                "weapon"
            }
            .into(),
            strength: f32::from(self.strength),
            curve,
            force_cap: matches!(self.kind, WeaponKind::Sniper | WeaponKind::Burst).then_some(8),
            ..TriggerSetting::default()
        };
        if self.kind == WeaponKind::Burst {
            setting.zone_forces = Some(
                (0..10)
                    .map(|zone| {
                        if zone >= self.tuning.start_zone {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .collect(),
            );
            match self.kick_style {
                KickStyle::Return => {
                    // Native 0x22 has separate stop and return forces for each pulse.
                    // Reference: Nielk1 TriggerEffectGenerator.Bow; this firmware mode is unofficial.
                    setting.mode = "bow".into();
                    setting.start_zone = Some(self.tuning.start_zone);
                    setting.end_zone = Some(self.tuning.end_zone);
                    setting.snap_strength = Some(f32::from(self.tuning.return_strength));
                }
                KickStyle::Strike => {
                    setting.mode = "vibration".into();
                    setting.frequency_hz = Some(self.native_frequency());
                }
                KickStyle::Stop => {}
            }
        }
        protocol::encode_trigger(&setting)
    }

    fn burst_spacing_ms(&self) -> u16 {
        if self.spacing_ms > 0 {
            self.spacing_ms
        } else {
            1000 / u16::from(self.rate.max(1))
        }
    }

    fn native_frequency(&self) -> u8 {
        (1000.0 / f32::from(self.burst_spacing_ms()))
            .round()
            .clamp(1.0, 255.0) as u8
    }

    fn native_train(&self) -> bool {
        self.kind == WeaponKind::Burst && self.kick_style == KickStyle::Strike
    }

    fn trigger_duration(&self) -> Option<Duration> {
        if self.native_train() {
            // Send the native train once. Later shots only schedule their grip pulse.
            (self.remaining == 2).then(|| self.pulse_interval() * 3)
        } else {
            Some(Duration::from_millis(match self.kind {
                WeaponKind::Sniper => 120,
                WeaponKind::Burst => u64::from(self.burst_kick_ms()),
                _ => 65,
            }))
        }
    }

    fn pulse_interval(&self) -> Duration {
        if self.native_train() {
            Duration::from_secs_f64(1.0 / f64::from(self.native_frequency()))
        } else if self.kind == WeaponKind::Burst && self.spacing_ms > 0 {
            Duration::from_millis(u64::from(self.spacing_ms))
        } else {
            Duration::from_secs_f64(1.0 / f64::from(self.rate.max(1)))
        }
    }

    fn burst_kick_ms(&self) -> u16 {
        let requested = if self.kick_ms == 0 { 95 } else { self.kick_ms };
        requested.min(self.burst_spacing_ms().saturating_sub(20).max(20))
    }

    fn advance(&mut self, now: Instant, right_pull: f32) -> bool {
        let interval = self.pulse_interval();
        let release_point = if matches!(self.kind, WeaponKind::Sniper | WeaponKind::Burst) {
            0.15
        } else {
            self.threshold - 0.08
        };
        let at_rest = right_pull < release_point;
        let released = if self.kind != WeaponKind::Burst || self.armed {
            at_rest
        } else if !at_rest
            || self.last_pulse.is_some_and(|at| {
                now.saturating_duration_since(at)
                    < Duration::from_millis(u64::from(self.burst_kick_ms()) + 20)
            })
        {
            // The motor's return stroke can briefly take the trigger to rest while the finger is still holding it.
            self.release_since = None;
            false
        } else {
            let since = *self.release_since.get_or_insert(now);
            now.saturating_duration_since(since) >= Duration::from_millis(60)
        };
        if released {
            self.armed = true;
            self.next_pulse = None;
            self.remaining = 0;
            self.last_pulse = None;
            self.release_since = None;
        }
        let first = self.armed && right_pull >= self.threshold;
        if first {
            self.armed = false;
            self.remaining = if self.kind == WeaponKind::Burst { 2 } else { 0 };
            self.next_pulse = Some(now + interval);
            self.last_pulse = Some(now);
            return true;
        }
        let repeat = self.next_pulse.is_some_and(|at| now >= at)
            && ((self.kind == WeaponKind::Smg && right_pull >= self.threshold)
                || (self.kind == WeaponKind::Burst && self.remaining > 0));
        if repeat {
            self.remaining = self.remaining.saturating_sub(1);
            self.next_pulse = Some(now + interval);
            self.last_pulse = Some(now);
        }
        repeat
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KickStyle {
    Return,
    Strike,
    Stop,
}

impl KickStyle {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "return" => Some(Self::Return),
            "strike" => Some(Self::Strike),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WeaponKind {
    Click,
    Pistol,
    Sniper,
    Burst,
    Smg,
}

impl WeaponKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "click" => Some(Self::Click),
            "pistol" => Some(Self::Pistol),
            "sniper" => Some(Self::Sniper),
            "burst" => Some(Self::Burst),
            "smg" => Some(Self::Smg),
            _ => None,
        }
    }
    fn cue(self, strength: u8) -> (&'static str, u16, u16, u16, u8, i16) {
        match self {
            Self::Click => ("square", 145, 2, 75, (strength / 2).max(1), 45),
            Self::Pistol => ("square", 155, 2, 105, strength, 35),
            Self::Sniper => ("sniper", 68, 2, 115, strength, 0),
            Self::Burst => ("square", 110, 2, 95, strength, 20),
            Self::Smg => ("square", 95, 2, 75, strength, 20),
        }
    }
}

impl Connected {
    fn stop_link_experiment(&mut self) -> Result<Option<serde_json::Value>, String> {
        let Some(run) = self.link_experiment.take() else {
            return Ok(None);
        };
        self.audio.cancel_cue();
        if run.bluetooth {
            let silence =
                protocol::bt_haptic_report_sequences(&[0; 64], self.sequence, run.sequence);
            self.sequence = self.sequence.wrapping_add(1) & 15;
            let written = self.hid.write(&silence).map_err(|e| e.to_string())?;
            if !full_write(written, silence.len()) {
                return Err("Short Bluetooth experiment stop write".into());
            }
            self.write_rumble([0, 0])?;
        }
        Ok(Some(run.event(false)))
    }
    fn tick_link_experiment(&mut self) -> Result<Option<serde_json::Value>, String> {
        let now = Instant::now();
        if self
            .link_experiment
            .as_ref()
            .is_some_and(|run| now >= run.deadline)
        {
            return self.stop_link_experiment();
        }
        let Some(run) = self.link_experiment.as_mut() else {
            return Ok(None);
        };
        if !run.bluetooth {
            run.sent = self
                .audio
                .callbacks()
                .saturating_sub(run.callback_base)
                .min(u64::from(u32::MAX)) as u32;
        }
        if let Some(mut pcm) = run.due(now) {
            for (i, sample) in pcm.iter_mut().enumerate() {
                *sample = (f32::from(*sample) * self.personal.side_limit(i % 2)).round() as i8;
            }
            let frame = protocol::bt_haptic_report_sequences(&pcm, self.sequence, run.sequence);
            self.sequence = self.sequence.wrapping_add(1) & 15;
            run.sequence = run.sequence.wrapping_add(1);
            let start = Instant::now();
            let n = self.hid.write(&frame).map_err(|e| e.to_string())?;
            if !full_write(n, frame.len()) {
                return Err("Short Bluetooth experiment write".into());
            }
            run.write_us = run.write_us.max(start.elapsed().as_micros() as u64);
            run.sent += 1;
        }
        if now.duration_since(run.last_event) >= Duration::from_millis(100) {
            run.last_event = now;
            return Ok(Some(run.event(true)));
        }
        Ok(None)
    }
    fn enable_pcm(&mut self) -> Result<(), String> {
        self.haptic = None;
        self.rumble_until = None;
        if self.rumble_active {
            self.write_rumble([0, 0])?;
        }
        self.last_rumble = None;
        self.write(protocol::audio_haptics_enable_report())
    }
    fn write_bt_audio(&mut self, pcm: &[i8; 64]) -> Result<(), String> {
        let report =
            protocol::bt_haptic_report_sequences(pcm, self.sequence, self.bt_audio_sequence);
        self.sequence = self.sequence.wrapping_add(1) & 15;
        self.bt_audio_sequence = self.bt_audio_sequence.wrapping_add(1);
        let count = self.hid.write(&report).map_err(|e| e.to_string())?;
        if !full_write(count, report.len()) {
            return Err("Short Bluetooth grip-audio write.".into());
        }
        Ok(())
    }
    fn tick_bt_audio(&mut self) -> Result<(), String> {
        if let Some(pcm) = self.audio.take_bluetooth_packet(Instant::now())? {
            self.write_bt_audio(&pcm)?;
        }
        Ok(())
    }
    fn load_assignment(&mut self) -> Result<(), String> {
        let result = (|| {
            let record = match crate::hardware::assignment(&self.controller_id) {
                Ok(record) => record,
                Err(error) => {
                    self.audio.configure_endpoint(None, Some(error.clone()));
                    return Err(error);
                }
            };
            self.audio
                .configure_endpoint(record.as_ref().and_then(|r| r.endpoint_id.clone()), None);
            let player = record.as_ref().map_or(0, |r| r.player);
            if player > 0 {
                self.write(protocol::player_leds_report(
                    [0, 4, 10, 21, 27, 31][usize::from(player)],
                ))?;
                self.player_leds_active = true;
            }
            Ok(())
        })();
        self.assignment_error = result.as_ref().err().cloned();
        result
    }

    fn play_voice(&mut self, voice: VoiceClip) -> Result<&str, String> {
        self.enable_pcm()?;
        self.audio.play(voice)
    }

    fn play_feedback(
        &mut self,
        shape: &str,
        frequency: u16,
        attack: u16,
        release: u16,
        strength: u8,
        pan: i16,
    ) {
        let voice = VoiceClip::new(shape, frequency, attack, release, strength, pan);
        if (matches!(self.transport, BusType::Usb) || self.audio.bluetooth_enabled())
            && self.play_voice(voice).is_ok()
        {
            return;
        }
        let fallback = HapticClip::new(shape.into(), frequency, attack, release, strength, pan);
        self.rumble_until = Some(Instant::now() + fallback.duration());
        self.haptic = Some(fallback);
    }

    fn start_timeline(&mut self, clip: EffectClip) -> Result<(), String> {
        clip.validate().map_err(str::to_string)?;
        if clip.has_grips()
            && !matches!(self.transport, BusType::Usb)
            && !self.audio.bluetooth_enabled()
        {
            return Err(
                "Stereo timeline grips require USB; trigger-only clips work over Bluetooth".into(),
            );
        }
        self.restore_action()?;
        self.audio.cancel_cue();
        if clip.has_grips() {
            self.enable_pcm()?;
            self.audio.play_timeline(clip.clone())?;
        }
        let start = Instant::now();
        self.trigger_active |= clip.trigger_mask() != 0;
        self.action_until = Some(start + Duration::from_millis(u64::from(clip.duration_ms())));
        self.timeline = Some(TimelineRun {
            clip,
            start,
            previous: [None, None],
            last_event: start - Duration::from_millis(50),
        });
        Ok(())
    }
    fn tick_timeline(&mut self) -> Result<Option<serde_json::Value>, String> {
        let Some(timeline) = self.timeline.as_ref() else {
            return Ok(None);
        };
        let elapsed = timeline
            .start
            .elapsed()
            .as_millis()
            .min(u128::from(u32::MAX)) as u32;
        let duration = timeline.clip.duration_ms();
        if elapsed >= duration {
            self.restore_action()?;
            return Ok(Some(
                json!({"type":"effectTimeline","playId":self.timeline_generation,"playing":false,"positionMs":duration,"durationMs":duration}),
            ));
        }
        let blocks = timeline
            .clip
            .triggers(elapsed - elapsed % 10, self.personal.cap());
        if blocks != timeline.previous {
            self.write(protocol::usb_report(
                (if blocks[0].is_some() { 8 } else { 0 })
                    | (if blocks[1].is_some() { 4 } else { 0 }),
                0,
                0,
                blocks[1],
                blocks[0],
                None,
                None,
                false,
            ))?;
            if let Some(timeline) = self.timeline.as_mut() {
                timeline.previous = blocks;
            }
        }
        let timeline = self.timeline.as_mut().unwrap();
        if timeline.last_event.elapsed() >= Duration::from_millis(50) {
            timeline.last_event = Instant::now();
            return Ok(Some(
                json!({"type":"effectTimeline","playId":self.timeline_generation,"name":timeline.clip.name,"triggerMask":timeline.clip.trigger_mask(),"playing":true,"positionMs":elapsed,"durationMs":duration}),
            ));
        }
        Ok(None)
    }
    fn play_action(&mut self, rule: &ActionRule) -> Result<(), String> {
        self.stop_action();
        if let Some(clip) = &rule.timeline {
            self.start_timeline(clip.clone())?;
            self.action_id = Some(rule.id.clone());
            return Ok(());
        }
        if let Some(h) = &rule.haptic {
            self.play_feedback(
                &h.shape,
                h.frequency,
                3,
                rule.duration_ms.min(650),
                h.strength,
                h.pan,
            );
        }
        if rule.left_trigger.is_some() || rule.right_trigger.is_some() {
            let off = TRIGGER_OFF;
            let (base_left, base_right) = self.last_trigger.unwrap_or((off, off));
            let left = rule
                .left_trigger
                .as_ref()
                .map(encode_live_trigger)
                .transpose()?
                .unwrap_or(base_left);
            let right = rule
                .right_trigger
                .as_ref()
                .map(encode_live_trigger)
                .transpose()?
                .unwrap_or(base_right);
            self.write(protocol::usb_report(
                (if rule.left_trigger.is_some() { 0x08 } else { 0 })
                    | (if rule.right_trigger.is_some() {
                        0x04
                    } else {
                        0
                    }),
                0,
                0,
                Some(right),
                Some(left),
                None,
                None,
                false,
            ))?;
            self.trigger_active = true;
            self.input_effect_until = None;
            if let Some(effect) = self.input_effect.as_mut() {
                effect.remaining = 0;
                effect.next_pulse = None;
                effect.armed = false;
            }
            self.action_until =
                Some(Instant::now() + Duration::from_millis(u64::from(rule.duration_ms)));
        }
        self.action_id = Some(rule.id.clone());
        self.action_until =
            Some(Instant::now() + Duration::from_millis(u64::from(rule.duration_ms)));
        Ok(())
    }
    fn cancel_actions(&mut self) {
        let cancelled = self.game_actions.take_cancelled();
        if self
            .action_id
            .as_ref()
            .is_some_and(|id| cancelled.contains(id))
        {
            self.stop_action();
        }
    }
    fn stop_action(&mut self) {
        if self.action_id.is_some() || self.timeline.is_some() {
            self.audio.cancel_cue();
            self.haptic = None;
            if self.rumble_active {
                let _ = self.write_rumble([0, 0]);
            }
            let _ = self.restore_action();
        }
    }
    fn restore_action(&mut self) -> Result<(), String> {
        if self.action_until.is_none() {
            return Ok(());
        }
        if let Some(timeline) = self.timeline.as_ref() {
            let expired = self
                .trigger_until
                .is_some_and(|until| Instant::now() >= until);
            let mask = if expired {
                12
            } else {
                timeline.clip.trigger_mask()
            };
            let off = TRIGGER_OFF;
            let (left, right) = if expired {
                (off, off)
            } else {
                self.last_trigger.unwrap_or((off, off))
            };
            if mask != 0 {
                self.write(protocol::usb_report(
                    mask,
                    0,
                    0,
                    (mask & 4 != 0).then_some(right),
                    (mask & 8 != 0).then_some(left),
                    None,
                    None,
                    false,
                ))?;
            }
            self.timeline = None;
            self.audio.stop_timeline();
            self.action_until = None;
            self.action_id = None;
            self.trigger_active = left[0] != 5 || right[0] != 5;
            return Ok(());
        }
        let off = TRIGGER_OFF;
        let (left, right) = self.last_trigger.unwrap_or((off, off));
        self.write(protocol::usb_report(
            0x0c,
            0,
            0,
            Some(right),
            Some(left),
            None,
            None,
            false,
        ))?;
        self.action_until = None;
        self.action_id = None;
        self.trigger_active = left[0] != 0x05 || right[0] != 0x05;
        Ok(())
    }
    fn set_speaker_route(&mut self, enabled: bool) -> Result<(), String> {
        if !enabled
            && matches!(self.transport, BusType::Bluetooth)
            && self.audio.bluetooth_enabled()
        {
            return Ok(());
        }
        if !matches!(self.transport, BusType::Usb) {
            return Err("Controller speaker routing requires USB.".into());
        }
        self.write(protocol::speaker_route_report(enabled))?;
        if !enabled {
            self.speaker_route_until = None;
        }
        Ok(())
    }

    fn set_microphone_mute(&mut self, muted: bool) -> Result<(), String> {
        let previous = self.microphone_muted;
        self.microphone_muted = Some(muted);
        if let Err(error) = self.write(protocol::microphone_mute_report(muted)) {
            self.microphone_muted = previous;
            return Err(error);
        }
        self.mic_user_set = true;
        Ok(())
    }

    fn write(&mut self, report: [u8; protocol::USB_OUTPUT_LEN]) -> Result<(), String> {
        let started = Instant::now();
        let result = self.write_report(report);
        self.write_timing
            .record(crate::runtime_diagnostics::elapsed_us(started));
        result
    }
    fn write_report(&mut self, mut report: [u8; protocol::USB_OUTPUT_LEN]) -> Result<(), String> {
        if self.personal.enabled {
            for (flag, offset) in [(4, 11), (8, 22)] {
                if report[1] & flag != 0 {
                    let block: [u8; 11] = report[offset..offset + 11].try_into().unwrap();
                    report[offset..offset + 11]
                        .copy_from_slice(&limit_trigger(block, self.personal.cap()));
                }
            }
            report[4] = (f32::from(report[4]) * self.personal.side_limit(0)).round() as u8;
            report[3] = (f32::from(report[3]) * self.personal.side_limit(1)).round() as u8;
        }
        if report[1] & 0x0c != 0 {
            protocol::use_full_motor_power(&mut report);
        }
        if report[2] & 0x02 != 0 && self.microphone_muted.unwrap_or(false) {
            report[10] |= 0x10;
        }
        if matches!(self.transport, BusType::Bluetooth) {
            let bt = protocol::bt_report(&report, self.sequence);
            self.sequence = self.sequence.wrapping_add(1);
            match self.hid.send_output_report(&bt) {
                Ok(()) => return Ok(()),
                Err(_) => self.hid.write(&bt),
            }
        } else {
            self.hid.write(&report)
        }
        .and_then(|written| {
            let required = if matches!(self.transport, BusType::Bluetooth) {
                78
            } else {
                48
            };
            if written >= required {
                Ok(written)
            } else {
                Err(hidapi::HidError::HidApiError {
                    message: format!("Only {written} output bytes accepted; expected {required}"),
                })
            }
        })
        .map(|_| ())
        .map_err(|error| format!("Controller write failed: {error}"))
    }

    /// `motors` are raw report levels (0-255); use `percent_to_motor` for 0-100 input.
    fn write_rumble(&mut self, motors: [u8; 2]) -> Result<(), String> {
        let result = if self.last_rumble == Some(motors) {
            Ok(())
        } else {
            self.write(protocol::usb_report(
                self.rumble_flags.0,
                0,
                self.rumble_flags.1,
                None,
                None,
                None,
                Some(motors),
                false,
            ))
        };
        result.map(|_| {
            self.rumble_active = motors != [0, 0];
            self.last_rumble = Some(motors);
        })
    }

    fn reset(&mut self) -> Result<(), String> {
        let _ = self.stop_link_experiment();
        self.desktop.set_enabled(false);
        let _ = self.radial.set(Default::default());
        self.motion.reset();
        self.controls = ControlProfile::default();
        self.haptic = None;
        if self.audio.bluetooth_enabled() {
            self.pending_events.push(
                json!({"type":"bluetoothAudioStatus","controllerId":self.controller_id,"enabled":false}),
            );
        }
        self.audio.set_bluetooth(false);
        self.game_rumble = [0, 0];
        self.game_audio_retry_at = None;
        let restore_speaker = self.speaker_route_until.is_some();
        self.speaker_route_until = None;
        if restore_speaker {
            self.set_speaker_route(false)?;
        }
        self.input_effect = None;
        self.input_effect_until = None;
        self.bows = [None, None];
        if self.low_battery_alert {
            self.pending_events
                .push(json!({"type":"hapticAlertStatus","lowBattery":false}));
        }
        if self.haptic_timer.is_some() {
            self.pending_events
                .push(json!({"type":"hapticAlertStatus","timerSeconds":0}));
        }
        self.low_battery_alert = false;
        self.low_battery_latched = false;
        self.low_battery_critical_latched = false;
        self.haptic_timer = None;
        self.user_light = None;
        self.anim = None;
        self.user_player_leds = None;
        self.mic_user_set = false;
        self.screen_light = false;
        self.screen_color = None;
        self.sensor_light = false;
        if self.led_brightness_dirty != (false, false) {
            let (brightness, setup) = self.led_brightness_dirty;
            if let Ok(report) =
                protocol::led_brightness_report(brightness.then_some(0), setup.then_some(0))
                && self.write(report).is_ok()
            {
                self.led_brightness_dirty = (false, false);
            }
        }
        self.game_actions = ActionMapper::default();
        self.timeline = None;
        self.timeline_generation = self.timeline_generation.saturating_add(1);
        self.action_until = None;
        self.action_id = None;
        self.last_sensor = None;
        if !(self.trigger_active
            || self.light_active
            || self.rumble_active
            || self.player_leds_active)
        {
            return Ok(());
        }
        let off = TRIGGER_OFF;
        let valid0 = (if self.trigger_active { 0x0c } else { 0 })
            | if self.rumble_active {
                self.rumble_flags.0
            } else {
                0
            };
        let mut report = protocol::usb_report(
            valid0,
            (if self.light_active { 0x08 } else { 0 })
                | if self.player_leds_active { 0x10 } else { 0 },
            if self.rumble_active {
                self.rumble_flags.1
            } else {
                0
            },
            self.trigger_active.then_some(off),
            self.trigger_active.then_some(off),
            None,
            if self.rumble_active {
                Some([0, 0])
            } else {
                None
            },
            false,
        );
        report[44] = 0;
        self.write(report)?;
        self.trigger_active = false;
        self.light_active = false;
        self.player_leds_active = false;
        self.rumble_active = false;
        self.trigger_until = None;
        self.rumble_until = None;
        self.last_trigger = None;
        self.last_light = None;
        self.battery_light = false;
        self.last_rumble = None;
        // Reset releases effects, not the player number the user assigned to this pad.
        self.restore_assigned_player_leds();
        Ok(())
    }

    /// Re-light the saved player-number assignment (used after `reset()` clears the LEDs).
    fn restore_assigned_player_leds(&mut self) {
        if let Ok(Some(record)) = crate::hardware::assignment(&self.controller_id)
            && (1..=5).contains(&record.player)
            && self
                .write(protocol::player_leds_report(
                    [0, 4, 10, 21, 27, 31][usize::from(record.player)],
                ))
                .is_ok()
        {
            self.player_leds_active = true;
        }
    }

    /// Remember only what the user explicitly set so it survives a reconnect or transport switch.
    fn remember(&self) {
        let remembered = Remembered {
            light: self.user_light,
            player_leds: self.user_player_leds,
            mic_muted: self.mic_user_set.then_some(self.microphone_muted).flatten(),
            low_battery: self.low_battery_alert.then_some(self.battery_thresholds),
        };
        if let Ok(mut map) = OUTPUT_MEMORY.lock() {
            if remembered.is_empty() {
                map.remove(&self.controller_id);
            } else {
                map.insert(self.controller_id.clone(), remembered);
            }
        }
    }

    /// Re-apply remembered output state for this controller id; conservative and best-effort.
    fn restore_remembered(&mut self, proxy: &Sender<serde_json::Value>) {
        let Some(remembered) = OUTPUT_MEMORY
            .lock()
            .ok()
            .and_then(|mut map| map.remove(&self.controller_id))
        else {
            return;
        };
        let mut restored = Vec::new();
        if let Some(rgb) = remembered.light
            && self.set_light(rgb).is_ok()
        {
            self.user_light = Some(rgb);
            restored.push("lightbar");
        }
        if let Some(mask) = remembered.player_leds
            && self.write(protocol::player_leds_report(mask)).is_ok()
        {
            self.player_leds_active = true;
            self.user_player_leds = Some(mask);
            restored.push("playerLeds");
        }
        if let Some(muted) = remembered.mic_muted
            && self.set_microphone_mute(muted).is_ok()
        {
            emit(proxy, json!({"type":"microphoneStatus","muted":muted}));
            restored.push("microphone");
        }
        if let Some((warn, critical)) = remembered.low_battery {
            self.low_battery_alert = true;
            self.battery_thresholds = (warn, critical);
            emit(
                proxy,
                json!({"type":"hapticAlertStatus","lowBattery":true,"warn":warn,"critical":critical}),
            );
            restored.push("lowBatteryAlert");
        }
        if !restored.is_empty() {
            emit(
                proxy,
                json!({"type":"outputRestored","controllerId":self.controller_id,"restored":restored}),
            );
        }
    }

    /// Unconditional lightbar write that keeps the cached state in step.
    fn write_light(&mut self, rgb: [u8; 3]) -> Result<(), String> {
        self.write(protocol::usb_report(
            0,
            0x04,
            0,
            None,
            None,
            Some(rgb),
            None,
            false,
        ))?;
        self.light_active = true;
        self.last_light = Some(rgb);
        Ok(())
    }

    /// Lightbar write that skips redundant reports.
    fn set_light(&mut self, rgb: [u8; 3]) -> Result<(), String> {
        if self.last_light == Some(rgb) {
            self.light_active = true;
            return Ok(());
        }
        self.write_light(rgb)
    }

    /// Emit `battery` and `audioJacks` events when the reported state changes.
    fn track_status(&mut self, info: &protocol::BatteryInfo, proxy: &Sender<serde_json::Value>) {
        self.battery_unknown_sent = false;
        let key = (info.percent, info.charge_state);
        if self.battery_key != Some(key) {
            self.battery_key = Some(key);
            if let Ok(mut live) = LIVE_BATTERY.lock() {
                *live = Some((self.path_display.clone(), info.percent, info.charging));
            }
            let faults = battery_faults(info.charge_state);
            emit(
                proxy,
                json!({"type":"battery","controllerId":self.controller_id,"available":true,
                "level":info.percent,"levelTenths":info.level_tenths,"charging":info.charging,
                "chargeState":info.charge_state,"label":info.charge_state.label(),
                "fault":!faults.is_empty(),"faults":faults}),
            );
        }
        let jacks = (info.headphone_jack, info.microphone_jack);
        if self.jacks != Some(jacks) {
            self.jacks = Some(jacks);
            emit(
                proxy,
                json!({"type":"audioJacks","controllerId":self.controller_id,"headphones":jacks.0,"microphone":jacks.1}),
            );
        }
    }

    /// Advance the optional lightbar animation (20 Hz at most).
    fn tick_light_animation(&mut self) {
        let Some(anim) = self.anim.as_ref() else {
            return;
        };
        if self.battery_light || self.screen_light || self.sensor_light {
            self.anim = None;
            return;
        }
        let now = Instant::now();
        if now < anim.next {
            return;
        }
        let color = animation_color(
            anim.kind,
            anim.rgb,
            anim.period_ms,
            now.duration_since(anim.started).as_millis() as u64,
            self.last_battery,
            self.battery_thresholds.0,
        );
        if let Some(anim) = self.anim.as_mut() {
            anim.next = now + Duration::from_millis(50);
        }
        let _ = self.set_light(color);
    }

    fn release_lightbar(&mut self) -> Result<(), String> {
        self.anim = None;
        self.user_light = None;
        if !self.light_active {
            return Ok(());
        }
        self.write(protocol::usb_report(
            0, 0x08, 0, None, None, None, None, false,
        ))?;
        self.light_active = false;
        self.last_light = None;
        self.battery_light = false;
        Ok(())
    }

    fn restore_input_effect(&mut self) -> Result<(), String> {
        if self.input_effect_until.is_none() {
            return Ok(());
        }
        let off = TRIGGER_OFF;
        let right = self.last_trigger.map(|(_, right)| right).unwrap_or(off);
        self.write(protocol::usb_report(
            0x04,
            0,
            0,
            Some(right),
            None,
            None,
            None,
            false,
        ))?;
        self.input_effect_until = None;
        self.trigger_active = self
            .last_trigger
            .is_some_and(|(l, r)| l[0] != 0x05 || r[0] != 0x05);
        Ok(())
    }

    fn update_input_effect(&mut self, right_pull: f32) -> Result<(), String> {
        if self.action_until.is_some() {
            return Ok(());
        }
        if self.input_effect_until.is_some_and(|t| Instant::now() >= t) {
            self.restore_input_effect()?;
        }
        let Some(effect) = self.input_effect.as_mut() else {
            return Ok(());
        };
        if !effect.advance(Instant::now(), right_pull) {
            if effect.native_train() && effect.armed {
                self.restore_input_effect()?;
            }
            return Ok(());
        }
        let kind = effect.kind;
        let strength = effect.strength;
        let duration = effect.trigger_duration();
        let tuning = effect.tuning;
        if let Some(duration) = duration {
            let click = effect.trigger_effect().map_err(str::to_string)?;
            if self.input_effect_until.is_some() {
                self.restore_input_effect()?;
            }
            self.write(protocol::usb_report(
                0x04,
                0,
                0,
                Some(click),
                None,
                None,
                None,
                false,
            ))?;
            self.trigger_active = true;
            self.input_effect_until = Some(Instant::now() + duration);
        }
        let (shape, frequency, attack, release, power, pan) = kind.cue(strength);
        if !matches!(kind, WeaponKind::Sniper | WeaponKind::Burst) {
            self.play_feedback(shape, frequency, attack, release, power, pan);
        } else if kind == WeaponKind::Burst && tuning.haptic_strength > 0 {
            self.play_feedback(
                "square",
                tuning.haptic_frequency,
                2,
                tuning.haptic_duration_ms,
                tuning.haptic_strength,
                0,
            );
        }
        Ok(())
    }

    fn update_bow_returns(&mut self, pulls: [f32; 2]) -> Result<(), String> {
        if self.action_until.is_some() {
            return Ok(());
        }
        for (side, pull) in pulls.into_iter().enumerate() {
            let change = self.bows[side].as_mut().and_then(|bow| bow.update(pull));
            if let Some(unloaded) = change
                && let Some((left, right)) = self.last_trigger
            {
                let baseline = if side == 0 { left } else { right };
                let effect = if unloaded { TRIGGER_OFF } else { baseline };
                self.write(protocol::usb_report(
                    if side == 0 { 0x08 } else { 0x04 },
                    0,
                    0,
                    (side == 1).then_some(effect),
                    (side == 0).then_some(effect),
                    None,
                    None,
                    false,
                ))?;
            }
        }
        Ok(())
    }
}

fn emit(proxy: &Sender<serde_json::Value>, value: serde_json::Value) {
    let _ = proxy.send(value);
}

fn is_controller(info: &hidapi::DeviceInfo) -> bool {
    info.vendor_id() == protocol::SONY_VID
        && matches!(
            info.product_id(),
            protocol::DUALSENSE_PID | protocol::EDGE_PID
        )
        && info.usage_page() == 1
        && info.usage() == 5
}

fn path_key(info: &hidapi::DeviceInfo) -> String {
    info.path().to_string_lossy().into_owned()
}

pub(crate) fn controller_identity(info: &hidapi::DeviceInfo, hid: &HidDevice) -> String {
    identify_controller(info, hid).0
}
fn identify_controller(info: &hidapi::DeviceInfo, hid: &HidDevice) -> (String, Option<String>) {
    let serial = info
        .serial_number()
        .unwrap_or("")
        .replace([':', '-'], "")
        .to_ascii_lowercase();
    let mut pairing = [0u8; 20];
    pairing[0] = 0x09;
    let address = if serial.len() == 12 && serial.bytes().all(|b| b.is_ascii_hexdigit()) {
        serial
    } else if hid
        .get_feature_report(&mut pairing)
        .is_ok_and(|length| length >= 7)
        && pairing[1..7].iter().any(|byte| *byte != 0)
    {
        pairing[1..7]
            .iter()
            .rev()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    } else {
        format!("path:{}", path_key(info))
    };
    (
        hash_identity(info.product_id(), &address),
        crate::native_provider::normalize_address(&address).ok(),
    )
}

/// Opaque stable local identifier; never include the hardware address in shareable diagnostics.
fn hash_identity(product_id: u16, address: &str) -> String {
    let identity = format!("{product_id:04x}:{address}");
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("pad-{hash:016x}")
}

/// Controller id derivable from the enumeration record alone (no open). Only
/// available when the HID serial is the 12-digit Bluetooth address.
fn serial_controller_id(info: &hidapi::DeviceInfo) -> Option<String> {
    let serial = info
        .serial_number()
        .unwrap_or("")
        .replace([':', '-'], "")
        .to_ascii_lowercase();
    (serial.len() == 12 && serial.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hash_identity(info.product_id(), &serial))
}

/// Which controller the user wants. The id is authoritative; the path is only a hint
/// because HID paths change between USB and Bluetooth and across reconnects.
#[derive(Clone, Debug, Default)]
struct Preference {
    path: Option<String>,
    controller_id: Option<String>,
}

impl Preference {
    fn pin(&mut self, found: &Connected) {
        self.path = Some(found.path_display.clone());
        self.controller_id = Some(found.controller_id.clone());
    }
}

/// Order candidates: same controller id, then same path, then USB before Bluetooth.
fn candidate_rank(pref: &Preference, id: Option<&str>, path: &str, usb: bool) -> (bool, bool, u8) {
    let id_match = pref.controller_id.as_deref().is_some_and(|w| id == Some(w));
    let path_match = pref.path.as_deref().is_some_and(|w| path == w);
    (!id_match, !path_match, u8::from(!usb))
}

static DEVICE_CACHE: std::sync::Mutex<Vec<serde_json::Value>> = std::sync::Mutex::new(Vec::new());
/// (hid path, battery percent, charging) for the open controller, used in list entries.
static LIVE_BATTERY: std::sync::Mutex<Option<(String, Option<u8>, bool)>> =
    std::sync::Mutex::new(None);

fn list_devices(api: &HidApi) -> Vec<serde_json::Value> {
    let live = LIVE_BATTERY.lock().ok().and_then(|live| live.clone());
    let mut items: Vec<_> = api
        .device_list()
        .filter(|info| is_controller(info))
        .map(|info| {
            let path = path_key(info);
            let controller_id = serial_controller_id(info);
            let assigned = controller_id
                .as_deref()
                .and_then(|id| crate::hardware::assignment(id).ok().flatten())
                .map(|record| record.name);
            let battery = live.as_ref().filter(|(p, _, _)| *p == path);
            json!({
                "controllerId": controller_id,
                "assignedName": assigned,
                "battery": battery.and_then(|(_, level, _)| *level),
                "charging": battery.map(|(_, _, charging)| *charging),
                "path": path,
                "name": info.product_string().unwrap_or("DualSense"),
                "transport": if matches!(info.bus_type(), BusType::Bluetooth) { "Bluetooth" } else { "USB" },
                "productId": format!("{:04x}", info.product_id()),
                "isEdge": info.product_id() == protocol::EDGE_PID,
            })
        })
        .collect();
    items.sort_by_key(|item| if item["transport"] == "USB" { 0 } else { 1 });
    items
}

fn emit_device_list(api: &HidApi, proxy: &Sender<serde_json::Value>, selected: Option<&str>) {
    let items = list_devices(api);
    if let Ok(mut cache) = DEVICE_CACHE.lock() {
        *cache = items.clone();
    }
    emit(
        proxy,
        json!({
            "type": "devices",
            "items": items,
            "selected": selected,
            "stale": false,
        }),
    );
}

fn emit_device_list_if_changed(
    api: &HidApi,
    proxy: &Sender<serde_json::Value>,
    selected: Option<&str>,
) {
    let items = list_devices(api);
    let changed = DEVICE_CACHE
        .lock()
        .map(|mut cache| {
            let changed = *cache != items;
            if changed {
                *cache = items.clone();
            }
            changed
        })
        .unwrap_or(true);
    if changed {
        emit(
            proxy,
            json!({"type":"devices","items":items,"selected":selected,"stale":false}),
        );
    }
}

/// Fault names for the battery event (empty when the charge state is healthy).
fn battery_faults(state: protocol::ChargeState) -> Vec<&'static str> {
    match state {
        protocol::ChargeState::VoltageOrTempFault => vec!["VoltageOrTempFault"],
        protocol::ChargeState::TemperatureFault => vec!["TemperatureFault"],
        protocol::ChargeState::ChargingError => vec!["ChargingError"],
        _ => Vec::new(),
    }
}

/// `0x0215` -> "2.21".
fn feature_version_label(feature: u16) -> String {
    format!("{}.{:02}", feature >> 8, feature & 0xff)
}

/// Build date (bytes 1..12) and time (12..20) of feature report 0x20, when printable.
fn firmware_build(report: &[u8; 64]) -> Option<String> {
    let text = |range: std::ops::Range<usize>| {
        let raw = &report[range];
        let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
        let value = std::str::from_utf8(&raw[..end]).ok()?.trim().to_string();
        (!value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic() || b == b' '))
            .then_some(value)
    };
    let date = text(1..12)?;
    Some(match text(12..20) {
        Some(time) => format!("{date} {time}"),
        None => date,
    })
}

/// True while a background Edge profile read is using the controller.
fn edge_busy() -> bool {
    crate::hardware::EDGE_READING.load(std::sync::atomic::Ordering::Acquire)
}

/// Cached list for moments when enumeration must not touch HID (native handoff, Edge job).
fn emit_cached_device_list(proxy: &Sender<serde_json::Value>, selected: Option<&str>) {
    let items = DEVICE_CACHE
        .lock()
        .map(|cache| cache.clone())
        .unwrap_or_default();
    emit(
        proxy,
        json!({"type":"devices","items":items,"selected":selected,"stale":true}),
    );
}

/// (sorted enumerated controller paths, path -> controller id of pads rejected by a strict probe).
type StrictRejected = (Vec<String>, std::collections::BTreeMap<String, String>);
static STRICT_REJECTED: std::sync::Mutex<StrictRejected> =
    std::sync::Mutex::new((Vec::new(), std::collections::BTreeMap::new()));

fn open_path(api: &HidApi, wanted: &Preference) -> Result<Option<Connected>, String> {
    open_filtered(api, wanted, false, false)
}

/// `usb_only` limits candidates to USB; `strict_id` additionally requires the preferred
/// controller id (no fallback to other pads).
fn open_filtered(
    api: &HidApi,
    wanted: &Preference,
    usb_only: bool,
    strict_id: bool,
) -> Result<Option<Connected>, String> {
    let mut candidates: Vec<_> = api
        .device_list()
        .filter(|info| is_controller(info))
        .filter(|info| !usb_only || matches!(info.bus_type(), BusType::Usb))
        .collect();
    let want_id = wanted.controller_id.as_deref();
    if strict_id {
        let Some(want) = want_id else {
            return Ok(None);
        };
        // A serial-derived id that differs is certainly another pad; unknown ids are verified after open.
        candidates.retain(|info| serial_controller_id(info).is_none_or(|id| id == want));
        // Paths already probed and found to belong to another pad are skipped until the
        // set of enumerated controllers changes.
        let mut signature: Vec<String> = api
            .device_list()
            .filter(|info| is_controller(info))
            .map(path_key)
            .collect();
        signature.sort();
        if let Ok(mut cache) = STRICT_REJECTED.lock() {
            if cache.0 != signature {
                *cache = (signature, std::collections::BTreeMap::new());
            }
            candidates.retain(|info| {
                cache
                    .1
                    .get(&path_key(info))
                    .is_none_or(|rejected_id| rejected_id == want)
            });
        }
    }
    candidates.sort_by_key(|info| {
        candidate_rank(
            wanted,
            serial_controller_id(info).as_deref(),
            &path_key(info),
            matches!(info.bus_type(), BusType::Usb),
        )
    });
    let mut last_error = None;
    let mut fallback: Option<Connected> = None;
    for info in candidates {
        match info.open_device(api) {
            Ok(hid) => {
                let mut firmware_report = [0u8; 64];
                firmware_report[0] = 0x20;
                let mut build = None;
                let (firmware, feature_version) = match hid.get_feature_report(&mut firmware_report)
                {
                    Ok(length) if length >= 46 => {
                        let hw = u32::from_le_bytes(firmware_report[24..28].try_into().unwrap());
                        let fw = u32::from_le_bytes(firmware_report[28..32].try_into().unwrap());
                        let feature =
                            u16::from_le_bytes(firmware_report[44..46].try_into().unwrap());
                        build = firmware_build(&firmware_report);
                        (
                            Some(format!("FW {fw:08X} / HW {hw:08X} / FEATURE {feature:04X}")),
                            Some(feature),
                        )
                    }
                    _ => (None, None),
                };
                let mut feature = [0u8; 41];
                feature[0] = 5;
                let factory = match hid.get_feature_report(&mut feature) {
                    Ok(length) => FactoryCalibration::parse(
                        &feature[..length.min(feature.len())],
                        matches!(info.bus_type(), BusType::Bluetooth),
                    )
                    .unwrap_or_else(FactoryCalibration::fallback),
                    Err(_) => FactoryCalibration::default(),
                };
                let mut dongle = DongleProbe::default();
                if !matches!(info.bus_type(), BusType::Bluetooth) {
                    let mut version = [0u8; 64];
                    version[0] = 0xf8;
                    if let Ok(n) = hid.get_feature_report(&mut version) {
                        let mut config = [0u8; 64];
                        config[0] = 0xf7;
                        if version.get(1).is_some_and(|b| b.is_ascii_alphanumeric())
                            && let Ok(m) = hid.get_feature_report(&mut config)
                        {
                            dongle = crate::integrations::dongle_probe(
                                &version[..n.min(64)],
                                &config[..m.min(64)],
                            );
                        }
                    }
                }
                let display = path_key(info);
                let (controller_id, bluetooth_address) = identify_controller(info, &hid);
                let saved_calibration = crate::output_calibration::load(&controller_id);
                let output_calibration_error = saved_calibration.as_ref().err().cloned();
                let output_calibration = saved_calibration.ok().flatten();
                let calibration = output_calibration
                    .as_ref()
                    .map(|r| r.settings.calibration.clone())
                    .unwrap_or_default();
                let mut found = Connected {
                    hid,
                    path: info.path().to_bytes().to_vec(),
                    controller_id,
                    path_display: display,
                    name: info.product_string().unwrap_or("DualSense").to_string(),
                    transport: info.bus_type(),
                    product_id: info.product_id(),
                    rumble_flags: protocol::rumble_flags(
                        feature_version,
                        info.product_id() == protocol::EDGE_PID,
                    ),
                    sequence: 0,
                    input_sequence: None,
                    write_timing: crate::runtime_diagnostics::Rolling::default(),
                    trigger_active: false,
                    light_active: false,
                    player_leds_active: false,
                    assignment_error: None,
                    rumble_active: false,
                    trigger_until: None,
                    rumble_until: None,
                    haptic: None,
                    audio: AudioEngine::default(),
                    speaker_route_until: None,
                    speaker_track_id: 0,
                    microphone_muted: None,
                    desktop: DesktopMapper::default(),
                    calibration,
                    calibration_measuring: false,
                    output_calibration,
                    output_calibration_error,
                    input_stale: false,
                    controls: ControlProfile::default(),
                    motion: MotionProcessor::with_factory(factory),
                    radial: RadialMapper::default(),
                    last_motion_event: Instant::now(),
                    dongle,
                    link_experiment: None,
                    bt_audio_sequence: 0,
                    bluetooth_address,
                    last_input_sent: Instant::now() - Duration::from_secs(1),
                    last_input_buttons: [false; 23],
                    last_valid_report: Instant::now(),
                    opened_at: Instant::now(),
                    panic_latched: false,
                    firmware,
                    firmware_version: feature_version.map(feature_version_label),
                    legacy_rumble: info.product_id() != protocol::EDGE_PID
                        && feature_version.is_some_and(|v| v < 0x0215),
                    firmware_build: build,
                    last_trigger: None,
                    game_actions: ActionMapper::default(),
                    action_until: None,
                    action_id: None,
                    timeline: None,
                    timeline_generation: 0,
                    personal: PersonalSettings::default(),
                    last_light: None,
                    user_light: None,
                    anim: None,
                    battery_light: false,
                    sensor_light: false,
                    last_sensor: None,
                    screen_light: false,
                    screen_color: None,
                    last_rumble: None,
                    game_rumble: [0, 0],
                    game_feedback_at: None,
                    game_audio_retry_at: None,
                    input_effect: None,
                    input_effect_until: None,
                    bows: [None, None],
                    low_battery_alert: false,
                    user_player_leds: None,
                    mic_user_set: false,
                    pending_events: Vec::new(),
                    game_refresh_at: None,
                    low_battery_latched: false,
                    low_battery_critical_latched: false,
                    battery_thresholds: (20, 5),
                    battery_key: None,
                    battery_unknown_sent: false,
                    jacks: None,
                    last_battery: None,
                    haptic_timer: None,
                    led_brightness_dirty: (false, false),
                };
                if strict_id && want_id != Some(found.controller_id.as_str()) {
                    if let Ok(mut cache) = STRICT_REJECTED.lock() {
                        cache
                            .1
                            .insert(found.path_display.clone(), found.controller_id.clone());
                    }
                    continue;
                }
                // Candidates with an unknown serial are ranked blindly; keep looking for the
                // pad whose opened id really matches and only fall back to the first other pad.
                if let Some(want) = want_id
                    && found.controller_id != want
                {
                    if fallback.is_none() {
                        fallback = Some(found);
                    }
                    continue;
                }
                let _ = found.load_assignment();
                return Ok(Some(found));
            }
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    if let Some(mut found) = fallback {
        let _ = found.load_assignment();
        return Ok(Some(found));
    }
    if let Some(error) = last_error {
        Err(format!("Controller found, but could not open it: {error}"))
    } else {
        Ok(None)
    }
}

/// Single teardown path for a controller that is being released: remembers its settings,
/// resets the hardware and flushes events the reset produced.
fn teardown(old: &mut Connected, proxy: &Sender<serde_json::Value>) -> Result<(), String> {
    old.remember();
    let result = old.reset();
    for event in old.pending_events.drain(..) {
        emit(proxy, event);
    }
    result
}

#[allow(clippy::too_many_arguments)] // Disconnect cleanup coordinates every active input and color worker.
fn drop_device(
    device: &mut Option<Connected>,
    mouse: &mut MouseMapper,
    gyro: &mut GyroAimMapper,
    remap: &mut RemapMapper,
    xinput: &mut XInputBridge,
    screen: &ScreenSampler,
    proxy: &Sender<serde_json::Value>,
    message: &str,
) {
    screen.set_enabled(false);
    stop_input_mappers(mouse, gyro, remap, xinput, proxy);
    emit(proxy, json!({"type":"lightMode","battery":false}));
    emit(
        proxy,
        json!({"type":"triggerLease","active":false,"durationMs":0}),
    );
    if let Some(mut old) = device.take() {
        let _ = teardown(&mut old, proxy);
    }
    if let Ok(mut live) = LIVE_BATTERY.lock() {
        *live = None;
    }
    let (state, reason) = disconnect_state(message);
    emit(
        proxy,
        json!({"type":"device","connected":false,"state":state,"reason":reason,"message":message}),
    );
}

/// Map a human disconnect message to the structured (state, reason) pair.
fn disconnect_state(message: &str) -> (&'static str, &'static str) {
    if message.starts_with("Stream stalled") {
        ("stalled", "stream-stalled")
    } else if message.starts_with("Controller disconnected") {
        ("reconnecting", "read-error")
    } else if message.starts_with("Disconnected") {
        ("disconnected", "removed")
    } else {
        ("disconnected", "other")
    }
}

fn stop_input_mappers(
    mouse: &mut MouseMapper,
    gyro: &mut GyroAimMapper,
    remap: &mut RemapMapper,
    xinput: &mut XInputBridge,
    proxy: &Sender<serde_json::Value>,
) {
    mouse.set_enabled(false);
    mouse.set_gestures(false);
    gyro.set_config(GyroAimConfig::default());
    remap.set(false, Vec::new());
    xinput.shutdown();
    // Key-ups first, while the input backend still exists.
    #[cfg(not(windows))]
    crate::native_input::release_all();
    emit(proxy, json!({"type":"mouseMode","enabled":false}));
    emit(proxy, json!({"type":"touchGestures","enabled":false}));
    emit(proxy, json!({"type":"gyroAim","enabled":false}));
    emit(proxy, json!({"type":"remap","enabled":false}));
    emit(proxy, xinput.status_event());
}

fn emit_connected(proxy: &Sender<serde_json::Value>, found: &Connected) {
    let assignment = crate::hardware::assignment(&found.controller_id)
        .ok()
        .flatten();
    emit(
        proxy,
        json!({
            "type": "device",
            "connected": true,
            "state": "connected",
            "reason": serde_json::Value::Null,
            "firmwareVersion": found.firmware_version,
            "firmwareBuild": found.firmware_build,
            "legacyRumble": found.legacy_rumble,
            "battery": found.last_battery,
            "name": assignment.as_ref().map(|r|r.name.as_str()).unwrap_or(&found.name),
            "hardwareName":found.name,"assignmentError":found.assignment_error,
            "assignment":assignment,"audioEndpointId":found.audio.configured_endpoint(),
            "transport": if matches!(found.transport, BusType::Bluetooth) { "Bluetooth" } else { "USB" },
            "firmware": found.firmware,
            "path": found.path_display,
            "controllerId": found.controller_id,
            "outputCalibration": {"record":found.output_calibration,"error":found.output_calibration_error},
            "dongle":found.dongle,
            "motionFactory":{"valid":found.motion.factory.valid,"message":found.motion.factory.message},
            "productId": format!("{:04x}", found.product_id),
            "isEdge": found.product_id == protocol::EDGE_PID,
            "bluetoothAudio": found.audio.bluetooth_enabled(),
        }),
    );
}

#[allow(clippy::too_many_arguments)] // Core command dispatch shares device + mapper state.
fn handle_command(
    command: DeviceCommand,
    device: &mut Option<Connected>,
    proxy: &Sender<serde_json::Value>,
    mouse: &mut MouseMapper,
    gyro: &mut GyroAimMapper,
    remap: &mut RemapMapper,
    xinput: &mut XInputBridge,
    telemetry: &mut TelemetryBridge,
    game_link: &mut GameLink,
    script: &mut Option<ScriptProgram>,
    screen: &ScreenSampler,
    telemetry_stale_latched: &mut bool,
    api: &mut HidApi,
    preferred: &mut Preference,
) -> bool {
    if device.as_ref().is_some_and(|d| d.link_experiment.is_some())
        && !matches!(
            command,
            DeviceCommand::StopLinkExperiment
                | DeviceCommand::Reset
                | DeviceCommand::Shutdown
                | DeviceCommand::SetUiFocused { .. }
                | DeviceCommand::Scan
                | DeviceCommand::ListDevices
                | DeviceCommand::SelectDevice { .. }
                | DeviceCommand::CheckDesktopInput
                | DeviceCommand::ProfileCheckpoint { .. }
        )
    {
        emit(
            proxy,
            json!({"type":"error","message":"Stop the link experiment before changing output settings"}),
        );
        return true;
    }
    match command {
        DeviceCommand::SetWeaponOverride { weapon } => {
            match game_link.manual_weapon(weapon) {
                Ok(status) => emit(proxy, json!({"type":"weaponRecognition","data":status})),
                Err(message) => emit(proxy, json!({"type":"error","message":message})),
            };
            return true;
        }
        DeviceCommand::CheckDesktopInput => {
            stop_input_mappers(mouse, gyro, remap, xinput, proxy);
            if let Some(d) = device.as_mut() {
                let _ = d.reset();
            }
            emit(proxy, json!({"type":"safetyReset"}));
            let result = crate::input_map::prepare_desktop();
            emit(
                proxy,
                json!({"type":"desktopInputStatus","ready":result.is_ok(),"message":result.err().unwrap_or_else(||"Desktop backend opened. No input was sent.".into()),"environment":crate::integrations::environment(),"pointerPrecision":crate::input_map::pointer_precision_enabled()}),
            );
            return true;
        }
        DeviceCommand::ProfileCheckpoint { request_id } => {
            // FIFO marker: all preceding commands and their error events have
            // completed before this acknowledgement reaches the UI.
            emit(
                proxy,
                json!({"type":"profileCheckpoint","requestId":request_id,
                "connected":device.is_some(),"controllerId":device.as_ref().map(|d|&d.controller_id)}),
            );
            return true;
        }
        DeviceCommand::SetGameRecording { enabled } => {
            game_link.record(enabled);
            return true;
        }
        DeviceCommand::SetGameLink { settings } => {
            match game_link.set(settings.clone()) {
                Ok(()) => emit(
                    proxy,
                    json!({"type":"gameLinkStatus","enabled":settings.enabled,"message":if settings.enabled {format!("Listening on 127.0.0.1:{} · {}",settings.port,settings.format)}else{"Game link off".into()}}),
                ),
                Err(message) => emit(proxy, json!({"type":"error","message":message})),
            }
            return true;
        }
        DeviceCommand::SetGameActions { enabled, rules } => {
            if let Some(d) = device.as_mut() {
                let mut mapper = ActionMapper::default();
                if let Err(message) = mapper.set(if enabled { rules } else { Vec::new() }) {
                    emit(proxy, json!({"type":"error","message":message}));
                } else {
                    d.stop_action();
                    d.game_actions = mapper;
                    emit(proxy, json!({"type":"gameActionsStatus","enabled":enabled}));
                }
            }
            return true;
        }
        DeviceCommand::Shutdown => {
            stop_input_mappers(mouse, gyro, remap, xinput, proxy);
            if let Some(device) = device.as_mut() {
                let _ = device.reset();
            }
            return false;
        }
        DeviceCommand::SetScript { enabled, source } => {
            if !enabled {
                *script = None;
                emit(
                    proxy,
                    json!({"type":"scriptStatus","enabled":false,"message":"Script off"}),
                );
            } else {
                match ScriptProgram::compile(&source) {
                    Ok(program) => {
                        *script = Some(program);
                        emit(
                            proxy,
                            json!({"type":"scriptStatus","enabled":true,"message":"Script armed · waiting for telemetry"}),
                        );
                    }
                    Err(error) => {
                        emit(
                            proxy,
                            json!({"type":"scriptStatus","enabled":script.is_some(),"message":error}),
                        );
                        emit(proxy, json!({"type":"error","message":error}));
                    }
                }
            }
            return true;
        }
        DeviceCommand::Scan | DeviceCommand::ListDevices => {
            // Refresh so a Scan never answers from a stale enumeration.
            let _ = api.refresh_devices();
            emit_device_list(
                api,
                proxy,
                preferred
                    .path
                    .as_deref()
                    .or(device.as_ref().map(|d| d.path_display.as_str())),
            );
            if let Some(found) = device.as_ref() {
                emit_connected(proxy, found);
            }
            return true;
        }
        DeviceCommand::SelectDevice { path } => {
            if device.as_ref().is_some_and(|d| d.path_display == path) {
                emit(proxy, json!({"type":"notice","message":"Already selected"}));
                return true;
            }
            stop_input_mappers(mouse, gyro, remap, xinput, proxy);
            screen.set_enabled(false);
            if let Some(mut old) = device.take() {
                let _ = teardown(&mut old, proxy);
            }
            if let Ok(mut live) = LIVE_BATTERY.lock() {
                *live = None;
            }
            emit(proxy, json!({"type":"lightMode","battery":false}));
            emit(
                proxy,
                json!({"type":"triggerLease","active":false,"durationMs":0}),
            );
            let _ = api.refresh_devices();
            // Only pin once the open succeeded, so a failed selection never leaves a bad path behind.
            let request = Preference {
                path: Some(path.clone()),
                controller_id: None,
            };
            match open_filtered(api, &request, false, false) {
                Ok(Some(mut found)) if found.path_display == path => {
                    preferred.pin(&found);
                    emit_connected(proxy, &found);
                    found.restore_remembered(proxy);
                    emit_device_list(api, proxy, Some(&found.path_display));
                    *device = Some(found);
                    emit(
                        proxy,
                        json!({"type":"notice","message":"Controller selected"}),
                    );
                }
                Ok(_) => {
                    // The chosen pad vanished; do not silently substitute another one.
                    *preferred = Preference::default();
                    emit(
                        proxy,
                        json!({"type":"device","connected":false,"state":"disconnected","reason":"selection-not-found","message":"Controller not found"}),
                    );
                    emit(
                        proxy,
                        json!({"type":"error","code":"selection-not-found","message":"The selected controller is no longer available"}),
                    );
                }
                Err(error) => {
                    *preferred = Preference::default();
                    emit(
                        proxy,
                        json!({"type":"error","code":"open-failed","message":error}),
                    );
                }
            }
            return true;
        }
        DeviceCommand::SetTouchMouse {
            enabled,
            sensitivity,
            acceleration,
        } => {
            if sensitivity.is_some_and(|v| !v.is_finite() || !(0.2..=3.0).contains(&v))
                || acceleration.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
            {
                emit(
                    proxy,
                    json!({"type":"error","message":"Touchpad pointer settings are out of range."}),
                );
                return true;
            }
            if enabled && let Err(message) = crate::input_map::prepare_desktop() {
                emit(proxy, json!({"type":"error","message":message}));
                return true;
            }
            if enabled && device.is_none() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Connect a controller first."}),
                );
                return true;
            }
            if enabled
                && device
                    .as_ref()
                    .is_some_and(|connected| connected.desktop.enabled())
            {
                emit(
                    proxy,
                    json!({"type":"error","message":"PC control already owns the touchpad pointer. Turn off PC control before using the separate touchpad mouse mode."}),
                );
                return true;
            }
            // Apply pointer settings only once the request is known to be acceptable.
            if sensitivity.is_some() || acceleration.is_some() {
                let (current_s, current_a) = mouse.pointer();
                mouse.set_pointer(
                    sensitivity.unwrap_or(current_s),
                    acceleration.unwrap_or(current_a),
                );
            }
            mouse.set_enabled(enabled);
            emit(proxy, json!({"type":"mouseMode","enabled":enabled}));
            emit(
                proxy,
                json!({"type":"notice","message":if enabled {"Touchpad mouse on"} else {"Touchpad mouse off"}}),
            );
            return true;
        }
        DeviceCommand::SetUiFocused { focused } => {
            if let Some(device) = device.as_mut() {
                device.desktop.set_ui_focused(focused);
                device.radial.focus(focused);
                remap.set_suspended(focused || device.desktop.enabled());
            } else {
                remap.set_suspended(focused);
            }
            return true;
        }
        DeviceCommand::SetPcMode { enabled } => {
            if enabled && let Err(message) = crate::input_map::prepare_desktop() {
                emit(proxy, json!({"type":"error","message":message}));
                return true;
            }
            let Some(device) = device.as_mut() else {
                emit(
                    proxy,
                    json!({"type":"error","message":"Connect a controller before enabling PC control."}),
                );
                return true;
            };
            if enabled && mouse.enabled() {
                mouse.set_enabled(false);
                emit(proxy, json!({"type":"mouseMode","enabled":false}));
            }
            device.desktop.set_enabled(enabled);
            remap.set_suspended(enabled || device.desktop.ui_focused());
            emit(proxy, json!({"type":"pcMode","enabled":enabled}));
            emit(
                proxy,
                json!({"type":"notice","message":if enabled {"PC control on"} else {"Game mode on"}}),
            );
            return true;
        }
        DeviceCommand::StopMacros => {
            if let Some(d) = device.as_mut() {
                d.desktop.stop_macros();
            }
            remap.set_suspended(true);
            emit(
                proxy,
                json!({"type":"notice","message":"Macros stopped. Release the controls before starting again."}),
            );
            return true;
        }
        DeviceCommand::SetPcBindings {
            bindings,
            click_threshold,
            pointer_sensitivity,
            reverse_scroll,
            pointer_acceleration,
            left_stick,
        } => {
            let Some(device) = device.as_mut() else {
                return true;
            };
            if !pointer_acceleration.is_finite() || !(0.0..=1.0).contains(&pointer_acceleration) {
                emit(
                    proxy,
                    json!({"type":"error","message":"Pointer acceleration must be between 0 and 1."}),
                );
                return true;
            }
            let mut seen = std::collections::HashSet::new();
            if !click_threshold.is_finite()
                || !(0.05..=0.95).contains(&click_threshold)
                || !pointer_sensitivity.is_finite()
                || !(0.4..=1.5).contains(&pointer_sensitivity)
                || bindings.len() > 21
                || bindings.iter().any(|b| {
                    b.button >= 23
                        || matches!(b.button, 6 | 7)
                        || b.modifier.is_some()
                        || (!matches!(b.behavior.as_str(), "launch" | "program")
                            && (b.vk == 0
                                || !crate::input_map::supported_key(b.vk)
                                || RemapBehavior::from_label(&b.behavior).is_none()
                                || b.launch_path.is_some()
                                || b.program.is_some()
                                || !b.launch_args.is_empty()))
                        || !crate::macros::valid_args(&b.launch_args)
                        || (b.behavior != "launch" && !b.launch_args.is_empty())
                        || (b.behavior == "program"
                            && (b.vk != 0
                                || b.launch_path.is_some()
                                || b.program.as_ref().is_none_or(|p| p.validate().is_err())
                                || b.program.as_ref().is_some_and(|p| {
                                    p.steps.iter().any(|s| match s {
                                        crate::macros::Step::Chord { keys, .. } => keys
                                            .iter()
                                            .any(|k| !crate::input_map::supported_key(*k)),
                                        crate::macros::Step::Launch { path, .. } => {
                                            !valid_launch_path(path)
                                        }
                                        _ => false,
                                    })
                                })))
                        || (b.behavior != "program" && b.program.is_some())
                        || (b.behavior == "launch"
                            && (b.vk != 0
                                || b.launch_path
                                    .as_deref()
                                    .is_none_or(|p| !valid_launch_path(p))))
                        || !macro_fields_valid(b)
                        || !seen.insert(b.identity())
                        || (matches!(b.behavior.as_str(), "launch" | "program")
                            && bindings.iter().filter(|o| o.button == b.button).count() > 1)
                })
            {
                emit(
                    proxy,
                    json!({"type":"error","message":"PC shortcuts need distinct buttons and valid key actions."}),
                );
                return true;
            }
            let launches = bindings
                .iter()
                .filter(|b| b.behavior == "launch")
                .map(|b| PcLaunchBinding {
                    button: b.button,
                    path: std::path::PathBuf::from(b.launch_path.as_deref().unwrap()),
                    args: b.launch_args.clone(),
                })
                .collect();
            let programs = bindings
                .iter()
                .filter_map(|b| b.program.clone().map(|p| (b.button, p)))
                .collect();
            let modes = bindings
                .iter()
                .filter(|b| b.behavior == "program")
                .map(|b| (b.button, b.program_mode.unwrap_or_default()))
                .collect();
            let mapped = bindings
                .into_iter()
                .filter(|b| !matches!(b.behavior.as_str(), "launch" | "program"))
                .map(|b| b.into_binding(None))
                .collect::<Vec<_>>();
            if let Err(message) = RemapBinding::validate_set(&mapped) {
                emit(proxy, json!({"type":"error","message":message}));
                return true;
            }
            // Only touch the live mapper once the whole set is known to be valid.
            device.desktop.set_programs(programs);
            device.desktop.set_program_modes(modes);
            device.desktop.set_bindings(
                mapped,
                click_threshold,
                pointer_sensitivity,
                reverse_scroll,
                launches,
            );
            device
                .desktop
                .set_pointer_acceleration(pointer_acceleration);
            device.desktop.set_left_stick_mode(left_stick);
            emit(
                proxy,
                json!({"type":"pcBindings","count":seen.len(),"clickThreshold":click_threshold}),
            );
            return true;
        }
        DeviceCommand::ReloadOutputCalibration { controller_id } => {
            if let Some(connected) = device.as_mut() {
                if connected.controller_id != controller_id {
                    emit(
                        proxy,
                        json!({"type":"error","message":"Controller changed; calibration was not applied."}),
                    );
                } else {
                    let loaded = crate::output_calibration::load(&controller_id);
                    connected.output_calibration_error = loaded.as_ref().err().cloned();
                    connected.output_calibration = loaded.ok().flatten();
                    if !connected.calibration_measuring {
                        connected.calibration = connected
                            .output_calibration
                            .as_ref()
                            .map(|r| r.settings.calibration.clone())
                            .unwrap_or_default();
                    }
                    emit(
                        proxy,
                        json!({"type":"outputCalibration","controllerId":controller_id,"record":connected.output_calibration,"error":connected.output_calibration_error}),
                    );
                }
            }
            return true;
        }
        DeviceCommand::SetCalibration {
            calibration,
            measuring,
        } => {
            if !calibration.valid() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Calibration ranges are incomplete or invalid."}),
                );
                return true;
            }
            if let Some(connected) = device.as_mut() {
                connected.calibration = calibration;
                connected.calibration_measuring = measuring;
                emit(proxy, json!({"type":"calibration","applied":true}));
            }
            return true;
        }
        DeviceCommand::SetTouchGestures { enabled } => {
            if enabled && (device.is_none() || !mouse.enabled()) {
                emit(
                    proxy,
                    json!({"type":"error","message":"Turn on touchpad mouse before enabling edge gestures."}),
                );
                return true;
            }
            mouse.set_gestures(enabled);
            emit(proxy, json!({"type":"touchGestures","enabled":enabled}));
            emit(
                proxy,
                json!({"type":"notice","message":if enabled {"Edge gestures on"} else {"Edge gestures off"}}),
            );
            return true;
        }
        DeviceCommand::SetGyroAim {
            enabled,
            sensitivity,
            yaw_scale,
            pitch_scale,
            invert_y,
            activation,
            bias,
            deadzone,
            smoothing,
            gate,
            stick_off_threshold,
            smooth_threshold,
            acceleration,
            accel_threshold,
            flick_without_gyro,
        } => {
            if enabled && let Err(message) = crate::input_map::prepare_desktop() {
                emit(proxy, json!({"type":"error","message":message}));
                return true;
            }
            if enabled && device.is_none() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Connect a controller first."}),
                );
                return true;
            }
            let Some(activation_mode) = GyroActivation::from_label(&activation) else {
                emit(
                    proxy,
                    json!({"type":"error","message":"Choose a valid gyro activation control."}),
                );
                return true;
            };
            let Some(gate_mode) = crate::input_map::GyroGate::from_label(&gate) else {
                emit(
                    proxy,
                    json!({"type":"error","message":"Gyro gate must be hold, disable or toggle."}),
                );
                return true;
            };
            if !(0.05..=8.0).contains(&sensitivity)
                || !stick_off_threshold.is_finite()
                || !(0.0..=1.0).contains(&stick_off_threshold)
                || !smooth_threshold.is_finite()
                || !(0.0..=1000.0).contains(&smooth_threshold)
                || !acceleration.is_finite()
                || !(0.0..=2.0).contains(&acceleration)
                || !accel_threshold.is_finite()
                || !(0.0..=1000.0).contains(&accel_threshold)
                || !(0.0..=4.0).contains(&yaw_scale)
                || !(0.0..=4.0).contains(&pitch_scale)
                || bias.iter().any(|n| i32::from(*n).abs() > 4096)
                || deadzone > 512
                || !smoothing.is_finite()
                || !(0.0..=0.85).contains(&smoothing)
            {
                emit(
                    proxy,
                    json!({"type":"error","message":"Gyro settings are out of range."}),
                );
                return true;
            }
            gyro.set_config(GyroAimConfig {
                enabled,
                sensitivity,
                yaw_scale,
                pitch_scale,
                invert_y,
                activation: activation_mode,
                deadzone,
                smoothing,
                bias,
                gate: gate_mode,
                stick_off_threshold,
                smooth_threshold,
                acceleration,
                accel_threshold,
                flick_without_gyro,
            });
            emit(
                proxy,
                json!({"type":"gyroAim","enabled":enabled,"sensitivity":sensitivity,"activation":activation,"invertY":invert_y}),
            );
            emit(
                proxy,
                json!({"type":"notice","message":if enabled {"Gyro aim on"} else {"Gyro aim off"}}),
            );
            return true;
        }
        DeviceCommand::SetRemap { enabled, bindings } => {
            if enabled && let Err(message) = crate::input_map::prepare_desktop() {
                emit(proxy, json!({"type":"error","message":message}));
                return true;
            }
            if enabled && device.is_none() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Connect a controller before enabling remap."}),
                );
                return true;
            }
            let mut seen = std::collections::HashSet::new();
            if bindings.len() > 46
                || bindings.iter().any(|b| {
                    b.button >= 23
                        || b.vk == 0
                        || !crate::input_map::supported_key(b.vk)
                        || b.modifier
                            .is_some_and(|modifier| modifier >= 23 || modifier == b.button)
                        || RemapBehavior::from_label(&b.behavior).is_none()
                        || b.launch_path.is_some()
                        || !macro_fields_valid(b)
                        || !seen.insert(b.identity())
                })
            {
                emit(
                    proxy,
                    json!({"type":"error","message":"Remap bindings need distinct button/layer pairs and valid key actions."}),
                );
                return true;
            }
            let mapped = bindings
                .into_iter()
                .map(|b| {
                    let modifier = b.modifier;
                    b.into_binding(modifier)
                })
                .collect::<Vec<_>>();
            if let Err(message) = RemapBinding::validate_set(&mapped) {
                emit(proxy, json!({"type":"error","message":message}));
                return true;
            }
            remap.set(enabled, mapped);
            emit(proxy, json!({"type":"remap","enabled":enabled}));
            emit(
                proxy,
                json!({"type":"notice","message":if enabled {"Remap on"} else {"Remap off"}}),
            );
            return true;
        }
        DeviceCommand::SetRemapTiming { hold_ms, gap_ms } => {
            if !(15..=500).contains(&hold_ms) || !(5..=500).contains(&gap_ms) {
                emit(
                    proxy,
                    json!({"type":"error","message":"Key hold must be 15-500 ms and gap 5-500 ms."}),
                );
                return true;
            }
            remap.set_timing(hold_ms, gap_ms);
            if let Some(d) = device.as_mut() {
                d.desktop.set_remap_timing(hold_ms, gap_ms);
            }
            emit(
                proxy,
                json!({"type":"remapTiming","holdMs":hold_ms,"gapMs":gap_ms}),
            );
            return true;
        }
        DeviceCommand::SetPadExtras { extras } => {
            xinput.set_extras(extras);
            emit(proxy, json!({"type":"padExtras","extras":extras}));
            return true;
        }
        DeviceCommand::PreviewEnvelope {
            path,
            kind,
            points,
            id,
        } => {
            if !crate::audio_haptics::file::valid_kind(&kind) || path.is_empty() {
                emit(
                    proxy,
                    json!({"type":"envelope","id":id,"path":path,"kind":kind,"error":"Choose a file and a valid content type."}),
                );
                return true;
            }
            let proxy = proxy.clone();
            std::thread::spawn(move || {
                let event = match crate::audio_haptics::file::preview_envelope(
                    std::path::Path::new(&path),
                    &kind,
                    points,
                ) {
                    Ok(values) => {
                        json!({"type":"envelope","id":id,"path":path,"kind":kind,"points":values})
                    }
                    Err(error) => {
                        json!({"type":"envelope","id":id,"path":path,"kind":kind,"error":error})
                    }
                };
                let _ = proxy.send(event);
            });
            return true;
        }
        DeviceCommand::SetXinput { enabled } => {
            if enabled && device.is_none() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Connect a controller first."}),
                );
                return true;
            }
            let event = xinput.set_enabled(enabled);
            let ok = event["status"] == "active" || event["status"] == "disabled";
            let message = event["message"].as_str().unwrap_or("XInput").to_string();
            emit(proxy, event);
            emit(
                proxy,
                json!({
                    "type": if ok { "notice" } else { "error" },
                    "message": message
                }),
            );
            if enabled && ok {
                let pointer_on = mouse.enabled() || gyro.config.enabled || remap.enabled();
                if pointer_on {
                    emit(
                        proxy,
                        json!({
                            "type": "notice",
                            "message": "XInput is on — hide the DualSense (e.g. HidHide) or turn off mouse/gyro/remap to avoid double input"
                        }),
                    );
                }
            }
            return true;
        }
        DeviceCommand::SetTelemetry {
            enabled,
            port,
            lan,
            format,
        } => {
            let was_enabled = telemetry.enabled();
            let configuration_changed =
                telemetry.port() != port || telemetry.lan() != lan || telemetry.format() != format;
            match telemetry.set_format(enabled, port, lan, &format) {
                Ok(()) => {
                    if enabled {
                        screen.set_enabled(false);
                        if let Some(connected) = device.as_mut() {
                            if connected.sensor_light
                                || connected.screen_light
                                || (was_enabled && configuration_changed)
                            {
                                let _ = connected.release_lightbar();
                            }
                            connected.anim = None;
                            connected.battery_light = false;
                            connected.sensor_light = false;
                            connected.last_sensor = None;
                            connected.screen_light = false;
                            connected.screen_color = None;
                        }
                        emit(proxy, json!({"type":"lightMode","battery":false}));
                        *telemetry_stale_latched = false;
                    } else if was_enabled && let Some(connected) = device.as_mut() {
                        let _ = connected.release_lightbar();
                    }
                    emit(
                        proxy,
                        json!({"type":"telemetryStatus","enabled":enabled,"port":port,"lan":lan,"format":telemetry.format(),"message":if enabled {let source=if format=="json" {"Local JSON"} else {"Forza Data Out"};if lan {format!("Listening for {source} on local network port {port}")} else {format!("Listening for {source} on 127.0.0.1:{port}")}} else {"Telemetry bridge off".into()}}),
                    );
                }
                Err(error) => {
                    if was_enabled
                        && !telemetry.enabled()
                        && let Some(connected) = device.as_mut()
                    {
                        let _ = connected.release_lightbar();
                    }
                    emit(
                        proxy,
                        json!({"type":"telemetryStatus","enabled":telemetry.enabled(),"message":error}),
                    );
                    emit(proxy, json!({"type":"error","message":error}));
                }
            }
            return true;
        }
        _ => {}
    }

    let edge_request = if let DeviceCommand::ReadEdgeProfiles {
        controller_id,
        request_id,
    } = &command
    {
        Some((controller_id.clone(), *request_id))
    } else {
        None
    };
    let Some(device) = device.as_mut() else {
        if let Some((id, request)) = &edge_request {
            emit(
                proxy,
                json!({"type":"edgeReadStatus","requestId":request,"controllerId":id,"error":"Connect the selected USB Edge first"}),
            );
            return true;
        }
        emit(
            proxy,
            json!({ "type": "notice", "message": "Connect a controller first." }),
        );
        return true;
    };

    let effect_request = match &command {
        DeviceCommand::PlayEffectClip { play_id, .. }
        | DeviceCommand::StopEffectClip { play_id } => Some(*play_id),
        _ => None,
    };
    let audio_request = match &command {
        DeviceCommand::PlayAudioFile { track_id, .. }
        | DeviceCommand::PlaySpeakerSound { track_id, .. }
        | DeviceCommand::SetSpeakerPaused { track_id, .. }
        | DeviceCommand::StopSpeakerSound { track_id } => Some(*track_id),
        _ => None,
    };
    let result: Result<&str, String> = match command {
        DeviceCommand::ReadEdgeProfiles{controller_id,request_id}=>{
            if device.controller_id!=controller_id||device.product_id!=protocol::EDGE_PID||!matches!(device.transport,BusType::Usb){Err("Select the same DualSense Edge over USB before reading profiles".into())}
            else{std::ffi::CString::new(device.path.clone()).map_err(|e|e.to_string()).and_then(|path|crate::hardware::start_edge_read(path,device.controller_id.clone(),device.firmware.clone(),request_id,proxy.clone())).map(|_|"Reading Edge profiles in the background")}
        }
        DeviceCommand::ApplyControllerAssignment{controller_id}=>{
            if controller_id!=device.controller_id{Err("The active controller changed; select it again".into())}
            else{(||{
                let record=crate::hardware::assignment(&controller_id)?;
                let endpoint=record.as_ref().and_then(|r|r.endpoint_id.as_deref());
                if device.audio.configured_endpoint()!=endpoint{stop_input_mappers(mouse,gyro,remap,xinput,proxy);game_link.stop();let _=telemetry.set(false,0,false);*script=None;screen.set_enabled(false);device.reset()?;emit(proxy,json!({"type":"safetyReset"}));}
                device.load_assignment()?;
                if record.as_ref().is_none_or(|r|r.player==0){device.write(protocol::player_leds_report(0))?;device.player_leds_active=false;}
                emit_connected(proxy,device);Ok("Controller assignment applied")
            })()}
        }
        DeviceCommand::SetBluetoothAudio{enabled,controller_id}=>{
            let result=(|| {
                if device.controller_id!=controller_id||!matches!(device.transport,BusType::Bluetooth){return Err("Select the same Bluetooth controller before changing grip audio.".to_string());}
                if device.audio.bluetooth_enabled(){let _=device.write_bt_audio(&[0;64]);}
                device.audio.set_bluetooth(enabled);device.haptic=None;device.rumble_until=None;device.game_rumble=[0,0];device.game_audio_retry_at=None;
                if enabled {if let Err(error)=device.enable_pcm(){device.audio.set_bluetooth(false);return Err(error);}}else{device.write_rumble([0,0])?;}
                Ok(())
            })();
            emit(proxy,json!({"type":"bluetoothAudioStatus","controllerId":controller_id,"enabled":device.audio.bluetooth_enabled(),"error":result.as_ref().err()}));
            result.map(|_|if enabled{"Bluetooth grip audio enabled; speaker requires USB."}else{"Bluetooth grip audio off."})
        }
        DeviceCommand::StartLinkExperiment{strength}=>{
            let run=LinkExperiment::new(Instant::now(),matches!(device.transport,BusType::Bluetooth),strength);
            run.and_then(|mut run|{
                stop_input_mappers(mouse,gyro,remap,xinput,proxy);
                game_link.stop();let _=telemetry.set(false,0,false);*script=None;screen.set_enabled(false);
                device.reset()?;emit(proxy,json!({"type":"safetyReset"}));
                let start=Instant::now();
                if run.bluetooth{device.write(protocol::audio_haptics_enable_report())?;}
                else{device.enable_pcm()?;device.audio.play_link_test(strength)?;}
                run.startup_us=start.elapsed().as_micros() as u64;run.callback_base=device.audio.callbacks();run.start=Instant::now();run.deadline=run.start+Duration::from_secs(2);
                emit(proxy,run.event(true));device.link_experiment=Some(run);Ok("Two-second link experiment started")
            })
        }
        DeviceCommand::StopLinkExperiment=>match device.stop_link_experiment(){
            Ok(event)=>{if let Some(event)=event{emit(proxy,event);}Ok("Link experiment stopped")},
            Err(error)=>{emit(proxy,json!({"type":"linkExperiment","running":false,"error":error}));Err(error)}
        },
        DeviceCommand::SetControlProfile {settings} => {
            let result=settings.validate().map_err(str::to_string).and_then(|_|{
                if settings.radial.enabled {crate::input_map::prepare_desktop()?;}
                device.radial.set(settings.radial.clone()).map_err(str::to_string)?;
                device.controls=settings;gyro.set_config(gyro.config.clone());
                emit(proxy,json!({"type":"controlProfile","applied":true,"settings":device.controls}));
                Ok("Input profile applied")
            });
            if let Err(message)=&result{emit(proxy,json!({"type":"controlProfile","applied":false,"message":message}));}
            result
        }
        DeviceCommand::SetAutoBias { enabled } => {
            device.motion.set_auto_bias(enabled);
            emit(proxy, json!({"type":"autoBias","enabled":device.motion.auto_bias()}));
            Ok(if enabled { "Automatic gyro bias on" } else { "Automatic gyro bias off" })
        }
        DeviceCommand::SetMotionBias {bias_dps} => device.motion.set_bias(bias_dps).map(|_|"Still bias restored").map_err(str::to_string),
        DeviceCommand::CalibrateMotion {enabled} => {
            device.motion.calibrate(enabled);
            emit(proxy,json!({"type":"motionCalibration","status":if enabled{"measuring"}else{"cancelled"}}));
            Ok(if enabled{"Waiting for two steady seconds"}else{"Still calibration cancelled"})
        }
        DeviceCommand::CancelRadial => {device.radial.cancel();Ok("Radial menu cancelled")}

        DeviceCommand::PlayPersonalTest {side,strength} => {
            if side>1 || strength>100 || !matches!(device.transport,BusType::Usb) {Err("Choose a grip and 0-100% strength over USB".into())}
            else { device.stop_action();let mut voice=VoiceClip::new("sine",90,10,180,strength,if side==0{-100}else{100});voice.raw_test=true;device.play_voice(voice).map(|_|"Raw grip test") }
        }
        DeviceCommand::PlayEffectClip {clip,play_id} => {
            if play_id<device.timeline_generation {Ok("Ignored stale timeline")} else {
                device.timeline_generation=play_id;
                device.start_timeline(clip).map(|_|"Timeline started")
            }
        }
        DeviceCommand::StopEffectClip {play_id} => {
            if play_id<device.timeline_generation {Ok("Ignored stale timeline stop")} else {
                device.timeline_generation=play_id;device.restore_action().map(|_|"Timeline stopped")
            }
        }
        DeviceCommand::SetPersonalHaptics {settings,request_id} => {
            let result=(||->Result<(),String>{
                settings.validate().map_err(str::to_string)?;
                device.audio.set_personal(settings.clone())?;device.personal=settings;
                if let Some(motors)=device.last_rumble.take(){device.write_rumble(motors)?;}
                if let Some(timeline)=device.timeline.as_mut(){timeline.previous=[None,None];device.tick_timeline()?;}
                else {
                    if device.action_until.is_some(){device.audio.cancel_cue();device.restore_action()?;}
                    if let Some((left,right))=device.last_trigger{device.write(protocol::usb_report(12,0,0,Some(right),Some(left),None,None,false))?;}
                }
                Ok(())
            })();
            emit(proxy,json!({"type":"personalHapticsStatus","requestId":request_id,"applied":result.is_ok(),"message":result.as_ref().err()}));
            result.map(|_|"Personal limits applied")
        }
        DeviceCommand::TestGameAction {rule} => {rule.validate().map_err(str::to_string).and_then(|()|device.play_action(&rule)).map(|()|"Action played")}
        DeviceCommand::ApplyTriggers { left, right } => {
            let _=device.restore_action();
            let bow_sides = [left.mode == "bow", right.mode == "bow"];
            let high_points = [&left,&right].map(|t| t.release_point.unwrap_or((f32::from(t.end_zone.unwrap_or(9))/10.0).clamp(0.2,0.97)));
            match (encode_live_trigger(&left), encode_live_trigger(&right)) {
                (Ok(left), Ok(right)) => {
                    let active = left[0] != 0x05 || right[0] != 0x05;
                    let changed = device.input_effect_until.is_some()
                        || device.last_trigger != Some((left, right));
                    let result = if changed {
                        device.write(protocol::usb_report(
                            0x0c,
                            0,
                            0,
                            Some(right),
                            Some(left),
                            None,
                            None,
                            false,
                        ))
                    } else {
                        Ok(())
                    };
                    result.map(|_| {
                        device.last_trigger = Some((left, right));
                        device.input_effect_until = None;
                        device.bows = std::array::from_fn(|i| bow_sides[i].then(|| BowReturn {high_point:high_points[i],..BowReturn::default()}));
                        if bow_sides[1] { device.input_effect = None; }
                        device.trigger_active = active;
                        device.trigger_until =
                            active.then(|| Instant::now() + Duration::from_secs(15));
                        emit(proxy, json!({"type":"triggerLease","active":active,"durationMs":if active {15000} else {0}}));
                        if active {
                            "Triggers applied (15s)"
                        } else {
                            "Triggers cleared"
                        }
                    })
                }
                (Err(error), _) | (_, Err(error)) => Err(error.into()),
            }
        }
        DeviceCommand::RenewTriggers => {
            if device.trigger_active && device.trigger_until.is_some() {
                device.trigger_until = Some(Instant::now() + Duration::from_secs(15));
                emit(
                    proxy,
                    json!({"type":"triggerLease","active":true,"durationMs":15000}),
                );
            }
            return true;
        }
        DeviceCommand::SetLightbar { rgb } => {
            screen.set_enabled(false);
            device.screen_light = false;
            device.screen_color = None;
            device.sensor_light = false;
            device.last_sensor = None;
            if telemetry.enabled() {
                let _ = telemetry.set(false, 0, false);
                emit(
                    proxy,
                    json!({"type":"telemetryStatus","enabled":false,"message":"Telemetry light released for manual color"}),
                );
            }
            device.anim = None;
            device.set_light(rgb).map(|_| {
                device.battery_light = false;
                device.user_light = Some(rgb);
                emit(proxy, json!({"type":"lightMode","battery":false}));
                "Lightbar set"
            })
        }
        DeviceCommand::SetPlayerLeds { mask } => {
            if mask > 0x1f {
                Err("Player indicator mask must use five lights".into())
            } else {
                device.write(protocol::player_leds_report(mask)).map(|_| {
                    device.player_leds_active = mask != 0;
                    device.user_player_leds = (mask != 0).then_some(mask);
                    "Player indicators set"
                })
            }
        }
        DeviceCommand::ReleaseLightbar => {
            screen.set_enabled(false);
            device.screen_light = false;
            device.screen_color = None;
            device.sensor_light = false;
            device.last_sensor = None;
            if telemetry.enabled() {
                let _ = telemetry.set(false, 0, false);
                emit(
                    proxy,
                    json!({"type":"telemetryStatus","enabled":false,"message":"Telemetry light released"}),
                );
            }
            device.battery_light = false;
            emit(proxy, json!({"type":"lightMode","battery":false}));
            device.release_lightbar().map(|_| "Lightbar released")
        }
        DeviceCommand::BatteryLightbar { enabled } => {
            if !enabled && !device.battery_light {
                emit(
                    proxy,
                    json!({"type":"lightMode","battery":false,"sensor":device.sensor_light,"screen":device.screen_light}),
                );
                return true;
            }
            screen.set_enabled(false);
            device.screen_light = false;
            device.screen_color = None;
            device.sensor_light = false;
            device.last_sensor = None;
            if enabled && telemetry.enabled() {
                let _ = telemetry.set(false, 0, false);
                emit(
                    proxy,
                    json!({"type":"telemetryStatus","enabled":false,"message":"Telemetry light released for battery color"}),
                );
            }
            device.battery_light = enabled;
            emit(proxy, json!({"type":"lightMode","battery":enabled}));
            if enabled {
                Ok("Battery light on")
            } else {
                device.release_lightbar().map(|_| "Battery light off")
            }
        }
        DeviceCommand::SetScreenLightbar { enabled, monitor } => {
            if let Some(index) = monitor {
                screen.set_monitor(index);
            }
            if !enabled && !device.screen_light {
                emit(
                    proxy,
                    json!({"type":"lightMode","battery":device.battery_light,"sensor":device.sensor_light,"screen":false}),
                );
                return true;
            }
            if enabled {
                if telemetry.enabled() {
                    let _ = telemetry.set(false, 0, false);
                    emit(
                        proxy,
                        json!({"type":"telemetryStatus","enabled":false,"message":"Telemetry light released for screen color"}),
                    );
                }
                let _ = device.release_lightbar();
                device.battery_light = false;
                device.sensor_light = false;
                device.last_sensor = None;
                device.screen_light = true;
                device.screen_color = None;
                screen.set_enabled(true);
                emit(
                    proxy,
                    json!({"type":"lightMode","battery":false,"screen":true}),
                );
                Ok("Following primary screen color")
            } else {
                screen.set_enabled(false);
                device.screen_light = false;
                device.screen_color = None;
                emit(
                    proxy,
                    json!({"type":"lightMode","battery":false,"screen":false}),
                );
                device.release_lightbar().map(|_| "Screen color off")
            }
        }
        DeviceCommand::SetTemperatureLightbar { enabled } => {
            if !enabled && !device.sensor_light {
                emit(
                    proxy,
                    json!({"type":"lightMode","battery":device.battery_light,"sensor":false,"screen":device.screen_light}),
                );
                return true;
            }
            screen.set_enabled(false);
            device.screen_light = false;
            device.screen_color = None;
            if enabled && telemetry.enabled() {
                let _ = telemetry.set(false, 0, false);
                emit(
                    proxy,
                    json!({"type":"telemetryStatus","enabled":false,"message":"Telemetry light released for sensor color"}),
                );
            }
            if enabled {
                let _ = device.release_lightbar();
            }
            device.battery_light = false;
            device.sensor_light = enabled;
            device.last_sensor = None;
            emit(
                proxy,
                json!({"type":"lightMode","battery":false,"sensor":enabled}),
            );
            if enabled {
                Ok("Following the raw sensor byte")
            } else {
                device.release_lightbar().map(|_| "Sensor color off")
            }
        }
        DeviceCommand::PlayHaptic { shape, .. } if !VoiceClip::is_known_shape(&shape) => {
            Err("Unknown haptic shape. Use sine, square, saw, noise, triangle, pulse, double or swell.".into())
        }
        DeviceCommand::PlayHaptic {
            shape,
            frequency,
            attack,
            release,
            strength,
            pan,
        } => {
            let voice = VoiceClip::new(&shape, frequency, attack, release, strength, pan);
            if matches!(device.transport, BusType::Usb)||device.audio.bluetooth_enabled() {
                match device.play_voice(voice) {
                    Ok(endpoint) => {
                        emit(
                            proxy,
                            json!({"type":"audioStatus","route":"voiceCoil","endpoint":endpoint,"callbacks":device.audio.callbacks()}),
                        );
                        Ok("USB voice-coil haptic playing")
                    }
                    Err(error) => {
                        emit(
                            proxy,
                            json!({"type":"audioStatus","route":"motor","reason":error}),
                        );
                        let clip =
                            HapticClip::new(shape, frequency, attack, release, strength, pan);
                        device.rumble_until = Some(Instant::now() + clip.duration());
                        device.haptic = Some(clip);
                        Ok("Haptic playing through motor fallback")
                    }
                }
            } else {
                emit(
                    proxy,
                    json!({"type":"audioStatus","route":"motor","reason":"Bluetooth voice-coil transport is not validated."}),
                );
                let clip = HapticClip::new(shape, frequency, attack, release, strength, pan);
                device.rumble_until = Some(Instant::now() + clip.duration());
                device.haptic = Some(clip);
                Ok("Bluetooth motor haptic playing")
            }
        }
        DeviceCommand::SetAdaptiveTrigger { left, right } => {
            let encode = |side: &Option<protocol::ExtraTrigger>| {
                side.as_ref().map(protocol::encode_extra_trigger).transpose()
            };
            match (encode(&left), encode(&right)) {
                (Ok(left_block), Ok(right_block)) => {
                    if left_block.is_none() && right_block.is_none() {
                        Err("Choose at least one trigger effect.".into())
                    } else if device.personal.enabled {
                        // Personal limits cannot scale these experimental modes (they would be
                        // written as OFF), so refuse instead of reporting them as applied.
                        Err("Extra trigger modes are unavailable while personal limits are on.".into())
                    } else {
                        let _ = device.restore_action();
                        let (base_left, base_right) =
                            device.last_trigger.unwrap_or((TRIGGER_OFF, TRIGGER_OFF));
                        let report = protocol::usb_report(
                            (if left_block.is_some() { 0x08 } else { 0 })
                                | (if right_block.is_some() { 0x04 } else { 0 }),
                            0,
                            0,
                            right_block,
                            left_block,
                            None,
                            None,
                            false,
                        );
                        device.write(report).map(|_| {
                            device.last_trigger = Some((
                                left_block.unwrap_or(base_left),
                                right_block.unwrap_or(base_right),
                            ));
                            device.input_effect_until = None;
                            device.bows = [None, None];
                            device.trigger_active = true;
                            device.trigger_until = Some(Instant::now() + Duration::from_secs(15));
                            emit(
                                proxy,
                                json!({"type":"triggerLease","active":true,"durationMs":15000}),
                            );
                            "Extra trigger mode applied (15s, unverified on hardware)"
                        })
                    }
                }
                (Err(error), _) | (_, Err(error)) => Err(error.into()),
            }
        }
        DeviceCommand::SetMicrophoneLed { mode } => match protocol::microphone_led_report(mode) {
            Ok(report) => device.write(report).map(|_| {
                // The LED-only report does not mute the microphone; report both separately.
                emit(
                    proxy,
                    json!({"type":"microphoneStatus","muted":device.microphone_muted.unwrap_or(false),"led":mode}),
                );
                "Microphone light set"
            }),
            Err(error) => Err(error.into()),
        },
        DeviceCommand::SetAudioVolumes {
            headphone,
            speaker,
            microphone,
        } => match protocol::audio_volume_report(headphone, speaker, microphone) {
            Ok(report) => device.write(report).map(|_| {
                emit(
                    proxy,
                    json!({"type":"audioVolumes","controllerId":device.controller_id,"headphone":headphone,"speaker":speaker,"microphone":microphone}),
                );
                "Controller volumes set"
            }),
            Err(error) => Err(error.into()),
        },
        DeviceCommand::SetLedBrightness {
            brightness,
            lightbar_setup,
        } => match protocol::led_brightness_report(brightness, lightbar_setup) {
            Ok(report) => device.write(report).map(|_| {
                device.led_brightness_dirty.0 |= brightness.is_some();
                device.led_brightness_dirty.1 |= lightbar_setup.is_some();
                emit(
                    proxy,
                    json!({"type":"ledBrightness","controllerId":device.controller_id,"brightness":brightness,"lightbarSetup":lightbar_setup}),
                );
                "LED brightness set"
            }),
            Err(error) => Err(error.into()),
        },
        DeviceCommand::SetLightbarAnimation {
            mode,
            rgb,
            period_ms,
        } => {
            if mode == "off" {
                let had = device.anim.take().is_some();
                let restored = match device.user_light {
                    Some(color) if had => device.set_light(color),
                    _ if had => device.release_lightbar(),
                    _ => Ok(()),
                };
                restored.map(|_| {
                    emit(proxy, json!({"type":"lightAnimation","mode":"off"}));
                    "Lightbar animation off"
                })
            } else if let Some(kind) = LightAnimKind::parse(&mode) {
                screen.set_enabled(false);
                device.screen_light = false;
                device.screen_color = None;
                device.sensor_light = false;
                device.last_sensor = None;
                device.battery_light = false;
                if telemetry.enabled() {
                    let _ = telemetry.set(false, 0, false);
                }
                let period_ms = period_ms.unwrap_or(3000).clamp(300, 20_000);
                let rgb = rgb.or(device.user_light).unwrap_or([77, 202, 177]);
                device.anim = Some(LightAnim {
                    kind,
                    rgb,
                    period_ms,
                    started: Instant::now(),
                    next: Instant::now(),
                });
                emit(proxy, json!({"type":"lightMode","battery":false}));
                emit(
                    proxy,
                    json!({"type":"lightAnimation","mode":mode,"rgb":rgb,"periodMs":period_ms}),
                );
                Ok("Lightbar animation on")
            } else {
                Err("Animation must be off, breathe, pulse, rainbow or lowBatteryBlink.".into())
            }
        }
        DeviceCommand::SetMicrophoneMute { enabled } => {
            device.set_microphone_mute(enabled).map(|_| {
                emit(proxy, json!({"type":"microphoneStatus","muted":enabled}));
                if enabled {
                    "Controller microphone muted"
                } else {
                    "Controller microphone live"
                }
            })
        }
        DeviceCommand::PlaySpeakerChirp => {
            if !matches!(device.transport, BusType::Usb) {
                Err("Controller speaker chirps require USB.".into())
            } else {
                let output = device.audio.start_output().map(|_| ());
                output.and_then(|_| device.set_speaker_route(true)).and_then(|_| device.audio.play_speaker_chirp()).map(|endpoint| {
                    device.speaker_route_until = Some(Instant::now() + Duration::from_millis(950));
                    emit(proxy, json!({"type":"speakerStatus","source":"chirp","message":format!("Chirp sent to {endpoint}")}));
                    "Controller speaker chirp playing"
                })
            }
        }
        DeviceCommand::SetAudioMixer {settings} => device.audio.set_mixer(settings).map(|_|"Audio mixer saved"),
        DeviceCommand::PlayAudioFile {path,position,duration,route,kind,track_id} => {
            if !matches!(device.transport,BusType::Usb)&&!device.audio.bluetooth_enabled() {Err("Use USB, or enable experimental Bluetooth grip audio and choose Grips only.".into())}
            else {
                device.audio.start_output().map(|_|()).and_then(|_|device.enable_pcm())
                    .and_then(|_|device.set_speaker_route(route!="grips"))
                    .and_then(|_|device.audio.play_file(path,(position,duration),&route,&kind,track_id))
                    .map(|_|{
                        device.speaker_track_id=track_id;
                        device.speaker_route_until=(route!="grips").then(||Instant::now()+Duration::from_secs(2));
                        "Opening controller audio"
                    })
            }
        }
        DeviceCommand::PlaySpeakerSound {
            track_id,
            pcm_base64,
            route,
            append,
        } => {
            if append && track_id != device.speaker_track_id {
                Err("Ignored an audio chunk from an old track.".into())
            } else if !matches!(device.transport, BusType::Usb)&&!device.audio.bluetooth_enabled() {
                Err("Controller speaker playback requires USB.".into())
            } else if pcm_base64.len() > 400_000 {
                Err("Sound is too long. Use at most 3 seconds.".into())
            } else {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(pcm_base64)
                    .map_err(|_| "Sound data is invalid".to_string());
                bytes.and_then(|bytes| {
                    if bytes.is_empty() || bytes.len() > 288_000 || bytes.len() % 2 != 0 {
                        return Err("Sound must be 48 kHz PCM and at most 3 seconds".into());
                    }
                    let samples: Vec<i16> = bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect();
                    let duration = Duration::from_secs_f32(samples.len() as f32 / 48_000.0);
                    let routing = device.audio.start_output().map(|_| ());
                    routing.and_then(|_| device.enable_pcm()).and_then(|_| if append { Ok(()) } else if route == "grips" { device.set_speaker_route(false) } else { device.set_speaker_route(true) })
                    .and_then(|_| device.audio.play_speaker_pcm(samples, &route, append)).map(|endpoint| {
                        device.speaker_track_id = track_id;
                        if route != "grips" {
                            let now = Instant::now();
                            let end = device.speaker_route_until.unwrap_or(now).max(now);
                            device.speaker_route_until = Some(if append { end + duration } else { now + duration + Duration::from_secs(1) });
                        }
                        if !append {
                            emit(proxy, json!({"type":"speakerStatus","source":"file","trackId":track_id,"message":format!("Sound sent to {endpoint}")}));
                        }
                        "Controller speaker playing"
                    })
                })
            }
        }
        DeviceCommand::StopSpeakerSound { track_id } => {
            device.speaker_track_id = track_id;
            device.audio.stop_speaker_pcm();
            emit(proxy, json!({"type":"speakerStatus","source":"stopped","trackId":track_id,"position":0}));
            if matches!(device.transport, BusType::Usb) {
                device.set_speaker_route(false).map(|_| "Controller audio stopped")
            } else {
                Ok("Controller audio stopped")
            }
        }
        DeviceCommand::SetSpeakerPaused { paused, track_id } => {
            (if track_id != device.speaker_track_id { Err("The audio track changed.".into()) } else { device.audio.pause_speaker_pcm(paused) }).map(|playback| {
                device.speaker_route_until = (!playback.paused && playback.speaker).then(|| Instant::now() + Duration::from_secs_f32(playback.remaining + 1.0));
                emit(proxy,json!({"type":"speakerStatus","source":"transport","trackId":track_id,"paused":playback.paused,"position":playback.position,"message":if paused {"Paused"} else {"Playing"}}));
                if paused { "Controller audio paused" } else { "Controller audio resumed" }
            })
        }
        DeviceCommand::SetAudioBridge {
            enabled,
            gain,
            preset,source,
        } => {
            if enabled && !crate::audio_haptics::capture_supported() {
                Err("System audio capture is unavailable on this OS version. macOS requires 14.6 or later.".into())
            } else if enabled && !matches!(device.transport, BusType::Usb)&&!device.audio.bluetooth_enabled() {
                Err("System audio haptics require USB. Bluetooth motor fallback remains available in Haptic Lab.".into())
            } else {
                let result = if enabled { source.validate().and_then(|_|device.enable_pcm()) } else { Ok(()) }
                    .and_then(|_| if source==CaptureSource::System {device.audio.set_bridge(enabled,gain,&preset)}else{device.audio.set_capture(enabled, gain, &preset,source.clone())});
                if let Ok(message) = &result {
                    emit(
                        proxy,
                        json!({"type":"audioStatus","route":if enabled {"voiceCoil"} else {"off"},"bridge":enabled,"captureSource":if enabled{Some(&source)}else{None},"endpoint":device.audio.endpoint(),"message":message}),
                    );
                } else if let Err(message) = &result {
                    emit(proxy,json!({"type":"audioStatus","route":if device.audio.bridge_on(){"voiceCoil"}else{"off"},"bridge":device.audio.bridge_on(),"captureSource":device.audio.capture_source(),"captureError":true,"message":message}));
                }
                result.map(|_| {
                    if enabled {
                        "Audio bridge on"
                    } else {
                        "Audio bridge off"
                    }
                })
            }
        }
        DeviceCommand::SetBridgeLatency { value } => device
            .audio
            .set_bridge_latency(&value)
            .inspect(|_| emit(proxy, json!({"type":"bridgeLatency","value":value})))
            .map(|_| "Bridge latency set (USB period changes when the stream reopens)"),
        DeviceCommand::SetHapticOutputLevel { level } => device
            .audio
            .set_output_level(&level)
            .inspect(|name| emit(proxy, json!({"type":"hapticOutputLevel","level":name})))
            .map(|name| match name {
                "soft" => "Overall intensity: Soft",
                "normal" => "Overall intensity: Normal",
                "max" => "Overall intensity: Max",
                _ => "Overall intensity: Strong",
            }),
        DeviceCommand::SetBridgeCustom {
            cutoff,
            bass,
            transient,
            gate,
        } => device
            .audio
            .set_bridge_custom(crate::audio_haptics::BridgeCustom {
                cutoff,
                bass,
                transient,
                gate,
            })
            .inspect(|_| {
                emit(
                    proxy,
                    json!({"type":"bridgeCustom","cutoff":cutoff,"bass":bass,"transient":transient,"gate":gate}),
                )
            })
            .map(|_| "Custom bridge preset saved"),
        DeviceCommand::SetBridgeOptions {
            mode,
            band_gains,
            onset,
            pan_emphasis,
        } => device
            .audio
            .set_bridge_options(crate::audio_haptics::BridgeOptions {
                mode: mode.clone(),
                band_gains,
                onset,
                pan_emphasis,
            })
            .inspect(|_| {
                emit(
                    proxy,
                    json!({"type":"bridgeOptions","mode":mode,"bandGains":band_gains,"onset":onset,"panEmphasis":pan_emphasis}),
                )
            })
            .map(|_| "Bridge options saved"),
        DeviceCommand::SetProcessReattach { enabled } => {
            device.audio.set_process_reattach(enabled);
            emit(proxy, json!({"type":"processReattach","enabled":enabled}));
            Ok(if enabled {
                "Process capture will re-attach after the app restarts"
            } else {
                "Process re-attach off"
            })
        }
        DeviceCommand::SetInputEffects {
            enabled,
            threshold,
            strength,
            kind,
            rate,
            spacing_ms,
            kick_ms,
            kick_style,
            tuning,
        } => {
            if !threshold.is_finite()
                || !(0.2..=0.95).contains(&threshold)
                || strength > 100
                || (enabled && strength == 0)
                || !(3..=20).contains(&rate)
                || (spacing_ms != 0 && !(50..=1000).contains(&spacing_ms))
                || (kick_ms != 0 && !(20..=250).contains(&kick_ms))
                || (kind == "burst" && kick_style != "strike" && kick_ms != 0
                    && u32::from(kick_ms) + 20
                        > if spacing_ms == 0 { 1000 / u32::from(rate.max(1)) } else { u32::from(spacing_ms) })
                || WeaponKind::parse(&kind).is_none()
                || KickStyle::parse(&kick_style).is_none()
                || tuning.validate(kick_style == "return").is_err()
            {
                Err("Weapon cue settings are out of range.".into())
            } else {
                let enabled = enabled && device.bows[1].is_none();
                device.restore_input_effect().map(|_| {
                    device.input_effect = enabled.then_some(InputEffect {threshold,strength,kind:WeaponKind::parse(&kind).unwrap(),rate,spacing_ms,kick_ms,remaining:0,kick_style:KickStyle::parse(&kick_style).unwrap(),tuning,armed:true,next_pulse:None,last_pulse:None,release_since:None});
                    emit(proxy, json!({"type":"inputEffects","enabled":enabled,"threshold":threshold,"strength":strength,"kind":kind,"rate":rate,"spacingMs":spacing_ms,"kickMs":kick_ms,"kickStyle":kick_style,"hapticStrength":tuning.haptic_strength,"hapticFrequency":tuning.haptic_frequency,"hapticDurationMs":tuning.haptic_duration_ms}));
                    if enabled {"Weapon cue armed"} else {"Weapon cue off"}
                })
            }
        }
        DeviceCommand::SetLowBatteryAlert { enabled } => {
            device.low_battery_alert = enabled;
            device.low_battery_latched = false;
            emit(
                proxy,
                json!({"type":"hapticAlertStatus","lowBattery":enabled}),
            );
            Ok(if enabled {
                "Low battery alert on"
            } else {
                "Low battery alert off"
            })
        }
        DeviceCommand::SetLowBatteryThresholds { warn, critical } => {
            if warn > 100 || critical == 0 || critical >= warn {
                Err("Choose a warning level above the critical level (1-100%).".into())
            } else {
                device.battery_thresholds = (warn, critical);
                device.low_battery_latched = false;
                device.low_battery_critical_latched = false;
                emit(
                    proxy,
                    json!({"type":"hapticAlertStatus","lowBattery":device.low_battery_alert,"warn":warn,"critical":critical}),
                );
                Ok("Low battery thresholds set")
            }
        }
        DeviceCommand::SetHapticTimer { seconds } => {
            if seconds > 3600 {
                Err("Choose a timer up to 60 minutes.".into())
            } else {
                device.haptic_timer =
                    (seconds > 0).then(|| Instant::now() + Duration::from_secs(u64::from(seconds)));
                emit(
                    proxy,
                    json!({"type":"hapticAlertStatus","timerSeconds":seconds}),
                );
                Ok(if seconds > 0 {
                    "Haptic timer started"
                } else {
                    "Haptic timer cleared"
                })
            }
        }
        DeviceCommand::Rumble {
            left,
            right,
            duration_ms,
        } => {
            device.haptic = None;
            let duration = duration_ms.clamp(20, 500);
            let motors = [percent_to_motor(left), percent_to_motor(right)];
            device.write_rumble(motors).map(|_| {
                device.rumble_until = Some(Instant::now() + Duration::from_millis(duration));
                "Rumble playing"
            })
        }
        DeviceCommand::Reset => {
            screen.set_enabled(false);
            let _ = telemetry.set(false, 0, false);
            *script = None;
            emit(
                proxy,
                json!({"type":"scriptStatus","enabled":false,"message":"Script off"}),
            );
            emit(
                proxy,
                json!({"type":"telemetryStatus","enabled":false,"message":"Telemetry bridge off"}),
            );
            stop_input_mappers(mouse, gyro, remap, xinput, proxy);
            emit(proxy, json!({"type":"safetyReset"}));
            device.battery_light = false;
            emit(proxy, json!({"type":"lightMode","battery":false}));
            device.reset().map(|_| {
                emit(
                    proxy,
                    json!({"type":"triggerLease","active":false,"durationMs":0}),
                );
                "Effects reset"
            })
        }
        DeviceCommand::SetHidRecording{..}|DeviceCommand::GetHidTrace|DeviceCommand::InspectHidTrace{..}|DeviceCommand::SetRuntimeDiagnostics{..}|DeviceCommand::LatencyProbe{..} => unreachable!(),
        DeviceCommand::ProfileCheckpoint { .. }
        | DeviceCommand::CheckDesktopInput
        | DeviceCommand::SetNativeHandoff { .. }
        | DeviceCommand::Scan
        | DeviceCommand::Shutdown
        | DeviceCommand::ListDevices
        | DeviceCommand::SelectDevice { .. }
        | DeviceCommand::SetTouchMouse { .. }
        | DeviceCommand::SetTouchGestures { .. }
        | DeviceCommand::SetGyroAim { .. }
        | DeviceCommand::SetRemap { .. }
        | DeviceCommand::SetRemapTiming { .. }
        | DeviceCommand::SetPadExtras { .. }
        | DeviceCommand::PreviewEnvelope { .. }
        | DeviceCommand::SetXinput { .. }
        | DeviceCommand::SetTelemetry { .. }
        | DeviceCommand::SetScript { .. }
        | DeviceCommand::SetUiFocused { .. }
        | DeviceCommand::SetPcMode { .. }
        | DeviceCommand::SetPcBindings { .. }
        | DeviceCommand::SetCalibration { .. }
        | DeviceCommand::ReloadOutputCalibration { .. }
        | DeviceCommand::SetGameActions { .. }
        | DeviceCommand::SetGameLink { .. }
        | DeviceCommand::StopMacros
        | DeviceCommand::SetGameRecording { .. } => unreachable!(),
        DeviceCommand::SetWeaponOverride{..}|DeviceCommand::WriteEdgeProfile{..}|DeviceCommand::NativeProviderControl{..}=>unreachable!(),
    };
    match result {
        Ok(message) => emit(proxy, json!({ "type": "notice", "message": message })),
        Err(message) => {
            if let Some((id, request)) = edge_request {
                emit(
                    proxy,
                    json!({"type":"edgeReadStatus","requestId":request,"controllerId":id,"error":message}),
                );
                return true;
            }
            if let Some(play_id) = effect_request {
                emit(
                    proxy,
                    json!({"type":"effectTimeline","playId":play_id,"playing":false,"error":message,"positionMs":0,"durationMs":0}),
                );
            }
            if let Some(track_id) = audio_request {
                emit(
                    proxy,
                    json!({"type":"speakerStatus","source":"error","trackId":track_id,"message":message}),
                );
            }
            emit(proxy, json!({ "type": "error", "message": message }));
        }
    }
    true
}

pub fn run(rx: Receiver<DeviceCommand>, proxy: Sender<serde_json::Value>, disable_hid: bool) {
    let mut api = match HidApi::new() {
        Ok(api) => api,
        Err(error) => {
            emit(
                &proxy,
                json!({ "type": "error", "message": format!("HID initialization failed: {error}") }),
            );
            return;
        }
    };
    let mut device: Option<Connected> = None;
    let mut mouse = MouseMapper::default();
    let mut gyro = GyroAimMapper::default();
    let mut remap = RemapMapper::default();
    let mut xinput = XInputBridge::default();
    let mut telemetry = TelemetryBridge::default();
    let mut game_link = GameLink::default();
    let mut next_game_error = Instant::now();
    let mut script: Option<ScriptProgram> = None;
    let mut last_suspension: (Option<&'static str>, Option<&'static str>) = (None, None);
    let mut last_script_cue: Option<Instant> = None;
    let screen = ScreenSampler::new();
    emit(&proxy, xinput.status_event());
    let mut preferred = Preference::default();
    let mut next_scan = Instant::now();
    let mut native_handoff = false;
    let mut edge_worker: Option<crate::edge_writer::Worker> = None;
    let mut native_provider = crate::native_provider::Manager::load(disable_hid);
    emit(&proxy, native_provider.status());
    if native_provider.owns_controller() {
        emit(&proxy, json!({"type":"nativeHandoff","enabled":true}));
    }
    let mut input_report = [0u8; 78];
    let mut next_process_check = Instant::now();
    #[cfg(windows)]
    if let Some(pid) = std::env::var("CONTROLLER_GUI_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    {
        set_gui_pid(pid);
    }
    #[cfg(windows)]
    let mut next_focus_check = Instant::now();
    let mut last_conflicts = Vec::<&'static str>::new();
    let mut first_process_check = true;
    let mut last_foreground = (None::<String>, None::<String>, None::<String>);
    let mut raw_reports = 0u32;
    let mut sequence_gaps = 0u32;
    let mut invalid_reports = 0u32;
    let mut next_rate = Instant::now() + Duration::from_secs(1);
    let mut next_audio_status = Instant::now();
    let mut telemetry_stale_latched = false;
    let mut recorder = crate::hid_trace::Recorder::default();
    let mut last_recording = false;
    let mut metrics = crate::runtime_diagnostics::Metrics::default();
    let mut diagnostics_until: Option<Instant> = None;
    let mut next_metrics = Instant::now();
    let mut last_loop = Instant::now();
    let mut deferred_audio = std::collections::VecDeque::<DeviceCommand>::new();
    let mut deferred_owner: Option<String> = None;
    loop {
        if let Some(event) = native_provider.poll() {
            emit(&proxy, event);
            emit(
                &proxy,
                json!({"type":"nativeHandoff","enabled":native_handoff||native_provider.owns_controller()}),
            );
            next_scan = Instant::now();
        }
        if let Some(event) = edge_worker.as_ref().and_then(|worker| worker.poll()) {
            emit(&proxy, event);
            edge_worker = None;
            next_scan = Instant::now();
        }
        let now = Instant::now();
        let diagnostics = diagnostics_until.is_some_and(|until| until > now);
        if diagnostics {
            metrics
                .loop_period
                .record(crate::runtime_diagnostics::elapsed_us(last_loop));
        }
        last_loop = now;
        recorder.tick();
        if last_recording && !recorder.active() {
            emit(
                &proxy,
                json!({"type":"hidRecording","active":false,"events":recorder.len()}),
            );
        }
        last_recording = recorder.active();
        if recorder.active() {
            recorder.connection(device.as_ref().map(|d| {
                (
                    format!("{}:{:?}", d.controller_id, d.opened_at),
                    if matches!(d.transport, BusType::Bluetooth) {
                        "Bluetooth"
                    } else {
                        "USB"
                    }
                    .into(),
                )
            }));
        }
        if let Some(connected) = device.as_mut()
            && crate::output_calibration::neutral_due(
                connected.last_valid_report,
                Instant::now(),
                connected.input_stale,
            )
        {
            connected.input_stale = true;
            if connected.game_rumble != [0, 0] {
                if matches!(connected.transport, BusType::Usb)
                    || connected.audio.bluetooth_enabled()
                {
                    let _ = connected.audio.set_game_rumble([0, 0]);
                } else {
                    let _ = connected.write_rumble([0, 0]);
                }
                connected.game_rumble = [0, 0];
            }
            if let Some(event) = xinput.neutralize() {
                emit(&proxy, event);
            }
            connected.desktop.cancel_for_modal();
            remap.set_suspended(true);
            mouse.cancel_pointer();
            emit(
                &proxy,
                json!({"type":"outputInputHealth","stale":true,"controllerId":connected.controller_id}),
            );
        }
        if let Some(connected) = device.as_mut()
            && let Err(error) = connected.audio.poll_output()
        {
            audio_failure(connected, &mut deferred_audio, &proxy, &error);
        }
        if !deferred_audio.is_empty()
            && device.as_ref().map(|d| &d.controller_id) != deferred_owner.as_ref()
        {
            cancel_audio_commands(
                &mut deferred_audio,
                &proxy,
                "Controller changed before audio was ready.",
            );
        }
        if diagnostics && now >= next_metrics {
            emit(
                &proxy,
                json!({"type":"runtimeStats","softwareOnly":true,"hidEnabled":!disable_hid,"metrics":metrics.report(),"write":device.as_ref().map(|d|d.write_timing.report()),"processDiscovery":processes::diagnostics(),"core":crate::runtime_diagnostics::resources(),"recording":recorder.active()}),
            );
            next_metrics = now + Duration::from_secs(1);
        }
        #[cfg(windows)]
        if let Some(gui_pid) = gui_pid()
            && Instant::now() >= next_focus_check
        {
            let focused = processes::gui_window_focused(gui_pid);
            if let Some(connected) = device.as_mut() {
                connected.desktop.set_ui_focused(focused);
                remap.set_suspended(focused || connected.desktop.enabled());
            }
            next_focus_check = Instant::now() + Duration::from_millis(60);
        }
        if Instant::now() >= next_audio_status {
            if let Some(connected) = device.as_mut() {
                if let Some((track_id, error)) = connected.audio.take_file_error() {
                    connected.audio.stop_speaker_pcm();
                    let _ = connected.set_speaker_route(false);
                    emit(
                        &proxy,
                        json!({"type":"speakerStatus","source":"error","trackId":track_id,"message":error}),
                    );
                }
                if let Some(status) = connected.audio.file_status()
                    && status["native"] == true
                    && status["speaker"] == true
                {
                    if matches!(
                        status["mode"].as_str(),
                        Some("playing" | "starting" | "buffering")
                    ) {
                        connected.speaker_route_until =
                            Some(Instant::now() + Duration::from_secs(2));
                    } else if status["mode"] == "finished"
                        && connected.speaker_route_until.is_some()
                    {
                        let _ = connected.set_speaker_route(false);
                    }
                }
                if let Some(error) = connected.audio.take_capture_error() {
                    emit(
                        &proxy,
                        json!({"type":"audioStatus","route":"off","bridge":false,"message":error}),
                    );
                    emit(&proxy, json!({"type":"error","message":error}));
                }
                let mut audio_status = connected.audio.diagnostics();
                audio_status["controllerId"] = json!(connected.controller_id);
                audio_status["firmware"] = json!(connected.firmware);
                emit(&proxy, audio_status);
                let now = Instant::now();
                let trigger_source = if connected
                    .timeline
                    .as_ref()
                    .is_some_and(|t| t.clip.trigger_mask() != 0)
                {
                    "Timeline"
                } else if connected
                    .input_effect_until
                    .is_some_and(|until| until > now)
                {
                    "Weapon feedback"
                } else if connected.action_until.is_some_and(|until| until > now) {
                    "Game action"
                } else if connected.trigger_active {
                    "Profile preview"
                } else {
                    "Idle"
                };
                let motor_source = if connected
                    .link_experiment
                    .as_ref()
                    .is_some_and(|run| run.bluetooth)
                {
                    "Bluetooth experiment"
                } else if connected.last_rumble.is_some_and(|v| v != [0, 0]) {
                    if connected.haptic.is_some() {
                        "Motor cue"
                    } else if connected.game_rumble != [0, 0] {
                        "Xbox rumble"
                    } else {
                        "Motor rumble"
                    }
                } else {
                    "Idle"
                };
                let light_source = if connected.screen_light {
                    "Screen color"
                } else if connected.battery_light {
                    "Battery"
                } else if connected.sensor_light {
                    "Sensor color"
                } else if connected.light_active {
                    "Custom color"
                } else {
                    "Released"
                };
                emit(
                    &proxy,
                    json!({"type":"feedbackState","controllerId":connected.controller_id,
                    "triggerSource":trigger_source,"motorSource":motor_source,"lightSource":light_source}),
                );
            }
            next_audio_status = Instant::now() + Duration::from_millis(250);
        }
        if Instant::now() >= next_rate {
            #[cfg(not(windows))]
            if let Some(message) = crate::native_input::recheck() {
                stop_input_mappers(&mut mouse, &mut gyro, &mut remap, &mut xinput, &proxy);
                if let Some(d) = device.as_mut() {
                    d.desktop.set_enabled(false);
                    d.radial.cancel();
                }
                emit(&proxy, json!({"type":"pcMode","enabled":false}));
                emit(
                    &proxy,
                    json!({"type":"desktopInputStatus","ready":false,"message":message,"environment":crate::integrations::environment(),"pointerPrecision":crate::input_map::pointer_precision_enabled()}),
                );
                emit(&proxy, json!({"type":"error","message":message}));
            }
            if let Some(message) = crate::input_map::take_input_error() {
                emit(
                    &proxy,
                    json!({"type":"desktopInputError","message":message}),
                );
            }
            if telemetry.stale() && !telemetry_stale_latched {
                telemetry_stale_latched = true;
                if let Some(connected) = device.as_mut() {
                    let _ = connected.release_lightbar();
                }
                emit(
                    &proxy,
                    json!({"type":"telemetryStatus","enabled":true,"port":telemetry.port(),"lan":telemetry.lan(),"format":telemetry.format(),"message":"Listening, but no valid telemetry arrived in the last second."}),
                );
            }
            if let Some(connected) = device.as_mut() {
                if let Some(error) = connected.audio.take_error() {
                    audio_failure(connected, &mut deferred_audio, &proxy, &error);
                } else if connected.audio.bridge_on() {
                    emit(
                        &proxy,
                        json!({"type":"audioStatus","route":"voiceCoil","bridge":true,"endpoint":connected.audio.endpoint(),"callbacks":connected.audio.callbacks(),"glitches":connected.audio.glitches(),"speakerFrames":connected.audio.speaker_frames(),"gripFrames":connected.audio.grip_frames(),"bridgeFrames":connected.audio.bridge_frames()}),
                    );
                }
            }
            emit(
                &proxy,
                json!({"type":"rate","reportsPerSecond":raw_reports,"sequenceGaps":sequence_gaps,"invalidReports":invalid_reports}),
            );
            raw_reports = 0;
            sequence_gaps = 0;
            invalid_reports = 0;
            next_rate = Instant::now() + Duration::from_secs(1);
        }
        if Instant::now() >= next_process_check {
            let status = processes::inspect();
            if first_process_check || status.conflicts != last_conflicts {
                emit(
                    &proxy,
                    json!({"type":"conflict","detected":!status.conflicts.is_empty(),
                    "message":if status.conflicts.is_empty() { "No conflicting tools.".to_string() }
                    else { status.conflicts.join(" ") } }),
                );
                last_conflicts = status.conflicts;
            }
            first_process_check = false;
            let foreground = (
                status.foreground_exe,
                status.foreground_title,
                status.foreground_path,
            );
            if foreground != last_foreground {
                emit(
                    &proxy,
                    json!({"type":"foreground","exe":foreground.0.as_deref().unwrap_or(""),"title":foreground.1.as_deref().unwrap_or(""),"path":foreground.2.as_deref().unwrap_or("")}),
                );
                last_foreground = foreground;
            }
            next_process_check = Instant::now() + Duration::from_millis(250);
        }
        // Bound command work per pass so UI/file bursts cannot starve HID reads.
        for _ in 0..32 {
            let from_deferred = !deferred_audio.is_empty()
                && device.as_ref().is_some_and(|d| !d.audio.output_opening());
            let command = if from_deferred {
                Ok(deferred_audio.pop_front().unwrap())
            } else {
                rx.try_recv()
            };
            match command {
                Ok(command) => {
                    if let DeviceCommand::NativeProviderControl {
                        operation,
                        controller_id,
                        request_id,
                        confirmed,
                    } = command
                    {
                        let mut prepared = false;
                        let result = (|| {
                            if disable_hid {
                                return Err(
                                    "Native provider access is disabled for this core session."
                                        .to_string(),
                                );
                            }
                            if edge_worker.is_some() || edge_busy() {
                                return Err("Wait for the Edge profile operation to finish.".into());
                            }
                            if operation == crate::native_provider::Operation::Attach {
                                let active=device.as_ref().filter(|d|Some(&d.controller_id)==controller_id.as_ref()&&matches!(d.transport,BusType::Bluetooth)).ok_or("Select the same Bluetooth controller before attaching the native bridge.")?;
                                let address = active.bluetooth_address.clone().ok_or(
                                    "The selected controller has no verified Bluetooth address.",
                                )?;
                                let lease = crate::native_provider::Lease::new(
                                    active.controller_id.clone(),
                                    address,
                                    active.product_id == protocol::EDGE_PID,
                                )?;
                                native_provider.prepare_attach(lease)?;
                                prepared = true;
                                game_link.stop();
                                let _ = telemetry.set(false, 0, false);
                                script = None;
                                screen.set_enabled(false);
                                cancel_audio_commands(
                                    &mut deferred_audio,
                                    &proxy,
                                    "Audio stopped for native bridge handoff.",
                                );
                                deferred_owner = None;
                                stop_input_mappers(
                                    &mut mouse,
                                    &mut gyro,
                                    &mut remap,
                                    &mut xinput,
                                    &proxy,
                                );
                                let reset = if let Some(mut active) = device.take() {
                                    teardown(&mut active, &proxy)
                                } else {
                                    Ok(())
                                };
                                emit(
                                    &proxy,
                                    json!({"type":"device","connected":false,"state":"handoff","reason":"native-bridge","message":"App monitoring paused for native bridge"}),
                                );
                                emit(&proxy, json!({"type":"safetyReset"}));
                                reset?;
                            }
                            native_provider.start(operation, request_id, confirmed)
                        })();
                        if result.is_err() && prepared {
                            let _ = native_provider.abandon_unstarted_attach();
                        }
                        let mut event = native_provider.status();
                        event["requestId"] = json!(request_id);
                        if let Err(error) = result {
                            event["error"] = json!(error);
                        }
                        emit(&proxy, event);
                        emit(
                            &proxy,
                            json!({"type":"nativeHandoff","enabled":native_handoff||native_provider.owns_controller()}),
                        );
                        continue;
                    }
                    if matches!(command, DeviceCommand::Scan | DeviceCommand::ListDevices)
                        && (native_handoff
                            || native_provider.owns_controller()
                            || edge_worker.is_some()
                            || edge_busy())
                    {
                        emit_cached_device_list(
                            &proxy,
                            preferred
                                .path
                                .as_deref()
                                .or(device.as_ref().map(|d| d.path_display.as_str())),
                        );
                        continue;
                    }
                    if native_provider.owns_controller()
                        && matches!(
                            command,
                            DeviceCommand::Scan
                                | DeviceCommand::SelectDevice { .. }
                                | DeviceCommand::SetXinput { enabled: true }
                                | DeviceCommand::SetNativeHandoff { enabled: false }
                        )
                    {
                        emit(
                            &proxy,
                            json!({"type":"error","message":"Detach the native bridge before resuming app monitoring or virtual Xbox output."}),
                        );
                        continue;
                    }
                    if edge_worker.is_some()
                        && !matches!(
                            command,
                            DeviceCommand::Shutdown
                                | DeviceCommand::Reset
                                | DeviceCommand::SetUiFocused { .. }
                                | DeviceCommand::LatencyProbe { .. }
                        )
                    {
                        if let DeviceCommand::WriteEdgeProfile {
                            data, request_id, ..
                        } = &command
                        {
                            emit(
                                &proxy,
                                json!({"type":"edgeWriteStatus","requestId":request_id,"controllerId":data.controller_id,"verified":false,"error":"An Edge write is already finishing."}),
                            );
                        } else {
                            emit(
                                &proxy,
                                json!({"type":"error","message":"Wait for the Edge profile write to finish."}),
                            );
                        }
                        continue;
                    }
                    if matches!(command, DeviceCommand::Reset | DeviceCommand::Shutdown)
                        && let Some(worker) = edge_worker.as_ref()
                    {
                        worker.cancel();
                    }
                    if let DeviceCommand::WriteEdgeProfile {
                        data,
                        slot,
                        edit,
                        request_id,
                    } = command
                    {
                        let prepared = (|| {
                            if disable_hid {
                                return Err("HID is disabled for this session.".to_string());
                            }
                            if edge_busy() {
                                return Err(
                                    "An Edge read is still finishing; try again in a moment."
                                        .to_string(),
                                );
                            }
                            let selected = device
                                .as_ref()
                                .filter(|d| {
                                    d.controller_id == data.controller_id
                                        && d.product_id == protocol::EDGE_PID
                                        && matches!(d.transport, BusType::Usb)
                                })
                                .ok_or(
                                    "Connect the same DualSense Edge over USB before writing.",
                                )?;
                            crate::edge_writer::validate_request(&data, slot, edit.clone())?;
                            std::ffi::CString::new(selected.path.clone()).map_err(|e| e.to_string())
                        })();
                        let owner = data.controller_id.clone();
                        let result=prepared.and_then(|path| {
                            game_link.stop();let _=telemetry.set(false,0,false);script=None;screen.set_enabled(false);cancel_audio_commands(&mut deferred_audio,&proxy,"Audio cancelled for the Edge profile write.");deferred_owner=None;
                            stop_input_mappers(&mut mouse,&mut gyro,&mut remap,&mut xinput,&proxy);
                            let mut active=device.take().ok_or("The selected controller disconnected.")?;
                            let reset=teardown(&mut active,&proxy);drop(active);
                            emit(&proxy,json!({"type":"device","connected":false,"state":"edge-busy","reason":"edge-write","message":"Saving Edge profile; monitoring is paused"}));reset?;
                            emit(&proxy,json!({"type":"safetyReset"}));
                            crate::edge_writer::start(path,data,slot,edit,request_id)
                        });
                        match result {
                            Ok(worker) => {
                                edge_worker = Some(worker);
                                emit(
                                    &proxy,
                                    json!({"type":"edgeWriteStarted","requestId":request_id,"controllerId":owner}),
                                );
                            }
                            Err(error) => emit(
                                &proxy,
                                json!({"type":"edgeWriteStatus","requestId":request_id,"controllerId":owner,"verified":false,"error":error}),
                            ),
                        }
                        continue;
                    }
                    if disable_hid
                        && matches!(
                            command,
                            DeviceCommand::Scan
                                | DeviceCommand::ListDevices
                                | DeviceCommand::SelectDevice { .. }
                        )
                    {
                        emit_device_list(&api, &proxy, None);
                        emit(
                            &proxy,
                            json!({"type":"device","connected":false,"state":"disconnected","reason":"hid-disabled","message":"HID disabled for this core session"}),
                        );
                        continue;
                    }
                    match &command {
                        DeviceCommand::SetHidRecording { enabled } => {
                            if *enabled {
                                recorder.start();
                            } else {
                                recorder.stop();
                            }
                            recorder.connection(device.as_ref().map(|d| {
                                (
                                    format!("{}:{:?}", d.controller_id, d.opened_at),
                                    if matches!(d.transport, BusType::Bluetooth) {
                                        "Bluetooth"
                                    } else {
                                        "USB"
                                    }
                                    .into(),
                                )
                            }));
                            last_recording = recorder.active();
                            emit(
                                &proxy,
                                json!({"type":"hidRecording","active":recorder.active(),"events":recorder.len()}),
                            );
                            continue;
                        }
                        DeviceCommand::GetHidTrace => {
                            if let Err(error) = crate::hid_trace::export_background(
                                recorder.snapshot(),
                                proxy.clone(),
                            ) {
                                emit(&proxy, json!({"type":"hidTrace","error":error}));
                            }
                            continue;
                        }
                        DeviceCommand::InspectHidTrace { json, request_id } => {
                            if let Err(error) = crate::hid_trace::analyze_background(
                                json.clone(),
                                *request_id,
                                proxy.clone(),
                            ) {
                                emit(
                                    &proxy,
                                    json!({"type":"hidReplay","requestId":request_id,"error":error}),
                                );
                            }
                            continue;
                        }
                        DeviceCommand::SetRuntimeDiagnostics { enabled } => {
                            diagnostics_until =
                                enabled.then(|| Instant::now() + Duration::from_secs(30));
                            if *enabled {
                                next_metrics = Instant::now();
                            }
                            continue;
                        }
                        DeviceCommand::LatencyProbe { id } => {
                            emit(&proxy, json!({"type":"latencyEcho","id":id}));
                            continue;
                        }
                        _ => {}
                    }
                    if !from_deferred
                        && command.primes_audio()
                        && let Some(d) = device.as_mut()
                    {
                        d.audio.allow_output_retry();
                    }
                    let opening = device.as_ref().is_some_and(|d| d.audio.output_opening());
                    if opening && command.bypasses_audio_wait() {
                        let before = deferred_audio.len();
                        deferred_audio.retain(|queued| !command.cancels_queued(queued));
                        if deferred_audio.len() != before
                            && !deferred_audio.iter().any(DeviceCommand::primes_audio)
                            && let Some(d) = device.as_mut()
                            && !d.audio.has_sources()
                        {
                            d.audio.stop();
                        }
                    }

                    if opening && command.cancels_audio_wait() {
                        cancel_audio_commands(
                            &mut deferred_audio,
                            &proxy,
                            "Audio startup cancelled.",
                        );
                        if let Some(d) = device.as_mut() {
                            d.audio.stop();
                        }
                    } else if opening && !command.bypasses_audio_wait() {
                        deferred_owner = device.as_ref().map(|d| d.controller_id.clone());
                        if deferred_audio.len() < 64 {
                            deferred_audio.push_back(command);
                        } else {
                            // Reject the incoming command with its own track/clip/checkpoint ID too.
                            deferred_audio.push_back(command);
                            cancel_audio_commands(
                                &mut deferred_audio,
                                &proxy,
                                "Too many commands arrived during audio startup. Try again.",
                            );
                            if let Some(d) = device.as_mut() {
                                d.audio.stop();
                            }
                        }
                        continue;
                    } else if command.primes_audio()
                        && device.as_ref().is_some_and(|d| {
                            matches!(d.transport, BusType::Usb)
                                && !d.audio.output_ready()
                                && !(from_deferred
                                    && command.motor_fallback()
                                    && d.audio.output_failed())
                        })
                    {
                        let d = device.as_mut().unwrap();
                        match d.audio.start_output() {
                            Ok(_) => {
                                deferred_owner = Some(d.controller_id.clone());
                                deferred_audio.push_back(command);
                                emit(&proxy, json!({"type":"audioStartup","stage":"starting"}));
                                continue;
                            }
                            Err(error) if command.motor_fallback() => {
                                d.audio.remember_output_failure(error);
                            }
                            Err(error) => {
                                deferred_audio.push_back(command);
                                cancel_audio_commands(&mut deferred_audio, &proxy, &error);
                                continue;
                            }
                        }
                    }
                    if let DeviceCommand::SetNativeHandoff { enabled } = command {
                        native_handoff = enabled;
                        if enabled {
                            game_link.stop();
                            let _ = telemetry.set(false, 0, false);
                            script = None;
                            screen.set_enabled(false);
                            cancel_audio_commands(
                                &mut deferred_audio,
                                &proxy,
                                "Audio cancelled while releasing the controller to the native game.",
                            );
                            deferred_owner = None;
                            stop_input_mappers(
                                &mut mouse,
                                &mut gyro,
                                &mut remap,
                                &mut xinput,
                                &proxy,
                            );
                            if let Some(mut active) = device.take() {
                                let _ = teardown(&mut active, &proxy);
                            }
                            emit(
                                &proxy,
                                json!({"type":"device","connected":false,"state":"handoff","reason":"native-feedback","message":"Controller released for native game feedback"}),
                            );
                            emit(&proxy, json!({"type":"safetyReset"}));
                        } else {
                            next_scan = Instant::now();
                        }
                        emit(&proxy, json!({"type":"nativeHandoff","enabled":enabled}));
                        continue;
                    }
                    if native_handoff
                        && matches!(
                            command,
                            DeviceCommand::SelectDevice { .. } | DeviceCommand::Scan
                        )
                    {
                        emit(
                            &proxy,
                            json!({"type":"error","message":"Resume controller monitoring before selecting a device"}),
                        );
                        continue;
                    }
                    if matches!(command, DeviceCommand::Reset | DeviceCommand::Shutdown) {
                        game_link.stop();
                    }
                    let output_was_ready = device.as_ref().is_some_and(|d| d.audio.output_ready());
                    if !handle_command(
                        command,
                        &mut device,
                        &proxy,
                        &mut mouse,
                        &mut gyro,
                        &mut remap,
                        &mut xinput,
                        &mut telemetry,
                        &mut game_link,
                        &mut script,
                        &screen,
                        &mut telemetry_stale_latched,
                        &mut api,
                        &mut preferred,
                    ) {
                        return;
                    }
                    if output_was_ready && !device.as_ref().is_some_and(|d| d.audio.output_ready())
                    {
                        cancel_audio_commands(
                            &mut deferred_audio,
                            &proxy,
                            "Controller output reset before pending audio started.",
                        );
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    stop_input_mappers(&mut mouse, &mut gyro, &mut remap, &mut xinput, &proxy);
                    if let Some(device) = device.as_mut() {
                        let _ = device.reset();
                    }
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        for update in game_link.poll(
            device.is_some(),
            device.as_ref().and_then(|d| d.last_battery),
        ) {
            match update {
                Ok(update) => {
                    if let Some(d) = device.as_mut() {
                        if update.reset {
                            let _ = d.restore_action();
                            if let Some(rgb) = d.last_light {
                                let _ = d.write_light(rgb);
                            } else {
                                let _ = d.release_lightbar();
                            }
                        }
                        if let Some(mask) = update.player_leds {
                            let _ = d.write(protocol::player_leds_report(mask));
                            d.player_leds_active = true;
                        }
                        if let Some(mode) = update.microphone_led {
                            let mut report =
                                protocol::usb_report(0, 1, 0, None, None, None, None, false);
                            report[9] = mode;
                            // LED only: it must not change the tracked mute state.
                            let _ = d.write(report);
                        }
                        if update.triggers.iter().any(Option::is_some) {
                            let rule = ActionRule {
                                id: "dsx".into(),
                                name: "DSX mod".into(),
                                source: "event".into(),
                                button: 0,
                                event: "dsx".into(),
                                window_ms: 280,
                                cooldown_ms: 0,
                                condition_button: None,
                                condition_threshold: 0.5,
                                cancel_on_release: false,
                                cancel_event: String::new(),
                                retrigger: "restart".into(),
                                duration_ms: 15000,
                                haptic: None,
                                left_trigger: update.triggers[0].clone(),
                                right_trigger: update.triggers[1].clone(),
                                timeline: None,
                            };
                            let _ = d.play_action(&rule);
                        }
                        if let Some(rgb) = update.rgb {
                            // Game-link color owns the lightbar; stop a running animation.
                            d.anim = None;
                            let _ = d.set_light(rgb);
                        }
                        for event in &update.events {
                            let rules = d.game_actions.event(event, Instant::now());
                            d.cancel_actions();
                            for rule in rules {
                                let _ = d.play_action(&rule);
                            }
                        }
                    }
                    if update.format.is_some()
                        || update.packet.is_some()
                        || update.weapon.is_some()
                        || !update.events.is_empty()
                    {
                        emit(
                            &proxy,
                            json!({"type":"gameEvent","weapon":update.weapon,"events":update.events,"packet":update.packet,"format":update.format}),
                        );
                    }
                    if let Some(status) = update.recognition_status {
                        emit(
                            &proxy,
                            json!({"type":"weaponRecognition","data":status,"clear":update.weapon_reset}),
                        );
                    }
                }
                Err(message) => {
                    if Instant::now() >= next_game_error {
                        emit(&proxy, json!({"type":"gameLinkError","message":message}));
                        next_game_error = Instant::now() + Duration::from_secs(2);
                    }
                }
            }
        }
        if let Some(tick) = telemetry.poll() {
            telemetry_stale_latched = false;
            let mut scripted = None;
            if tick.emit_ui
                && let Some(program) = script.as_ref()
            {
                match program.evaluate(tick.sample) {
                    Ok(effect) => {
                        if effect.light.is_some() || effect.cue.is_some() {
                            let cue = effect.cue.map(|value| match value {
                                ScriptCue::Pulse => "pulse",
                                ScriptCue::Grit => "grit",
                                ScriptCue::Shift => "shift",
                            });
                            emit(
                                &proxy,
                                json!({"type":"scriptOutput","light":effect.light,"cue":cue}),
                            );
                        }
                        scripted = Some(effect);
                    }
                    Err(error) => {
                        script = None;
                        emit(
                            &proxy,
                            json!({"type":"scriptStatus","enabled":false,"message":error}),
                        );
                        emit(&proxy, json!({"type":"error","message":error}));
                    }
                }
            }
            let color = scripted
                .and_then(|effect| effect.light)
                .unwrap_or_else(|| telemetry::rev_color(tick.sample));
            if tick.emit_ui {
                emit(
                    &proxy,
                    json!({"type":"telemetry","rpm":tick.sample.rpm.round(),"maxRpm":tick.sample.max_rpm.round(),"speedKmh":tick.sample.speed_mps.map(|v|(v*3.6).round()),"gear":tick.sample.gear,"brake":tick.sample.brake,"slip":tick.sample.slip,"kerb":tick.sample.kerb,"source":tick.sample.source,"revColor":color,"estimatedBrakeSlip":tick.sample.brake_slip()}),
                );
            }
            if let Some(connected) = device.as_mut() {
                if tick.emit_ui
                    && connected.last_light != Some(color)
                    && connected.write_light(color).is_ok()
                {
                    connected.battery_light = false;
                }
                if let Some(cue) = scripted.and_then(|effect| effect.cue) {
                    let due =
                        last_script_cue.is_none_or(|at| at.elapsed() >= Duration::from_millis(500));
                    if due {
                        let (shape, frequency, attack, release, strength) = match cue {
                            ScriptCue::Pulse => ("sine", 125, 4, 90, 25),
                            ScriptCue::Grit => ("noise", 95, 4, 100, 22),
                            ScriptCue::Shift => ("saw", 115, 3, 85, 35),
                        };
                        connected.play_feedback(shape, frequency, attack, release, strength, 0);
                        last_script_cue = Some(Instant::now());
                    }
                } else if let Some(cue) = tick.cue {
                    let (shape, frequency, attack, release, strength, pan) = match cue {
                        TelemetryCue::Shift => ("saw", 115, 3, 85, 55, 0),
                        TelemetryCue::BrakeSlip => ("square", 135, 2, 65, 32, 0),
                        TelemetryCue::KerbLeft => ("noise", 90, 4, 95, 28, -80),
                        TelemetryCue::KerbRight => ("noise", 90, 4, 95, 28, 80),
                        TelemetryCue::KerbBoth => ("noise", 90, 4, 95, 28, 0),
                        TelemetryCue::Slip => ("saw", 70, 6, 130, 20, 0),
                    };
                    let clip = VoiceClip::new(shape, frequency, attack, release, strength, pan);
                    if !matches!(connected.transport, BusType::Usb)
                        || connected.play_voice(clip).is_err()
                    {
                        let fallback = HapticClip::new(
                            shape.into(),
                            frequency,
                            attack,
                            release,
                            strength,
                            pan,
                        );
                        connected.rumble_until = Some(Instant::now() + fallback.duration());
                        connected.haptic = Some(fallback);
                    }
                }
            }
        }
        if let Some(result) = screen.take()
            && let Some(connected) = device.as_mut()
            && connected.screen_light
        {
            match result {
                Ok(raw) => {
                    let color = screen_color::smooth(connected.screen_color, raw);
                    connected.screen_color = Some(color);
                    if connected.last_light != Some(color) && connected.write_light(color).is_ok() {
                        emit(&proxy, json!({"type":"screenColor","rgb":color}));
                    }
                }
                Err(error) => {
                    screen.set_enabled(false);
                    connected.screen_light = false;
                    connected.screen_color = None;
                    let _ = connected.release_lightbar();
                    emit(
                        &proxy,
                        json!({"type":"lightMode","battery":false,"screen":false}),
                    );
                    emit(&proxy, json!({"type":"error","message":error}));
                }
            }
        }
        if !disable_hid && device.is_some() && Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(2);
            if api.refresh_devices().is_ok()
                && let Some(current) = device.as_ref()
            {
                let present = api
                    .device_list()
                    .any(|info| is_controller(info) && info.path().to_bytes() == current.path);
                if !present || current.last_valid_report.elapsed() > Duration::from_secs(4) {
                    let message = if present {
                        "Stream stalled — reconnecting"
                    } else {
                        "Disconnected"
                    };
                    drop_device(
                        &mut device,
                        &mut mouse,
                        &mut gyro,
                        &mut remap,
                        &mut xinput,
                        &screen,
                        &proxy,
                        message,
                    );
                    next_scan = Instant::now();
                } else if matches!(current.transport, BusType::Bluetooth)
                    && api
                        .device_list()
                        .any(|info| is_controller(info) && matches!(info.bus_type(), BusType::Usb))
                    // Only hand over to a USB pad that is the same physical controller.
                    && let Ok(Some(mut found)) = open_filtered(
                        &api,
                        &Preference {
                            path: None,
                            controller_id: Some(current.controller_id.clone()),
                        },
                        true,
                        true,
                    )
                    && found.path != current.path
                {
                    if preferred.controller_id.is_some() {
                        preferred.pin(&found);
                    }
                    stop_input_mappers(&mut mouse, &mut gyro, &mut remap, &mut xinput, &proxy);
                    screen.set_enabled(false);
                    if let Some(old) = device.as_mut() {
                        let _ = teardown(old, &proxy);
                    }
                    emit(&proxy, json!({"type":"lightMode","battery":false}));
                    emit(
                        &proxy,
                        json!({"type":"triggerLease","active":false,"durationMs":0}),
                    );
                    emit_connected(&proxy, &found);
                    found.restore_remembered(&proxy);
                    emit_device_list(&api, &proxy, Some(&found.path_display));
                    if let Ok(mut live) = LIVE_BATTERY.lock() {
                        *live = None;
                    }
                    device = Some(found);
                }
            }
            emit_device_list_if_changed(
                &api,
                &proxy,
                preferred
                    .path
                    .as_deref()
                    .or(device.as_ref().map(|d| d.path_display.as_str())),
            );
        }
        remap.tick();
        if let Some(connected) = device.as_mut() {
            for event in connected.pending_events.drain(..) {
                emit(&proxy, event);
            }
            match connected.tick_link_experiment() {
                Ok(Some(event)) => emit(&proxy, event),
                Ok(None) => {}
                Err(error) => {
                    let _ = connected.stop_link_experiment();
                    emit(
                        &proxy,
                        json!({"type":"linkExperiment","running":false,"error":error}),
                    );
                    emit(&proxy, json!({"type":"error","message":error}));
                }
            }
            if let Err(error) = connected.tick_bt_audio() {
                connected.audio.set_bluetooth(false);
                let _ = connected.write_bt_audio(&[0; 64]);
                emit(
                    &proxy,
                    json!({"type":"bluetoothAudioStatus","controllerId":connected.controller_id,"enabled":false,"error":error}),
                );
            }
            let feedback = if xinput.active() {
                match xinput.take_feedback() {
                    Ok(motors) => motors,
                    Err(message) => {
                        xinput.shutdown();
                        emit(&proxy, xinput.status_event());
                        emit(&proxy, json!({"type":"error","message":message}));
                        Some([0, 0])
                    }
                }
            } else if connected.game_rumble != [0, 0] {
                Some([0, 0])
            } else {
                None
            };
            // The audio side expires game rumble after 10 s without an update, so keep
            // steady non-zero rumble alive while the virtual pad is up.
            if feedback.is_some_and(|m| m != [0, 0]) {
                connected.game_feedback_at = Some(Instant::now());
            }
            let action = if feedback.is_none()
                && xinput.active()
                && connected.game_rumble != [0, 0]
                && !connected.input_stale
            {
                rumble_keepalive(
                    connected.game_feedback_at,
                    connected.game_refresh_at,
                    Instant::now(),
                )
            } else {
                RumbleKeepalive::Wait
            };
            let refresh = action == RumbleKeepalive::Refresh;
            let feedback = match action {
                RumbleKeepalive::Refresh => Some(connected.game_rumble),
                // The game stopped talking (killed or hung) while its last rumble was non-zero.
                RumbleKeepalive::Expire => Some([0, 0]),
                RumbleKeepalive::Wait => feedback,
            };
            if feedback.is_some() {
                connected.game_refresh_at = Some(Instant::now());
            }
            if let Some(motors) = feedback {
                let motors = if connected.input_stale {
                    [0, 0]
                } else {
                    motors
                };
                let usb = matches!(connected.transport, BusType::Usb)
                    || connected.audio.bluetooth_enabled();
                let recovering = usb
                    && motors != [0, 0]
                    && connected
                        .game_audio_retry_at
                        .is_some_and(|at| Instant::now() < at);
                let suppressed = !usb && connected.haptic.is_some();
                let result = if recovering {
                    Err("USB game feedback is waiting to retry".into())
                } else if usb {
                    if motors != [0, 0]
                        && (connected.game_rumble == [0, 0] || connected.audio.endpoint().is_none())
                    {
                        connected
                            .enable_pcm()
                            .and_then(|_| connected.audio.set_game_rumble(motors))
                    } else {
                        connected.audio.set_game_rumble(motors)
                    }
                } else if connected.haptic.is_none() {
                    connected.write_rumble(motors).map(|_| {
                        connected.rumble_until = Some(Instant::now() + Duration::from_secs(10));
                    })
                } else {
                    Ok(())
                };
                connected.game_rumble = motors;
                if !refresh {
                    emit(
                        &proxy,
                        json!({"type":"gameFeedback","motors":motors,"delivered":result.is_ok() && !suppressed,"suppressed":suppressed}),
                    );
                }
                if let Err(message) = result
                    && !recovering
                {
                    if usb {
                        connected.game_audio_retry_at =
                            Some(Instant::now() + Duration::from_secs(10));
                    }
                    emit(&proxy, json!({"type":"error","message":message}));
                } else if !recovering && motors != [0, 0] {
                    connected.game_audio_retry_at = None;
                }
            }
            connected.desktop.tick();
            connected.radial.tick();
            if !xinput.active() {
                connected.audio.release_idle_output();
            }
        }
        if let Some(connected) = device.as_mut() {
            match connected.tick_timeline() {
                Ok(Some(event)) => emit(&proxy, event),
                Err(message) => emit(&proxy, json!({"type":"error","message":message})),
                _ => {}
            }
            if connected
                .speaker_route_until
                .is_some_and(|at| Instant::now() >= at)
                && let Err(error) = connected.set_speaker_route(false)
            {
                emit(&proxy, json!({"type":"error","message":error}));
            }
            connected.tick_light_animation();
            if connected
                .haptic_timer
                .is_some_and(|at| Instant::now() >= at)
            {
                connected.haptic_timer = None;
                connected.play_feedback("sine", 95, 8, 250, 48, 0);
                emit(&proxy, json!({"type":"hapticAlertStatus","timerSeconds":0}));
                emit(
                    &proxy,
                    json!({"type":"hapticAlert","message":"Haptic timer finished"}),
                );
            }
            if connected.action_until.is_some_and(|t| Instant::now() >= t)
                && let Err(message) = connected.restore_action()
            {
                emit(&proxy, json!({"type":"error","message":message}));
            }
            if connected
                .input_effect_until
                .is_some_and(|t| Instant::now() >= t)
                && let Err(error) = connected.restore_input_effect()
            {
                emit(&proxy, json!({"type":"error","message":error}));
            }
            if connected
                .trigger_until
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                let off = TRIGGER_OFF;
                let held = connected
                    .timeline
                    .as_ref()
                    .map_or(0, |t| t.clip.trigger_mask());
                let mask = 12 & !held;
                let report = protocol::usb_report(
                    mask,
                    0,
                    0,
                    (mask & 4 != 0).then_some(off),
                    (mask & 8 != 0).then_some(off),
                    None,
                    None,
                    false,
                );
                if mask == 0 || connected.write(report).is_ok() {
                    connected.trigger_active = held != 0;
                    connected.trigger_until = None;
                    connected.last_trigger = None;
                    connected.bows = [None, None];
                    emit(
                        &proxy,
                        json!({"type":"triggerLease","active":false,"durationMs":0}),
                    );
                    emit(
                        &proxy,
                        json!({ "type": "notice", "message": "Trigger lease ended" }),
                    );
                }
            }
            if let Some(clip) = connected.haptic.as_mut() {
                if clip.due() {
                    match clip.sample_motors() {
                        Some(motors) => {
                            let _ = connected.write_rumble(motors.map(percent_to_motor));
                        }
                        None => {
                            let _ = connected.write_rumble([0, 0]);
                            connected.haptic = None;
                            connected.rumble_until = None;
                        }
                    }
                }
            } else if connected
                .rumble_until
                .is_some_and(|deadline| Instant::now() >= deadline)
                && connected.write_rumble([0, 0]).is_ok()
            {
                connected.rumble_until = None;
            }
            let read_started = Instant::now();
            let read_result = connected.hid.read_timeout(
                &mut input_report,
                if connected.timeline.is_some() || connected.audio.bluetooth_enabled() {
                    5
                } else {
                    20
                },
            );
            if diagnostics {
                metrics
                    .read_wait
                    .record(crate::runtime_diagnostics::elapsed_us(read_started));
            }
            match read_result {
                Ok(length) if length > 0 => {
                    recorder.input(&input_report[..length]);
                    let input_started = Instant::now();
                    let parsed = protocol::parse_input(&input_report[..length]);
                    if diagnostics {
                        metrics
                            .decode
                            .record(crate::runtime_diagnostics::elapsed_us(input_started));
                    }
                    if let Some(mut input) = parsed {
                        if input.full_report {
                            connected.last_battery = input.battery;
                            connected.track_status(&input.battery_info, &proxy);
                            if connected.microphone_muted.is_none() {
                                connected.microphone_muted =
                                    Some(input.battery_info.microphone_muted);
                                emit(
                                    &proxy,
                                    json!({"type":"microphoneStatus","muted":input.battery_info.microphone_muted}),
                                );
                            }
                        } else if !connected.battery_unknown_sent {
                            // Short Bluetooth reports carry no battery or jack state.
                            connected.battery_unknown_sent = true;
                            emit(
                                &proxy,
                                json!({"type":"battery","controllerId":connected.controller_id,"available":false,"reason":"short-report","level":null,"charging":false,"faults":[]}),
                            );
                        }
                        if input.buttons[18] > 0.5 && !connected.last_input_buttons[18] {
                            let muted = !connected.microphone_muted.unwrap_or(false);
                            match connected.set_microphone_mute(muted) {
                                Ok(()) => {
                                    emit(&proxy, json!({"type":"microphoneStatus","muted":muted}))
                                }
                                Err(error) => emit(&proxy, json!({"type":"error","message":error})),
                            }
                        }
                        connected.last_valid_report = Instant::now();
                        raw_reports += 1;
                        if input.full_report {
                            let (warn, critical) = connected.battery_thresholds;
                            let clear = warn.saturating_add(10).min(100);
                            if input.charging || input.battery.is_some_and(|level| level >= clear) {
                                connected.low_battery_latched = false;
                                connected.low_battery_critical_latched = false;
                            } else if connected.low_battery_alert
                                && let Some(level) = input.battery
                            {
                                if level <= critical && !connected.low_battery_critical_latched {
                                    connected.low_battery_critical_latched = true;
                                    connected.low_battery_latched = true;
                                    connected.play_feedback("square", 155, 3, 160, 45, 0);
                                    emit(
                                        &proxy,
                                        json!({"type":"hapticAlert","level":"critical","message":format!("Battery critical: {level}%")}),
                                    );
                                } else if level <= warn
                                    && level > critical
                                    && !connected.low_battery_latched
                                {
                                    connected.low_battery_latched = true;
                                    connected.play_feedback("square", 155, 3, 120, 32, 0);
                                    emit(
                                        &proxy,
                                        json!({"type":"hapticAlert","level":"warn","message":format!("Battery low: {level}%")}),
                                    );
                                }
                            }
                        }
                        if let Some(sequence) = input.sequence {
                            if let Some(previous) = connected.input_sequence {
                                let delta = sequence.wrapping_sub(previous);
                                if (2..128).contains(&delta) {
                                    sequence_gaps += u32::from(delta - 1);
                                }
                            }
                            connected.input_sequence = Some(sequence);
                        }
                        if connected.battery_light
                            && let Some(battery) = input.battery
                        {
                            let color = if battery > 70 {
                                [77, 202, 177]
                            } else if battery > 30 {
                                [255, 159, 76]
                            } else {
                                [255, 65, 77]
                            };
                            let _ = connected.set_light(color);
                        }
                        if connected.sensor_light
                            && let Some(sensor_byte) = input.battery_info.sensor_byte
                        {
                            let color = sensor_color(sensor_byte);
                            let changed = connected.last_sensor != Some(sensor_byte);
                            let _ = connected.set_light(color);
                            if changed && connected.last_light == Some(color) {
                                connected.last_sensor = Some(sensor_byte);
                                emit(
                                    &proxy,
                                    json!({"type":"sensorColor","sensorByte":sensor_byte,"rgb":color}),
                                );
                            }
                        }
                        let raw_axes = input.axes;
                        let raw_triggers = [input.buttons[6], input.buttons[7]];
                        let (mut game_input, corrected_axes) = crate::output_calibration::project(
                            &input,
                            connected.output_calibration.as_ref(),
                            (!connected.calibration_measuring
                                && connected.output_calibration_error.is_none())
                            .then_some(&connected.calibration),
                            &connected.controls.sticks,
                        );
                        if connected.input_stale {
                            connected.input_stale = false;
                            emit(
                                &proxy,
                                json!({"type":"outputInputHealth","stale":false,"controllerId":connected.controller_id}),
                            );
                        }
                        let motion = connected.motion.update(&input, Instant::now());
                        if motion.calibration_complete {
                            emit(
                                &proxy,
                                json!({"type":"motionCalibration","status":"complete","controllerId":connected.controller_id,"biasDps":motion.bias_dps}),
                            );
                        }
                        if connected.last_motion_event.elapsed() >= Duration::from_millis(100)
                            || motion.calibration_complete
                        {
                            emit(&proxy, json!({"type":"motion","reading":motion}));
                            connected.last_motion_event = Instant::now();
                        }
                        connected.calibration.apply(&mut input);
                        let (radial, radial_cue) = connected.radial.update(
                            &input,
                            connected.desktop.enabled(),
                            connected.desktop.ui_focused(),
                        );
                        if radial.changed || radial.committed.is_some() || radial.cancelled {
                            emit(
                                &proxy,
                                json!({"type":"radialMenu","open":connected.radial.state.open,"selected":radial.selected,"committed":radial.committed,"cancelled":radial.cancelled,"consume":radial.consume,"preview":connected.desktop.ui_focused()}),
                            );
                        }
                        if radial_cue {
                            connected.play_feedback("sine", 170, 2, 35, 12, 0);
                        }
                        let touch_cue = if radial.consume {
                            mouse.cancel_pointer();
                            connected.desktop.cancel_for_modal();
                            None
                        } else {
                            let cue = mouse.update(&input);
                            connected.desktop.update(&input);
                            cue
                        };
                        // Flick output is suspended while the UI has focus, PC control owns the
                        // pointer, or the radial menu consumes the stick.
                        let flick_suspended = connected.desktop.ui_focused()
                            || connected.desktop.enabled()
                            || radial.consume;
                        gyro.update(&input, &motion, &connected.controls.motion, flick_suspended);
                        remap.set_suspended(flick_suspended);
                        remap.update(&input);
                        if (gyro.config.enabled || gyro.config.flick_without_gyro)
                            && motion.valid
                            && connected.controls.motion.flick_enabled
                            && !flick_suspended
                        {
                            game_input.axes[2] = 0.0;
                            game_input.axes[3] = 0.0;
                        }
                        // Buttons a consuming binding handles are hidden from the virtual pad.
                        for (index, consumed) in remap
                            .consumed()
                            .into_iter()
                            .zip(connected.desktop.consumed_buttons())
                            .map(|(a, b)| a || b)
                            .enumerate()
                        {
                            if consumed && let Some(value) = game_input.buttons.get_mut(index) {
                                *value = 0.0;
                            }
                        }
                        let suspension = (
                            connected.desktop.suspension_reason(),
                            gyro.inactive_reason()
                                .filter(|_| gyro.config.enabled || gyro.config.flick_without_gyro),
                        );
                        if suspension != last_suspension {
                            last_suspension = suspension;
                            emit(
                                &proxy,
                                json!({"type":"suspended","controllerId":connected.controller_id,"pc":suspension.0,"gyro":suspension.1}),
                            );
                        }
                        if remap.take_queue_overflow() || connected.desktop.take_queue_overflow() {
                            emit(
                                &proxy,
                                json!({"type":"queueOverflow","message":"A macro was dropped because the key queue was full."}),
                            );
                        }
                        let virtual_axes = game_input.axes;
                        if let Some(message) = connected.desktop.take_launch_error() {
                            emit(&proxy, json!({"type":"error","message":message}));
                        }
                        if !connected.desktop.enabled() && !input.panic_combo {
                            let rules = connected
                                .game_actions
                                .update(&input.buttons, Instant::now());
                            connected.cancel_actions();
                            for rule in rules {
                                if radial.consume {
                                    continue;
                                }
                                match connected.play_action(&rule) {
                                    Ok(()) => {
                                        emit(&proxy, json!({"type":"gameAction","name":rule.name}))
                                    }
                                    Err(message) => {
                                        emit(&proxy, json!({"type":"error","message":message}))
                                    }
                                }
                            }
                        }
                        if let Some(event) = if input.panic_combo {
                            xinput.neutralize()
                        } else {
                            xinput.update(&game_input)
                        } {
                            emit(&proxy, event.clone());
                            if let Some(message) = event["message"].as_str() {
                                emit(&proxy, json!({ "type": "error", "message": message }));
                            }
                        }
                        if input.panic_combo && !connected.panic_latched {
                            cancel_audio_commands(
                                &mut deferred_audio,
                                &proxy,
                                "Controller reset cancelled pending audio.",
                            );
                            game_link.stop();
                            screen.set_enabled(false);
                            let _ = telemetry.set(false, 0, false);
                            script = None;
                            emit(
                                &proxy,
                                json!({"type":"scriptStatus","enabled":false,"message":"Script off"}),
                            );
                            emit(
                                &proxy,
                                json!({"type":"telemetryStatus","enabled":false,"message":"Telemetry bridge off"}),
                            );
                            emit(&proxy, json!({"type":"safetyReset"}));
                            stop_input_mappers(
                                &mut mouse,
                                &mut gyro,
                                &mut remap,
                                &mut xinput,
                                &proxy,
                            );
                            connected.battery_light = false;
                            emit(&proxy, json!({"type":"lightMode","battery":false}));
                            let _ = connected.reset();
                            emit(
                                &proxy,
                                json!({"type":"triggerLease","active":false,"durationMs":0}),
                            );
                            emit(
                                &proxy,
                                json!({ "type": "notice", "message": "PS + Options — reset" }),
                            );
                        }
                        connected.panic_latched = input.panic_combo;
                        if !input.panic_combo
                            && let Some(cue) = touch_cue
                        {
                            let pan = match cue {
                                TouchCue::Left => -85,
                                TouchCue::Right => 85,
                                TouchCue::Top => 0,
                            };
                            let clip = VoiceClip::new("sine", 180, 2, 55, 18, pan);
                            if !matches!(connected.transport, BusType::Usb)
                                || connected.play_voice(clip).is_err()
                            {
                                let fallback = HapticClip::new("sine".into(), 180, 2, 55, 18, pan);
                                connected.rumble_until = Some(Instant::now() + fallback.duration());
                                connected.haptic = Some(fallback);
                            }
                        }
                        if !input.panic_combo
                            && let Err(error) =
                                connected.update_bow_returns([input.buttons[6], input.buttons[7]])
                        {
                            emit(&proxy, json!({"type":"error","message":error}));
                        }
                        if !input.panic_combo
                            && let Err(error) = connected.update_input_effect(input.buttons[7])
                        {
                            emit(&proxy, json!({"type":"error","message":error}));
                        }
                        let mut button_changed = false;
                        for (previous, value) in
                            connected.last_input_buttons.iter_mut().zip(input.buttons)
                        {
                            let pressed = value > 0.5;
                            button_changed |= *previous != pressed;
                            *previous = pressed;
                        }
                        if button_changed
                            || connected.last_input_sent.elapsed() >= Duration::from_millis(32)
                        {
                            emit(
                                &proxy,
                                json!({ "type": "input", "controllerId":connected.controller_id, "input": input,"rawAxes":raw_axes,"correctedAxes":corrected_axes,"virtualAxes":virtual_axes,"rawTriggers":raw_triggers,"virtualTriggers":[game_input.buttons[6],game_input.buttons[7]],"virtualReport":crate::xinput_bridge::Encoded::from_input(&game_input),"virtualOutputActive":xinput.active()&&!input.panic_combo }),
                            );
                            connected.last_input_sent = Instant::now();
                        }
                    } else if matches!(input_report[0], 0x01 | 0x31) {
                        invalid_reports += 1;
                    }
                    if diagnostics {
                        metrics
                            .dispatch
                            .record(crate::runtime_diagnostics::elapsed_us(input_started));
                    }
                }
                Ok(_) => {
                    recorder.timeout();
                }
                Err(error) => {
                    recorder.read_error();
                    drop_device(
                        &mut device,
                        &mut mouse,
                        &mut gyro,
                        &mut remap,
                        &mut xinput,
                        &screen,
                        &proxy,
                        &format!("Controller disconnected: {error}"),
                    );
                    next_scan = Instant::now() + Duration::from_secs(1);
                }
            }
        } else {
            if !disable_hid
                && !native_handoff
                && !native_provider.owns_controller()
                && edge_worker.is_none()
                && !edge_busy()
                && Instant::now() >= next_scan
            {
                let _ = api.refresh_devices();
                // A pinned controller is never silently replaced by another pad.
                let opened = if preferred.controller_id.is_some() {
                    open_filtered(&api, &preferred, false, true)
                } else {
                    open_path(&api, &preferred)
                };
                match opened {
                    Ok(Some(mut found)) => {
                        let substituted = preferred
                            .controller_id
                            .as_deref()
                            .is_some_and(|id| id != found.controller_id);
                        if substituted {
                            emit(
                                &proxy,
                                json!({"type":"notice","code":"pinned-missing","message":"The selected controller is unavailable; using another connected DualSense"}),
                            );
                        } else if preferred.controller_id.is_some() {
                            // Same pad, possibly on a new transport/path: keep the pin current.
                            preferred.pin(&found);
                        }
                        emit_connected(&proxy, &found);
                        found.restore_remembered(&proxy);
                        emit_device_list(&api, &proxy, Some(&found.path_display));
                        if let Ok(mut live) = LIVE_BATTERY.lock() {
                            *live = None;
                        }
                        device = Some(found);
                    }
                    Ok(None) => {
                        let pinned = preferred.controller_id.is_some();
                        emit(
                            &proxy,
                            json!({ "type": "device", "connected": false, "state": if pinned {"pinned-missing"} else {"disconnected"}, "reason": if pinned {"pinned-missing"} else {"none-found"}, "message": "No DualSense found" }),
                        );
                        emit_device_list_if_changed(&api, &proxy, preferred.path.as_deref());
                    }
                    Err(error) => emit(
                        &proxy,
                        json!({ "type": "device", "connected": false, "state": "disconnected", "reason": "open-failed", "message": error }),
                    ),
                }
                next_scan = Instant::now() + Duration::from_secs(2);
            }
            thread::sleep(Duration::from_millis(35));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn game_rumble_keepalive_stops_after_the_game_goes_silent() {
        use super::{RumbleKeepalive as K, rumble_keepalive};
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let at = |s| t0 + Duration::from_secs(s);
        assert_eq!(rumble_keepalive(Some(t0), Some(t0), at(1)), K::Wait);
        assert_eq!(rumble_keepalive(Some(t0), Some(t0), at(3)), K::Refresh);
        assert_eq!(rumble_keepalive(Some(t0), None, at(3)), K::Refresh);
        // Refreshes do not count as real updates, so a dead game is cut off.
        assert_eq!(rumble_keepalive(Some(t0), Some(at(29)), at(31)), K::Expire);
        assert_eq!(rumble_keepalive(None, None, at(1)), K::Expire);
    }
    #[test]
    fn reset_rejects_pending_playback_and_checkpoint_before_they_can_restart() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut queue = std::collections::VecDeque::from([
            super::DeviceCommand::PlayAudioFile {
                path: "test.wav".into(),
                position: 0.0,
                duration: 5.0,
                route: "grips".into(),
                kind: "music".into(),
                track_id: 77,
            },
            super::DeviceCommand::ProfileCheckpoint { request_id: 78 },
        ]);
        super::cancel_audio_commands(&mut queue, &sender, "Controller reset");
        assert!(queue.is_empty());
        let events: Vec<_> = receiver.try_iter().collect();
        assert!(
            events.iter().any(|e| e["type"] == "speakerStatus"
                && e["trackId"] == 77
                && e["source"] == "error")
        );
        assert!(events.iter().any(|e| e["type"] == "profileCheckpoint"
            && e["requestId"] == 78
            && e["connected"] == false));
    }
    #[test]
    fn scoped_stop_removes_only_its_pending_audio_and_global_reset_cancels_startup() {
        let chunk = |track_id| super::DeviceCommand::PlaySpeakerSound {
            track_id,
            pcm_base64: "AAAAAA==".into(),
            route: "speaker".into(),
            append: false,
        };
        let stop = super::DeviceCommand::StopSpeakerSound { track_id: 42 };
        assert!(stop.bypasses_audio_wait());
        assert!(!stop.cancels_audio_wait());
        assert!(stop.cancels_queued(&chunk(42)));
        assert!(!stop.cancels_queued(&chunk(43)));
        assert!(super::DeviceCommand::Reset.cancels_audio_wait());
        assert!(!super::DeviceCommand::SetPcMode { enabled: false }.cancels_audio_wait());
    }

    use super::{
        BowReturn, DeviceCommand, InputEffect, KickStyle, TriggerSetting, WeaponKind,
        encode_live_trigger, sensor_color,
    };

    #[test]
    fn rumble_percent_scales_to_the_full_byte_range() {
        assert_eq!(super::percent_to_motor(0), 0);
        assert_eq!(super::percent_to_motor(100), 255);
        assert_eq!(super::percent_to_motor(50), 128);
        assert_eq!(super::percent_to_motor(250), 255);
    }

    #[test]
    fn full_write_accepts_padded_and_unknown_counts() {
        assert_eq!(super::full_write(0, 142), cfg!(windows));
        assert!(super::full_write(142, 142));
        assert!(super::full_write(143, 142));
        assert!(!super::full_write(20, 142));
    }

    #[test]
    fn disconnect_messages_map_to_structured_states() {
        assert_eq!(
            super::disconnect_state("Stream stalled \u{2014} reconnecting").0,
            "stalled"
        );
        assert_eq!(
            super::disconnect_state("Controller disconnected: x").0,
            "reconnecting"
        );
        assert_eq!(
            super::disconnect_state("Disconnected"),
            ("disconnected", "removed")
        );
    }

    #[test]
    fn candidate_rank_prefers_controller_id_then_path_then_usb() {
        let pref = super::Preference {
            path: Some("p1".into()),
            controller_id: Some("pad-a".into()),
        };
        let same_id_bt = super::candidate_rank(&pref, Some("pad-a"), "other", false);
        let other_id_same_path = super::candidate_rank(&pref, Some("pad-b"), "p1", true);
        let other = super::candidate_rank(&pref, None, "x", true);
        assert!(same_id_bt < other_id_same_path);
        assert!(other_id_same_path < other);
    }

    #[test]
    fn animations_follow_their_shapes() {
        use super::{LightAnimKind::*, animation_color};
        let base = [200, 100, 50];
        assert_eq!(animation_color(Pulse, base, 1000, 0, None, 20), base);
        assert!(animation_color(Breathe, base, 1000, 0, None, 20)[0] < 40);
        assert_eq!(
            animation_color(Rainbow, base, 1000, 0, None, 20),
            [255, 0, 0]
        );
        assert_eq!(
            animation_color(LowBatteryBlink, base, 1000, 100, Some(10), 20),
            [255, 40, 40]
        );
        assert_eq!(
            animation_color(LowBatteryBlink, base, 1000, 600, Some(10), 20),
            [0, 0, 0]
        );
        assert_eq!(
            animation_color(LowBatteryBlink, base, 1000, 600, Some(80), 20),
            base
        );
    }

    #[test]
    fn firmware_labels_and_build_strings_parse() {
        assert_eq!(super::feature_version_label(0x0215), "2.21");
        let mut report = [0u8; 64];
        report[1..12].copy_from_slice(b"Sep 20 2020");
        report[12..20].copy_from_slice(b"12:34:56");
        assert_eq!(
            super::firmware_build(&report).as_deref(),
            Some("Sep 20 2020 12:34:56")
        );
        assert_eq!(super::firmware_build(&[0u8; 64]), None);
    }
    use aftertouch_protocol as protocol;
    use std::time::{Duration, Instant};

    #[test]
    fn profile_checkpoints_preserve_bounded_request_ids() {
        let command: DeviceCommand =
            serde_json::from_str(r#"{"action":"profileCheckpoint","requestId":42}"#).unwrap();
        assert!(matches!(
            command,
            DeviceCommand::ProfileCheckpoint { request_id: 42 }
        ));
        for value in [-1i64, i64::from(u32::MAX) + 1] {
            assert!(
                serde_json::from_value::<DeviceCommand>(
                    serde_json::json!({"action":"profileCheckpoint","requestId":value})
                )
                .is_err()
            );
        }
    }

    #[test]
    fn bow_unloads_on_return_and_rearms_at_rest() {
        let mut bow = BowReturn::default();
        for pull in [0.0, 0.2, 0.6, 0.94] {
            assert_eq!(bow.update(pull), None);
        }
        assert_eq!(bow.update(0.96), Some(true));
        assert_eq!(bow.update(0.98), None);
        assert_eq!(bow.update(0.3), None);
        assert_eq!(bow.update(0.02), Some(false));
        assert_eq!(bow.update(0.5), None);
        assert_eq!(bow.update(0.4), Some(true));
        let encoded = encode_live_trigger(&TriggerSetting {
            mode: "bow".into(),
            strength: 90.0,
            curve: vec![1.0; 11],
            start_zone: Some(0),
            end_zone: Some(9),
            force_cap: Some(8),
            ..TriggerSetting::default()
        })
        .unwrap();
        assert_eq!(encoded[0], 0x21);
        let packed = u32::from_le_bytes(encoded[3..7].try_into().unwrap());
        assert!((packed & 7) < ((packed >> 27) & 7));
    }

    #[test]
    fn sniper_does_not_rearm_when_its_kick_moves_the_finger() {
        let now = Instant::now();
        let mut effect = InputEffect {
            threshold: 0.84,
            strength: 100,
            kind: WeaponKind::Sniper,
            rate: 3,
            spacing_ms: 0,
            kick_ms: 0,
            remaining: 0,
            kick_style: KickStyle::Return,
            tuning: protocol::BurstTuning::default(),
            armed: true,
            next_pulse: None,
            last_pulse: None,
            release_since: None,
        };
        assert!(effect.advance(now, 0.9));
        assert!(!effect.advance(now + Duration::from_millis(50), 0.5));
        assert!(!effect.advance(now + Duration::from_millis(150), 0.9));
        assert!(!effect.advance(now + Duration::from_millis(200), 0.1));
        assert!(effect.advance(now + Duration::from_millis(300), 0.9));
    }

    #[test]
    fn raw_sensor_byte_spans_the_color_palette() {
        assert_eq!(sensor_color(0), [60, 130, 200]);
        assert_eq!(sensor_color(127), [230, 165, 45]);
        assert_eq!(sensor_color(255), [220, 45, 50]);
    }

    #[test]
    fn burst_fires_three_spaced_kicks_at_partial_pull_and_rearms_at_rest() {
        let now = Instant::now();
        let mut burst = InputEffect {
            threshold: 0.77,
            strength: 100,
            kind: WeaponKind::Burst,
            rate: 4,
            spacing_ms: 251,
            kick_ms: 95,
            remaining: 0,
            kick_style: KickStyle::Return,
            tuning: protocol::BurstTuning::default(),
            armed: true,
            next_pulse: None,
            last_pulse: None,
            release_since: None,
        };
        assert!(!burst.advance(now, 0.76));
        assert!(burst.advance(now, 0.78));
        let first = burst.trigger_effect().unwrap();
        assert_eq!(first[0], 0x22);
        assert_eq!(first[3] & 7, 7); // full stop force
        assert_eq!((first[3] >> 3) & 7, 7); // independent full return force
        assert_eq!(burst.burst_kick_ms(), 95);
        assert!(!burst.advance(now + Duration::from_millis(50), 0.0));
        assert!(!burst.advance(now + Duration::from_millis(250), 0.78));
        // A motor kick can move the finger below the firing point without cancelling the train.
        assert!(burst.advance(now + Duration::from_millis(251), 0.5));
        assert_eq!(burst.trigger_effect().unwrap(), first);
        assert!(!burst.advance(now + Duration::from_millis(275), 0.0));
        assert!(!burst.advance(now + Duration::from_millis(501), 0.78));
        assert!(burst.advance(now + Duration::from_millis(502), 0.78));
        assert_eq!(burst.trigger_effect().unwrap(), first);
        assert!(!burst.advance(now + Duration::from_millis(753), 0.78));
        assert!(!burst.advance(now + Duration::from_millis(1004), 0.78));
        assert!(!burst.advance(now + Duration::from_millis(1100), 0.0));
        assert!(!burst.advance(now + Duration::from_millis(1160), 0.0));
        assert!(burst.advance(now + Duration::from_millis(1200), 0.78));
        assert!(!burst.advance(now + Duration::from_millis(1250), 0.0));
        assert!(!burst.advance(now + Duration::from_millis(1370), 0.0));
        assert!(!burst.advance(now + Duration::from_millis(1430), 0.0));
        assert!(!burst.advance(now + Duration::from_millis(1451), 0.0));
        let mut smg = InputEffect {
            kind: WeaponKind::Smg,
            rate: 6,
            ..burst
        };
        smg.armed = true;
        smg.next_pulse = None;
        assert!(smg.advance(now, 0.8));
        assert!(smg.advance(now + Duration::from_millis(167), 0.8));
        assert!(!smg.advance(now + Duration::from_millis(334), 0.0));
        assert!(smg.advance(now + Duration::from_millis(500), 0.8));
    }

    #[test]
    fn burst_styles_use_distinct_motor_effects_and_old_commands_get_a_default() {
        let command: DeviceCommand = serde_json::from_value(serde_json::json!({
            "action":"setInputEffects","enabled":true,"threshold":0.77,"strength":100,
            "kind":"burst","rate":4
        }))
        .unwrap();
        let DeviceCommand::SetInputEffects { kick_style, .. } = command else {
            panic!("wrong command")
        };
        assert_eq!(kick_style, "return");
        let mut effect = InputEffect {
            kind: WeaponKind::Burst,
            threshold: 0.77,
            strength: 100,
            rate: 4,
            spacing_ms: 251,
            kick_ms: 95,
            remaining: 0,
            kick_style: KickStyle::Strike,
            tuning: protocol::BurstTuning::default(),
            armed: true,
            next_pulse: None,
            last_pulse: None,
            release_since: None,
        };
        let strike = effect.trigger_effect().unwrap();
        assert_eq!(strike[0], 0x26);
        assert_eq!(strike[9], 4);
        assert_eq!(u16::from_le_bytes([strike[1], strike[2]]), 0x3fc);
        assert_eq!(effect.pulse_interval(), Duration::from_millis(250));
        effect.kick_style = KickStyle::Stop;
        let stop = effect.trigger_effect().unwrap();
        assert_eq!(stop[0], 0x21);
        assert_eq!(
            u32::from_le_bytes(stop[3..7].try_into().unwrap()) >> 27 & 7,
            7
        );
    }

    #[test]
    fn native_burst_at_ninety_percent_runs_once_with_three_grip_pulses() {
        let now = Instant::now();
        let mut effect = InputEffect {
            kind: WeaponKind::Burst,
            threshold: 0.9,
            strength: 100,
            rate: 4,
            spacing_ms: 250,
            kick_ms: 95,
            remaining: 0,
            kick_style: KickStyle::Strike,
            tuning: protocol::BurstTuning {
                haptic_strength: 85,
                ..protocol::BurstTuning::default()
            },
            armed: true,
            next_pulse: None,
            last_pulse: None,
            release_since: None,
        };
        assert!(!effect.advance(now, 0.899));
        assert!(effect.advance(now, 0.9));
        assert_eq!(effect.trigger_duration(), Some(Duration::from_millis(750)));
        let encoded = effect.trigger_effect().unwrap();
        assert_eq!(&encoded[..7], &[0x26, 0xfc, 0x03, 0xc0, 0xff, 0xff, 0x3f]);
        assert_eq!(encoded[9], 4);
        assert_eq!(effect.tuning.haptic_strength, 85);
        assert!(!effect.advance(now + Duration::from_millis(249), 0.92));
        assert!(effect.advance(now + Duration::from_millis(250), 0.92));
        assert_eq!(effect.trigger_duration(), None);
        assert!(effect.advance(now + Duration::from_millis(500), 0.92));
        assert_eq!(effect.trigger_duration(), None);
        assert!(!effect.advance(now + Duration::from_millis(750), 0.92));
        // Releasing during another train cancels its remaining scheduled pulses.
        assert!(!effect.advance(now + Duration::from_millis(800), 0.0));
        assert!(!effect.advance(now + Duration::from_millis(860), 0.0));
        assert!(effect.advance(now + Duration::from_millis(900), 0.91));
        assert!(!effect.advance(now + Duration::from_millis(1030), 0.0));
        assert!(!effect.advance(now + Duration::from_millis(1090), 0.0));
        assert_eq!(effect.remaining, 0);
        assert!(effect.armed);
        assert!(!effect.advance(now + Duration::from_millis(1150), 0.0));
    }

    #[test]
    fn sniper_load_keeps_a_light_ramp_full_stop_and_clear_release() {
        let encoded = encode_live_trigger(&TriggerSetting {
            mode: "weapon".into(),
            strength: 100.0,
            curve: vec![1.0; 11],
            start_zone: Some(5),
            end_zone: Some(6),
            force_cap: Some(8),
            preload_strength: Some(15.0),
            ..TriggerSetting::default()
        })
        .unwrap();
        assert_eq!(encoded[0], 0x21);
        let mask = u16::from_le_bytes([encoded[1], encoded[2]]);
        assert_eq!(mask & (1 << 5), 1 << 5);
        assert_eq!(mask & (0b1111 << 6), 0);
        let packed = u32::from_le_bytes(encoded[3..7].try_into().unwrap());
        assert_eq!((packed >> 15) & 7, 7);
        assert_eq!(packed & 7, 0);
        assert_eq!((packed >> 12) & 7, 0);
    }

    #[test]
    fn ui_camel_case_field_names_reach_the_core() {
        // The UI sends camelCase; these used to be rejected or silently dropped.
        let reload: DeviceCommand = serde_json::from_value(
            serde_json::json!({"action":"reloadOutputCalibration","controllerId":"pad-1"}),
        )
        .expect("controllerId must be accepted");
        assert!(matches!(
            reload,
            DeviceCommand::ReloadOutputCalibration { controller_id } if controller_id == "pad-1"
        ));
        let gyro: DeviceCommand = serde_json::from_value(serde_json::json!({
            "action":"setGyroAim","enabled":true,"sensitivity":1.0,
            "yawScale":1.5,"pitchScale":0.5,"invertY":true,"activation":"l2"
        }))
        .expect("camelCase gyro fields must be accepted");
        match gyro {
            DeviceCommand::SetGyroAim {
                yaw_scale,
                pitch_scale,
                invert_y,
                ..
            } => {
                assert_eq!(yaw_scale, 1.5);
                assert_eq!(pitch_scale, 0.5);
                assert!(invert_y, "Invert pitch must not be dropped");
            }
            other => panic!("unexpected command {other:?}"),
        }
        // Old snake_case callers keep working.
        assert!(serde_json::from_value::<DeviceCommand>(
            serde_json::json!({"action":"setGyroAim","enabled":false,"invert_y":true,"activation":"l2"})
        )
        .is_ok());
    }

    #[test]
    fn new_commands_deserialize_and_ignore_the_persistent_flag() {
        use serde_json::json;
        let parse = |v: serde_json::Value| serde_json::from_value::<super::DeviceCommand>(v);
        assert!(
            parse(json!({"action":"setBridgeLatency","value":"low","persistent":true})).is_ok()
        );
        assert!(
            parse(json!({"action":"setHapticOutputLevel","level":"max","persistent":true})).is_ok()
        );
        assert!(parse(json!({"action":"setHapticOutputLevel"})).is_err());
        assert!(parse(json!({"action":"setBridgeCustom","cutoff":180.0,"bass":0.7,"transient":0.2,"gate":0.0})).is_ok());
        assert!(parse(json!({"action":"setBridgeOptions","mode":"threeBand","bandGains":[1,1,0.5],"onset":0.2,"panEmphasis":1})).is_ok());
        assert!(parse(json!({"action":"setProcessReattach","enabled":true})).is_ok());
        assert!(
            parse(json!({"action":"previewEnvelope","path":"a.wav","kind":"bands:1,1,1"})).is_ok()
        );
        assert!(parse(json!({"action":"setRemapTiming","holdMs":40,"gapMs":20})).is_ok());
        assert!(
            parse(
                json!({"action":"setPadExtras","touchpadClick":"a","mic":null,"persistent":true})
            )
            .is_ok()
        );
        assert!(parse(json!({"action":"setScreenLightbar","enabled":true,"monitor":1})).is_ok());
        assert!(parse(json!({"action":"setTouchMouse","enabled":true,"sensitivity":1.2,"acceleration":0.3})).is_ok());
        let gyro = parse(json!({"action":"setGyroAim","enabled":true,"gate":"toggle","stickOffThreshold":0.4,"smoothThreshold":30,"acceleration":1,"accelThreshold":60,"flickWithoutGyro":true,"persistent":true})).unwrap();
        assert!(matches!(
            gyro,
            super::DeviceCommand::SetGyroAim {
                flick_without_gyro: true,
                ..
            }
        ));
        let remap = parse(json!({"action":"setRemap","enabled":true,"persistent":true,"bindings":[
            {"button":1,"vk":65,"modifier":4,"modifiers":[5],"layerOnly":true,"longPressMs":400,"turboMs":0,"consume":true}]})).unwrap();
        let super::DeviceCommand::SetRemap { bindings, .. } = remap else {
            panic!()
        };
        assert_eq!(bindings[0].extras.modifiers, vec![5]);
        assert!(bindings[0].extras.consume);
        let old =
            parse(json!({"action":"setRemap","enabled":true,"bindings":[{"button":1,"vk":65}]}))
                .unwrap();
        assert!(matches!(old, super::DeviceCommand::SetRemap { .. }));
        let pc = parse(
            json!({"action":"setPcBindings","bindings":[],"click_threshold":0.5,"pointer_sensitivity":1.0,"reverse_scroll":true,"pointer_acceleration":0.2,"left_stick":"wasd"}),
        );
        assert!(pc.is_ok());
    }
}
