use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
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

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum DeviceCommand {
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
    SetGameActions {
        enabled: bool,
        rules: Vec<ActionRule>,
    },
    TestGameAction {
        rule: ActionRule,
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
    },
    SetTouchMouse {
        enabled: bool,
    },
    SetTouchGestures {
        enabled: bool,
    },
    SetGyroAim {
        enabled: bool,
        #[serde(default = "default_sens")]
        sensitivity: f32,
        #[serde(default = "default_axis")]
        yaw_scale: f32,
        #[serde(default = "default_axis")]
        pitch_scale: f32,
        #[serde(default)]
        invert_y: bool,
        #[serde(default = "default_activation")]
        activation: String,
        #[serde(default)]
        bias: [i16; 3],
        #[serde(default = "default_gyro_deadzone")]
        deadzone: u16,
        #[serde(default = "default_gyro_smoothing")]
        smoothing: f32,
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
    },
    SetCalibration {
        calibration: InputCalibration,
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
        preset: String,
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
    trigger_active: bool,
    light_active: bool,
    player_leds_active: bool,
    rumble_active: bool,
    trigger_until: Option<Instant>,
    rumble_until: Option<Instant>,
    haptic: Option<HapticClip>,
    audio: AudioEngine,
    speaker_route_until: Option<Instant>,
    speaker_track_id: u64,
    microphone_muted: Option<bool>,
    desktop: DesktopMapper,
    calibration: InputCalibration,
    last_input_sent: Instant,
    last_input_buttons: [bool; 23],
    last_valid_report: Instant,
    panic_latched: bool,
    firmware: Option<String>,
    last_trigger: Option<([u8; 11], [u8; 11])>,
    last_light: Option<[u8; 3]>,
    battery_light: bool,
    sensor_light: bool,
    last_sensor: Option<u8>,
    screen_light: bool,
    screen_color: Option<[u8; 3]>,
    last_rumble: Option<[u8; 2]>,
    input_effect: Option<InputEffect>,
    input_effect_until: Option<Instant>,
    bows: [Option<BowReturn>; 2],
    game_actions: ActionMapper,
    action_until: Option<Instant>,
    low_battery_alert: bool,
    low_battery_latched: bool,
    last_battery: Option<u8>,
    haptic_timer: Option<Instant>,
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

fn encode_live_trigger(setting: &TriggerSetting) -> Result<[u8; 11], &'static str> {
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
    fn enable_pcm(&mut self) -> Result<(), String> {
        self.haptic = None;
        self.rumble_until = None;
        if self.rumble_active {
            self.write_rumble([0, 0])?;
        }
        self.last_rumble = None;
        self.write(protocol::audio_haptics_enable_report())
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
        if matches!(self.transport, BusType::Usb) && self.play_voice(voice).is_ok() {
            return;
        }
        let fallback = HapticClip::new(shape.into(), frequency, attack, release, strength, pan);
        self.rumble_until = Some(Instant::now() + fallback.duration());
        self.haptic = Some(fallback);
    }

    fn play_action(&mut self, rule: &ActionRule) -> Result<(), String> {
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
            let off = protocol::encode_trigger(&TriggerSetting {
                mode: "off".into(),
                curve: vec![0.; 11],
                ..TriggerSetting::default()
            })
            .map_err(str::to_string)?;
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
        Ok(())
    }
    fn restore_action(&mut self) -> Result<(), String> {
        if self.action_until.is_none() {
            return Ok(());
        }
        let off = protocol::encode_trigger(&TriggerSetting {
            mode: "off".into(),
            curve: vec![0.; 11],
            ..TriggerSetting::default()
        })
        .map_err(str::to_string)?;
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
        self.trigger_active = left[0] != 0x05 || right[0] != 0x05;
        Ok(())
    }
    fn set_speaker_route(&mut self, enabled: bool) -> Result<(), String> {
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
        Ok(())
    }

    fn write(&mut self, mut report: [u8; protocol::USB_OUTPUT_LEN]) -> Result<(), String> {
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
        self.desktop.set_enabled(false);
        self.haptic = None;
        self.audio.stop();
        let restore_speaker = self.speaker_route_until.is_some();
        self.speaker_route_until = None;
        if restore_speaker {
            self.set_speaker_route(false)?;
        }
        self.input_effect = None;
        self.input_effect_until = None;
        self.bows = [None, None];
        self.low_battery_alert = false;
        self.low_battery_latched = false;
        self.haptic_timer = None;
        self.screen_light = false;
        self.screen_color = None;
        self.sensor_light = false;
        self.game_actions = ActionMapper::default();
        self.action_until = None;
        self.last_sensor = None;
        if !(self.trigger_active
            || self.light_active
            || self.rumble_active
            || self.player_leds_active)
        {
            return Ok(());
        }
        let off = protocol::encode_trigger(&TriggerSetting {
            mode: "off".into(),
            strength: 0.0,
            curve: vec![0.0; 11],
            ..TriggerSetting::default()
        })
        .unwrap();
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
        Ok(())
    }

    fn release_lightbar(&mut self) -> Result<(), String> {
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
        let off = protocol::encode_trigger(&TriggerSetting {
            mode: "off".into(),
            strength: 0.0,
            curve: vec![0.0; 11],
            ..TriggerSetting::default()
        })
        .unwrap();
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
                let effect = if unloaded {
                    [0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
                } else {
                    baseline
                };
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

fn controller_identity(info: &hidapi::DeviceInfo, hid: &HidDevice) -> String {
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
    // Opaque stable local identifier; never include the hardware address in shareable diagnostics.
    let identity = format!("{:04x}:{address}", info.product_id());
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("pad-{hash:016x}")
}

fn list_devices(api: &HidApi) -> Vec<serde_json::Value> {
    let mut items: Vec<_> = api
        .device_list()
        .filter(|info| is_controller(info))
        .map(|info| {
            json!({
                "path": path_key(info),
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
    emit(
        proxy,
        json!({
            "type": "devices",
            "items": list_devices(api),
            "selected": selected,
        }),
    );
}

fn open_path(api: &HidApi, wanted: Option<&str>) -> Result<Option<Connected>, String> {
    let mut candidates: Vec<_> = api
        .device_list()
        .filter(|info| is_controller(info))
        .collect();
    candidates.sort_by_key(|info| {
        let usb = matches!(info.bus_type(), BusType::Usb);
        let preferred = wanted.is_some_and(|w| path_key(info) == w);
        (!preferred, if usb { 0 } else { 1 })
    });
    if let Some(wanted) = wanted {
        candidates.retain(|info| path_key(info) == wanted);
        if candidates.is_empty() {
            return Err(format!(
                "Selected controller is no longer available: {wanted}"
            ));
        }
    }
    let mut last_error = None;
    for info in candidates {
        match info.open_device(api) {
            Ok(hid) => {
                let mut firmware_report = [0u8; 64];
                firmware_report[0] = 0x20;
                let (firmware, feature_version) = match hid.get_feature_report(&mut firmware_report)
                {
                    Ok(length) if length >= 46 => {
                        let hw = u32::from_le_bytes(firmware_report[24..28].try_into().unwrap());
                        let fw = u32::from_le_bytes(firmware_report[28..32].try_into().unwrap());
                        let feature =
                            u16::from_le_bytes(firmware_report[44..46].try_into().unwrap());
                        (
                            Some(format!("FW {fw:08X} / HW {hw:08X} / FEATURE {feature:04X}")),
                            Some(feature),
                        )
                    }
                    _ => (None, None),
                };
                if matches!(info.bus_type(), BusType::Bluetooth) {
                    let mut feature = [0u8; 41];
                    feature[0] = 0x05;
                    let _ = hid.get_feature_report(&mut feature);
                }
                let display = path_key(info);
                let controller_id = controller_identity(info, &hid);
                return Ok(Some(Connected {
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
                    trigger_active: false,
                    light_active: false,
                    player_leds_active: false,
                    rumble_active: false,
                    trigger_until: None,
                    rumble_until: None,
                    haptic: None,
                    audio: AudioEngine::default(),
                    speaker_route_until: None,
                    speaker_track_id: 0,
                    microphone_muted: None,
                    desktop: DesktopMapper::default(),
                    calibration: InputCalibration::default(),
                    last_input_sent: Instant::now() - Duration::from_secs(1),
                    last_input_buttons: [false; 23],
                    last_valid_report: Instant::now(),
                    panic_latched: false,
                    firmware,
                    last_trigger: None,
                    game_actions: ActionMapper::default(),
                    action_until: None,
                    last_light: None,
                    battery_light: false,
                    sensor_light: false,
                    last_sensor: None,
                    screen_light: false,
                    screen_color: None,
                    last_rumble: None,
                    input_effect: None,
                    input_effect_until: None,
                    bows: [None, None],
                    low_battery_alert: false,
                    low_battery_latched: false,
                    last_battery: None,
                    haptic_timer: None,
                }));
            }
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    if let Some(error) = last_error {
        Err(format!("Controller found, but could not open it: {error}"))
    } else {
        Ok(None)
    }
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
        let _ = old.reset();
    }
    emit(
        proxy,
        json!({"type":"device","connected":false,"message":message}),
    );
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
    emit(proxy, json!({"type":"mouseMode","enabled":false}));
    emit(proxy, json!({"type":"touchGestures","enabled":false}));
    emit(proxy, json!({"type":"gyroAim","enabled":false}));
    emit(proxy, json!({"type":"remap","enabled":false}));
    emit(proxy, xinput.status_event());
}

fn emit_connected(proxy: &Sender<serde_json::Value>, found: &Connected) {
    emit(
        proxy,
        json!({
            "type": "device",
            "connected": true,
            "name": found.name,
            "transport": if matches!(found.transport, BusType::Bluetooth) { "Bluetooth" } else { "USB" },
            "firmware": found.firmware,
            "path": found.path_display,
            "controllerId": found.controller_id,
            "productId": format!("{:04x}", found.product_id),
            "isEdge": found.product_id == protocol::EDGE_PID,
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
    api: &HidApi,
    preferred_path: &mut Option<String>,
) -> bool {
    match command {
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
                let _ = d.restore_action();
                if let Err(message) = d.game_actions.set(if enabled { rules } else { Vec::new() }) {
                    emit(proxy, json!({"type":"error","message":message}));
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
            emit_device_list(
                api,
                proxy,
                preferred_path
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
                let _ = old.reset();
            }
            emit(proxy, json!({"type":"lightMode","battery":false}));
            emit(
                proxy,
                json!({"type":"triggerLease","active":false,"durationMs":0}),
            );
            *preferred_path = Some(path.clone());
            match open_path(api, Some(&path)) {
                Ok(Some(found)) => {
                    emit_connected(proxy, &found);
                    emit_device_list(api, proxy, Some(&found.path_display));
                    *device = Some(found);
                    emit(
                        proxy,
                        json!({"type":"notice","message":"Controller selected"}),
                    );
                }
                Ok(None) => {
                    emit(
                        proxy,
                        json!({"type":"device","connected":false,"message":"Controller not found"}),
                    );
                }
                Err(error) => {
                    emit(proxy, json!({"type":"error","message":error}));
                }
            }
            return true;
        }
        DeviceCommand::SetTouchMouse { enabled } => {
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
        } => {
            let Some(device) = device.as_mut() else {
                return true;
            };
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
                        || (b.behavior == "macro"
                            && (b.sequence.is_empty() || b.sequence.len() > 5))
                        || (b.behavior != "macro" && !b.sequence.is_empty())
                        || b.chord.len() > 8
                        || b.chord
                            .iter()
                            .any(|&vk| !crate::input_map::supported_key(vk))
                        || (b.behavior == "macro" && !b.chord.is_empty())
                        || b.sequence
                            .iter()
                            .any(|&vk| !crate::input_map::supported_key(vk))
                        || !seen.insert(b.button)
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
            device.desktop.set_programs(programs);
            let mapped = bindings
                .into_iter()
                .filter(|b| !matches!(b.behavior.as_str(), "launch" | "program"))
                .map(|b| RemapBinding {
                    button: b.button,
                    vk: b.vk,
                    modifier: None,
                    behavior: RemapBehavior::from_label(&b.behavior).unwrap_or(RemapBehavior::Hold),
                    sequence: b.sequence,
                    chord: b.chord,
                })
                .collect();
            device.desktop.set_bindings(
                mapped,
                click_threshold,
                pointer_sensitivity,
                reverse_scroll,
                launches,
            );
            emit(
                proxy,
                json!({"type":"pcBindings","count":seen.len(),"clickThreshold":click_threshold}),
            );
            return true;
        }
        DeviceCommand::SetCalibration { calibration } => {
            if !calibration.valid() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Calibration ranges are incomplete or invalid."}),
                );
                return true;
            }
            if let Some(connected) = device.as_mut() {
                connected.calibration = calibration;
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
            if !(0.05..=8.0).contains(&sensitivity)
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
                bias,
                deadzone,
                smoothing,
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
                        || (b.behavior == "macro"
                            && (b.sequence.is_empty() || b.sequence.len() > 5))
                        || (b.behavior != "macro" && !b.sequence.is_empty())
                        || b.chord.len() > 8
                        || b.chord
                            .iter()
                            .any(|&vk| !crate::input_map::supported_key(vk))
                        || (b.behavior == "macro" && !b.chord.is_empty())
                        || b.sequence
                            .iter()
                            .any(|&vk| !crate::input_map::supported_key(vk))
                        || !seen.insert((b.button, b.modifier))
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
                .map(|b| RemapBinding {
                    button: b.button,
                    vk: b.vk,
                    modifier: b.modifier,
                    behavior: RemapBehavior::from_label(&b.behavior).unwrap_or(RemapBehavior::Hold),
                    sequence: b.sequence,
                    chord: b.chord,
                })
                .collect();
            remap.set(enabled, mapped);
            emit(proxy, json!({"type":"remap","enabled":enabled}));
            emit(
                proxy,
                json!({"type":"notice","message":if enabled {"Remap on"} else {"Remap off"}}),
            );
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

    let Some(device) = device.as_mut() else {
        emit(
            proxy,
            json!({ "type": "notice", "message": "Connect a controller first." }),
        );
        return true;
    };

    let audio_request = match &command {
        DeviceCommand::PlayAudioFile { track_id, .. }
        | DeviceCommand::PlaySpeakerSound { track_id, .. }
        | DeviceCommand::SetSpeakerPaused { track_id, .. }
        | DeviceCommand::StopSpeakerSound { track_id } => Some(*track_id),
        _ => None,
    };
    let result: Result<&str, String> = match command {
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
            let result = if device.last_light == Some(rgb) {
                Ok(())
            } else {
                device.write(protocol::usb_report(
                    0,
                    0x04,
                    0,
                    None,
                    None,
                    Some(rgb),
                    None,
                    false,
                ))
            };
            result.map(|_| {
                device.battery_light = false;
                emit(proxy, json!({"type":"lightMode","battery":false}));
                device.light_active = true;
                device.last_light = Some(rgb);
                "Lightbar set"
            })
        }
        DeviceCommand::SetPlayerLeds { mask } => {
            if mask > 0x1f {
                Err("Player indicator mask must use five lights".into())
            } else {
                device.write(protocol::player_leds_report(mask)).map(|_| {
                    device.player_leds_active = mask != 0;
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
        DeviceCommand::SetScreenLightbar { enabled } => {
            if enabled && !cfg!(windows) {
                emit(
                    proxy,
                    json!({"type":"error","message":"Screen color sampling is currently available on Windows."}),
                );
                return true;
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
        DeviceCommand::PlayHaptic {
            shape,
            frequency,
            attack,
            release,
            strength,
            pan,
        } => {
            let voice = VoiceClip::new(&shape, frequency, attack, release, strength, pan);
            if matches!(device.transport, BusType::Usb) {
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
            if !matches!(device.transport,BusType::Usb) {Err("Controller audio requires USB.".into())}
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
            } else if !matches!(device.transport, BusType::Usb) {
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
            preset,
        } => {
            if enabled && !cfg!(windows) {
                Err("System audio capture is currently available on Windows.".into())
            } else if enabled && !matches!(device.transport, BusType::Usb) {
                Err("System audio haptics require USB. Bluetooth motor fallback remains available in Haptic Lab.".into())
            } else {
                let result = if enabled { device.enable_pcm() } else { Ok(()) }
                    .and_then(|_| device.audio.set_bridge(enabled, gain, &preset));
                if let Ok(message) = &result {
                    emit(
                        proxy,
                        json!({"type":"audioStatus","route":if enabled {"voiceCoil"} else {"off"},"bridge":enabled,"endpoint":device.audio.endpoint(),"message":message}),
                    );
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
            let motors = [left.min(100), right.min(100)];
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
        DeviceCommand::Scan
        | DeviceCommand::Shutdown
        | DeviceCommand::ListDevices
        | DeviceCommand::SelectDevice { .. }
        | DeviceCommand::SetTouchMouse { .. }
        | DeviceCommand::SetTouchGestures { .. }
        | DeviceCommand::SetGyroAim { .. }
        | DeviceCommand::SetRemap { .. }
        | DeviceCommand::SetXinput { .. }
        | DeviceCommand::SetTelemetry { .. }
        | DeviceCommand::SetScript { .. }
        | DeviceCommand::SetUiFocused { .. }
        | DeviceCommand::SetPcMode { .. }
        | DeviceCommand::SetPcBindings { .. }
        | DeviceCommand::SetCalibration { .. }
        | DeviceCommand::SetGameActions { .. }
        | DeviceCommand::SetGameLink { .. }
        | DeviceCommand::StopMacros
        | DeviceCommand::SetGameRecording { .. } => unreachable!(),
    };
    match result {
        Ok(message) => emit(proxy, json!({ "type": "notice", "message": message })),
        Err(message) => {
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

pub fn run(rx: Receiver<DeviceCommand>, proxy: Sender<serde_json::Value>) {
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
    let mut last_script_cue: Option<Instant> = None;
    let screen = ScreenSampler::new();
    emit(&proxy, xinput.status_event());
    let mut preferred_path: Option<String> = None;
    let mut next_scan = Instant::now();
    let mut input_report = [0u8; 78];
    let mut next_process_check = Instant::now();
    #[cfg(windows)]
    let gui_pid = std::env::var("CONTROLLER_GUI_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
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
    loop {
        #[cfg(windows)]
        if let Some(gui_pid) = gui_pid
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
                emit(&proxy, connected.audio.diagnostics());
            }
            next_audio_status = Instant::now() + Duration::from_millis(250);
        }
        if Instant::now() >= next_rate {
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
                    if let Some(status) = connected.audio.file_status() {
                        emit(
                            &proxy,
                            json!({"type":"speakerStatus","source":"error","trackId":status["trackId"],"message":error}),
                        );
                    }
                    connected.audio.stop();
                    emit(
                        &proxy,
                        json!({"type":"audioStatus","route":"off","bridge":false,"message":error}),
                    );
                    emit(&proxy, json!({"type":"error","message":error}));
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
            next_process_check = Instant::now() + Duration::from_secs(2);
        }
        loop {
            match rx.try_recv() {
                Ok(command) => {
                    if matches!(command, DeviceCommand::Reset | DeviceCommand::Shutdown) {
                        game_link.stop();
                    }
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
                        &api,
                        &mut preferred_path,
                    ) {
                        return;
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
                                let _ = d.write(protocol::usb_report(
                                    0,
                                    4,
                                    0,
                                    None,
                                    None,
                                    Some(rgb),
                                    None,
                                    false,
                                ));
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
                                duration_ms: 15000,
                                haptic: None,
                                left_trigger: update.triggers[0].clone(),
                                right_trigger: update.triggers[1].clone(),
                            };
                            let _ = d.play_action(&rule);
                        }
                        if let Some(rgb) = update.rgb {
                            let _ = d.write(protocol::usb_report(
                                0,
                                4,
                                0,
                                None,
                                None,
                                Some(rgb),
                                None,
                                false,
                            ));
                            d.light_active = true;
                        }
                        for event in &update.events {
                            for rule in d.game_actions.event(event, Instant::now()) {
                                let _ = d.play_action(&rule);
                            }
                        }
                    }
                    emit(
                        &proxy,
                        json!({"type":"gameEvent","weapon":update.weapon,"events":update.events,"packet":update.packet,"format":update.format}),
                    );
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
                if tick.emit_ui && connected.last_light != Some(color) {
                    let report =
                        protocol::usb_report(0, 0x04, 0, None, None, Some(color), None, false);
                    if connected.write(report).is_ok() {
                        connected.last_light = Some(color);
                        connected.light_active = true;
                        connected.battery_light = false;
                    }
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
                    if connected.last_light != Some(color) {
                        let report =
                            protocol::usb_report(0, 0x04, 0, None, None, Some(color), None, false);
                        if connected.write(report).is_ok() {
                            connected.last_light = Some(color);
                            connected.light_active = true;
                            emit(&proxy, json!({"type":"screenColor","rgb":color}));
                        }
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
        if device.is_some() && Instant::now() >= next_scan {
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
                    && preferred_path.is_none()
                    && api
                        .device_list()
                        .any(|info| is_controller(info) && matches!(info.bus_type(), BusType::Usb))
                    && let Ok(Some(found)) = open_path(&api, None)
                    && found.path != current.path
                {
                    stop_input_mappers(&mut mouse, &mut gyro, &mut remap, &mut xinput, &proxy);
                    screen.set_enabled(false);
                    if let Some(old) = device.as_mut() {
                        let _ = old.reset();
                    }
                    emit(&proxy, json!({"type":"lightMode","battery":false}));
                    emit(
                        &proxy,
                        json!({"type":"triggerLease","active":false,"durationMs":0}),
                    );
                    emit_connected(&proxy, &found);
                    emit_device_list(&api, &proxy, Some(&found.path_display));
                    device = Some(found);
                }
            }
            emit_device_list(
                &api,
                &proxy,
                preferred_path
                    .as_deref()
                    .or(device.as_ref().map(|d| d.path_display.as_str())),
            );
        }
        remap.tick();
        if let Some(connected) = device.as_mut() {
            connected.desktop.tick();
        }
        if let Some(connected) = device.as_mut() {
            if connected
                .speaker_route_until
                .is_some_and(|at| Instant::now() >= at)
                && let Err(error) = connected.set_speaker_route(false)
            {
                emit(&proxy, json!({"type":"error","message":error}));
            }
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
                let off = protocol::encode_trigger(&TriggerSetting {
                    mode: "off".into(),
                    strength: 0.0,
                    curve: vec![0.0; 11],
                    ..TriggerSetting::default()
                })
                .unwrap();
                let report =
                    protocol::usb_report(0x0c, 0, 0, Some(off), Some(off), None, None, false);
                if connected.write(report).is_ok() {
                    connected.trigger_active = false;
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
                            let _ = connected.write_rumble(motors);
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
            match connected.hid.read_timeout(&mut input_report, 20) {
                Ok(length) if length > 0 => {
                    if let Some(mut input) = protocol::parse_input(&input_report[..length]) {
                        connected.last_battery = input.battery;
                        if connected.microphone_muted.is_none() {
                            connected.microphone_muted = Some(input.battery_info.microphone_muted);
                            emit(
                                &proxy,
                                json!({"type":"microphoneStatus","muted":input.battery_info.microphone_muted}),
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
                        if input.charging || input.battery.is_some_and(|level| level >= 30) {
                            connected.low_battery_latched = false;
                        } else if connected.low_battery_alert
                            && !connected.low_battery_latched
                            && let Some(level) = input.battery.filter(|level| *level <= 20)
                        {
                            connected.low_battery_latched = true;
                            connected.play_feedback("square", 155, 3, 120, 32, 0);
                            emit(
                                &proxy,
                                json!({"type":"hapticAlert","message":format!("Battery low: {level}%")}),
                            );
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
                            if connected.last_light != Some(color) {
                                let report = protocol::usb_report(
                                    0,
                                    0x04,
                                    0,
                                    None,
                                    None,
                                    Some(color),
                                    None,
                                    false,
                                );
                                if connected.write(report).is_ok() {
                                    connected.last_light = Some(color);
                                    connected.light_active = true;
                                }
                            }
                        }
                        if connected.sensor_light
                            && let Some(sensor_byte) = input.battery_info.sensor_byte
                        {
                            let color = sensor_color(sensor_byte);
                            let changed = connected.last_sensor != Some(sensor_byte);
                            if connected.last_light != Some(color) {
                                let report = protocol::usb_report(
                                    0,
                                    0x04,
                                    0,
                                    None,
                                    None,
                                    Some(color),
                                    None,
                                    false,
                                );
                                if connected.write(report).is_ok() {
                                    connected.last_light = Some(color);
                                    connected.light_active = true;
                                }
                            }
                            if changed && connected.last_light == Some(color) {
                                connected.last_sensor = Some(sensor_byte);
                                emit(
                                    &proxy,
                                    json!({"type":"sensorColor","sensorByte":sensor_byte,"rgb":color}),
                                );
                            }
                        }
                        connected.calibration.apply(&mut input);
                        let touch_cue = mouse.update(&input);
                        gyro.update(&input);
                        remap.update(&input);
                        connected.desktop.update(&input);
                        if let Some(message) = connected.desktop.take_launch_error() {
                            emit(&proxy, json!({"type":"error","message":message}));
                        }
                        if !connected.desktop.enabled() && !input.panic_combo {
                            for rule in connected
                                .game_actions
                                .update(&input.buttons, Instant::now())
                            {
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
                        if let Some(event) = xinput.update(&input) {
                            emit(&proxy, event.clone());
                            if let Some(message) = event["message"].as_str() {
                                emit(&proxy, json!({ "type": "error", "message": message }));
                            }
                        }
                        if input.panic_combo && !connected.panic_latched {
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
                            emit(&proxy, json!({ "type": "input", "input": input }));
                            connected.last_input_sent = Instant::now();
                        }
                    } else if matches!(input_report[0], 0x01 | 0x31) {
                        invalid_reports += 1;
                    }
                }
                Ok(_) => {}
                Err(error) => {
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
            if Instant::now() >= next_scan {
                let _ = api.refresh_devices();
                match open_path(&api, preferred_path.as_deref()) {
                    Ok(Some(found)) => {
                        emit_connected(&proxy, &found);
                        emit_device_list(&api, &proxy, Some(&found.path_display));
                        device = Some(found);
                    }
                    Ok(None) => {
                        emit(
                            &proxy,
                            json!({ "type": "device", "connected": false, "message": "No DualSense found" }),
                        );
                        emit_device_list(&api, &proxy, preferred_path.as_deref());
                    }
                    Err(error) => emit(
                        &proxy,
                        json!({ "type": "device", "connected": false, "message": error }),
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
    use super::{
        BowReturn, DeviceCommand, InputEffect, KickStyle, TriggerSetting, WeaponKind,
        encode_live_trigger, sensor_color,
    };
    use aftertouch_protocol as protocol;
    use std::time::{Duration, Instant};

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
}
