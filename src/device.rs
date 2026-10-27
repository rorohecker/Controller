use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::{
    audio_haptics::{AudioEngine, VoiceClip},
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
    PlaySpeakerSound {
        #[serde(rename = "pcmBase64")]
        pcm_base64: String,
        route: String,
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
    #[serde(default, rename = "launchPath")]
    pub launch_path: Option<String>,
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
        && file.is_file()
        && file
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
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
    low_battery_alert: bool,
    low_battery_latched: bool,
    haptic_timer: Option<Instant>,
}

struct InputEffect {
    threshold: f32,
    strength: u8,
    kind: WeaponKind,
    rate: u8,
    armed: bool,
    remaining: u8,
    next_pulse: Option<Instant>,
}

impl InputEffect {
    fn advance(&mut self, now: Instant, right_pull: f32) -> bool {
        let interval = Duration::from_secs_f64(1.0 / f64::from(self.rate.max(1)));
        if right_pull < self.threshold - 0.08 {
            self.armed = true;
            if self.kind == WeaponKind::Smg {
                self.next_pulse = None;
            }
        }
        let first = self.armed && right_pull >= self.threshold;
        if first {
            self.armed = false;
            self.remaining = if self.kind == WeaponKind::Burst { 2 } else { 0 };
            self.next_pulse = Some(now + interval);
            return true;
        }
        let repeat = self.next_pulse.is_some_and(|at| now >= at)
            && (self.remaining > 0
                || (self.kind == WeaponKind::Smg && right_pull >= self.threshold));
        if repeat {
            self.remaining = self.remaining.saturating_sub(1);
            self.next_pulse = Some(now + interval);
        }
        repeat
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
            Self::Sniper => ("sniper", 55, 2, 300, strength, 0),
            Self::Burst => ("square", 110, 2, 65, strength, 20),
            Self::Smg => ("square", 95, 2, 75, strength, 20),
        }
    }
}

impl Connected {
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
        if matches!(self.transport, BusType::Usb) && self.audio.play(voice).is_ok() {
            return;
        }
        let fallback = HapticClip::new(shape.into(), frequency, attack, release, strength, pan);
        self.rumble_until = Some(Instant::now() + fallback.duration());
        self.haptic = Some(fallback);
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

    fn write(&mut self, report: [u8; protocol::USB_OUTPUT_LEN]) -> Result<(), String> {
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
        self.low_battery_alert = false;
        self.low_battery_latched = false;
        self.haptic_timer = None;
        self.screen_light = false;
        self.screen_color = None;
        self.sensor_light = false;
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
        if self.input_effect_until.is_some_and(|t| Instant::now() >= t) {
            self.restore_input_effect()?;
        }
        let Some(effect) = self.input_effect.as_mut() else {
            return Ok(());
        };
        if !effect.advance(Instant::now(), right_pull) {
            return Ok(());
        }
        let kind = effect.kind;
        let threshold = effect.threshold;
        let strength = effect.strength;
        if self.input_effect_until.is_some() {
            self.restore_input_effect()?;
        }
        let mut curve = vec![0.0; 11];
        let vibration = matches!(
            kind,
            WeaponKind::Sniper | WeaponKind::Burst | WeaponKind::Smg
        );
        if vibration {
            curve[2..].fill(1.0);
        } else {
            let zone = ((threshold * 10.0).round() as usize).clamp(2, 7);
            curve[zone + 1..].fill(1.0);
        }
        let click = protocol::encode_trigger(&TriggerSetting {
            mode: if vibration { "vibration" } else { "weapon" }.into(),
            strength: strength as f32,
            curve,
            force_cap: (kind == WeaponKind::Sniper).then_some(8),
            ..TriggerSetting::default()
        })
        .map_err(str::to_string)?;
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
        self.input_effect_until = Some(
            Instant::now()
                + Duration::from_millis(match kind {
                    WeaponKind::Sniper => 210,
                    WeaponKind::Burst => 70,
                    WeaponKind::Smg => 65,
                    _ => 65,
                }),
        );
        let (shape, frequency, attack, release, power, pan) = kind.cue(strength);
        self.play_feedback(shape, frequency, attack, release, power, pan);
        if kind == WeaponKind::Sniper && matches!(self.transport, BusType::Usb) {
            self.write_rumble([72, 88])?;
            self.rumble_until = Some(Instant::now() + Duration::from_millis(145));
        } else if kind == WeaponKind::Burst && matches!(self.transport, BusType::Usb) {
            self.write_rumble([38, 50])?;
            self.rumble_until = Some(Instant::now() + Duration::from_millis(60));
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
                return Ok(Some(Connected {
                    hid,
                    path: info.path().to_bytes().to_vec(),
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
                    desktop: DesktopMapper::default(),
                    calibration: InputCalibration::default(),
                    last_input_sent: Instant::now() - Duration::from_secs(1),
                    last_input_buttons: [false; 23],
                    last_valid_report: Instant::now(),
                    panic_latched: false,
                    firmware,
                    last_trigger: None,
                    last_light: None,
                    battery_light: false,
                    sensor_light: false,
                    last_sensor: None,
                    screen_light: false,
                    screen_color: None,
                    last_rumble: None,
                    input_effect: None,
                    input_effect_until: None,
                    low_battery_alert: false,
                    low_battery_latched: false,
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
    script: &mut Option<ScriptProgram>,
    screen: &ScreenSampler,
    telemetry_stale_latched: &mut bool,
    api: &HidApi,
    preferred_path: &mut Option<String>,
) -> bool {
    match command {
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
            if enabled && !cfg!(windows) {
                emit(
                    proxy,
                    json!({"type":"error","message":"Desktop pointer control is currently available on Windows."}),
                );
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
            if enabled && !cfg!(windows) {
                emit(
                    proxy,
                    json!({"type":"error","message":"PC control is currently available on Windows."}),
                );
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
                        || (b.behavior != "launch"
                            && (b.vk == 0
                                || b.vk > 255
                                || RemapBehavior::from_label(&b.behavior).is_none()
                                || b.launch_path.is_some()))
                        || (b.behavior == "launch"
                            && (b.vk != 0
                                || b.launch_path.as_deref().is_none_or(valid_launch_path)))
                        || (b.behavior == "macro"
                            && (b.sequence.is_empty() || b.sequence.len() > 5))
                        || (b.behavior != "macro" && !b.sequence.is_empty())
                        || b.sequence.iter().any(|&vk| vk == 0 || vk > 255)
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
                })
                .collect();
            let mapped = bindings
                .into_iter()
                .filter(|b| b.behavior != "launch")
                .map(|b| RemapBinding {
                    button: b.button,
                    vk: b.vk,
                    modifier: None,
                    behavior: RemapBehavior::from_label(&b.behavior).unwrap_or(RemapBehavior::Hold),
                    sequence: b.sequence,
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
            if enabled && !cfg!(windows) {
                emit(
                    proxy,
                    json!({"type":"error","message":"Gyro pointer control is currently available on Windows."}),
                );
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
            if enabled && !cfg!(windows) {
                emit(
                    proxy,
                    json!({"type":"error","message":"Keyboard remap is currently available on Windows."}),
                );
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
                        || b.vk > 255
                        || b.modifier
                            .is_some_and(|modifier| modifier >= 23 || modifier == b.button)
                        || RemapBehavior::from_label(&b.behavior).is_none()
                        || b.launch_path.is_some()
                        || (b.behavior == "macro"
                            && (b.sequence.is_empty() || b.sequence.len() > 5))
                        || (b.behavior != "macro" && !b.sequence.is_empty())
                        || b.sequence.iter().any(|&vk| vk == 0 || vk > 255)
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

    let result: Result<&str, String> = match command {
        DeviceCommand::ApplyTriggers { left, right } => {
            match (
                protocol::encode_trigger(&left),
                protocol::encode_trigger(&right),
            ) {
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
                match device.audio.play(voice) {
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
        DeviceCommand::PlaySpeakerSound { pcm_base64, route } => {
            if !matches!(device.transport, BusType::Usb) {
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
                    routing.and_then(|_| if route == "grips" { device.set_speaker_route(false) } else { device.set_speaker_route(true) })
                    .and_then(|_| device.audio.play_speaker_pcm(samples, &route)).map(|endpoint| {
                        device.speaker_route_until = (route != "grips").then(|| Instant::now() + duration + Duration::from_millis(100));
                        emit(proxy, json!({"type":"speakerStatus","source":"file","message":format!("Sound sent to {endpoint}")}));
                        "Controller speaker playing"
                    })
                })
            }
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
                let result = device.audio.set_bridge(enabled, gain, &preset);
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
        } => {
            if !threshold.is_finite()
                || !(0.2..=0.95).contains(&threshold)
                || strength > 100
                || (enabled && strength == 0)
                || !(3..=20).contains(&rate)
                || WeaponKind::parse(&kind).is_none()
            {
                Err("Weapon cue settings are out of range.".into())
            } else {
                device.restore_input_effect().map(|_| {
                    device.input_effect = enabled.then_some(InputEffect {threshold,strength,kind:WeaponKind::parse(&kind).unwrap(),rate,armed:true,remaining:0,next_pulse:None});
                    emit(proxy, json!({"type":"inputEffects","enabled":enabled,"threshold":threshold,"strength":strength,"kind":kind,"rate":rate}));
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
        | DeviceCommand::SetCalibration { .. } => unreachable!(),
    };
    match result {
        Ok(message) => emit(proxy, json!({ "type": "notice", "message": message })),
        Err(message) => emit(proxy, json!({ "type": "error", "message": message })),
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
    let mut script: Option<ScriptProgram> = None;
    let mut last_script_cue: Option<Instant> = None;
    let screen = ScreenSampler::new();
    emit(&proxy, xinput.status_event());
    let mut preferred_path: Option<String> = None;
    let mut next_scan = Instant::now();
    let mut input_report = [0u8; 78];
    let mut next_process_check = Instant::now();
    let gui_pid = std::env::var("CONTROLLER_GUI_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let mut next_focus_check = Instant::now();
    let mut last_conflicts = Vec::<&'static str>::new();
    let mut first_process_check = true;
    let mut last_foreground = (None::<String>, None::<String>, None::<String>);
    let mut raw_reports = 0u32;
    let mut sequence_gaps = 0u32;
    let mut invalid_reports = 0u32;
    let mut next_rate = Instant::now() + Duration::from_secs(1);
    let mut telemetry_stale_latched = false;
    loop {
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
        if Instant::now() >= next_rate {
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
                    connected.audio.stop();
                    emit(
                        &proxy,
                        json!({"type":"audioStatus","route":"off","bridge":false,"message":error}),
                    );
                    emit(&proxy, json!({"type":"error","message":error}));
                } else if connected.audio.bridge_on() {
                    emit(
                        &proxy,
                        json!({"type":"audioStatus","route":"voiceCoil","bridge":true,"endpoint":connected.audio.endpoint(),"callbacks":connected.audio.callbacks(),"glitches":connected.audio.glitches()}),
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
                    if !handle_command(
                        command,
                        &mut device,
                        &proxy,
                        &mut mouse,
                        &mut gyro,
                        &mut remap,
                        &mut xinput,
                        &mut telemetry,
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
                        || connected.audio.play(clip).is_err()
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
                        if let Some(event) = xinput.update(&input) {
                            emit(&proxy, event.clone());
                            if let Some(message) = event["message"].as_str() {
                                emit(&proxy, json!({ "type": "error", "message": message }));
                            }
                        }
                        if input.panic_combo && !connected.panic_latched {
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
                                || connected.audio.play(clip).is_err()
                            {
                                let fallback = HapticClip::new("sine".into(), 180, 2, 55, 18, pan);
                                connected.rumble_until = Some(Instant::now() + fallback.duration());
                                connected.haptic = Some(fallback);
                            }
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
    use super::{InputEffect, WeaponKind, sensor_color};
    use std::time::{Duration, Instant};

    #[test]
    fn raw_sensor_byte_spans_the_color_palette() {
        assert_eq!(sensor_color(0), [60, 130, 200]);
        assert_eq!(sensor_color(127), [230, 165, 45]);
        assert_eq!(sensor_color(255), [220, 45, 50]);
    }

    #[test]
    fn burst_fires_three_separate_kicks_and_smg_stops_on_release() {
        let now = Instant::now();
        let mut burst = InputEffect {
            threshold: 0.6,
            strength: 60,
            kind: WeaponKind::Burst,
            rate: 6,
            armed: true,
            remaining: 0,
            next_pulse: None,
        };
        assert!(burst.advance(now, 0.7));
        let (_, _, attack, release, _, _) = WeaponKind::Burst.cue(78);
        assert!(u64::from(attack + release) < 1000 / 6);
        assert!(!burst.advance(now + Duration::from_millis(90), 0.7));
        assert!(!burst.advance(now + Duration::from_millis(166), 0.7));
        assert!(burst.advance(now + Duration::from_millis(167), 0.7));
        assert!(!burst.advance(now + Duration::from_millis(300), 0.7));
        assert!(burst.advance(now + Duration::from_millis(334), 0.7));
        assert!(!burst.advance(now + Duration::from_millis(500), 0.7));
        let mut smg = InputEffect {
            kind: WeaponKind::Smg,
            ..burst
        };
        smg.armed = true;
        smg.remaining = 0;
        smg.next_pulse = None;
        assert!(smg.advance(now, 0.7));
        assert!(smg.advance(now + Duration::from_millis(167), 0.7));
        assert!(!smg.advance(now + Duration::from_millis(334), 0.0));
        assert!(smg.advance(now + Duration::from_millis(500), 0.7));
    }
}
