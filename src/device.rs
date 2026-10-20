use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::{
    haptics::HapticClip,
    input_map::{
        GyroActivation, GyroAimConfig, GyroAimMapper, MouseMapper, RemapBinding, RemapMapper,
    },
    processes,
    xinput_bridge::XInputBridge,
};
use aftertouch_protocol::{self as protocol, TriggerSetting};
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
    ReleaseLightbar,
    BatteryLightbar {
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
    },
    SetRemap {
        enabled: bool,
        #[serde(default)]
        bindings: Vec<RemapBindingDto>,
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
    rumble_active: bool,
    trigger_until: Option<Instant>,
    rumble_until: Option<Instant>,
    haptic: Option<HapticClip>,
    last_input_sent: Instant,
    last_valid_report: Instant,
    panic_latched: bool,
    firmware: Option<String>,
    last_trigger: Option<([u8; 11], [u8; 11])>,
    last_light: Option<[u8; 3]>,
    battery_light: bool,
    last_rumble: Option<[u8; 2]>,
}

impl Connected {
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
        self.haptic = None;
        if !(self.trigger_active || self.light_active || self.rumble_active) {
            return Ok(());
        }
        let off = protocol::encode_trigger(&TriggerSetting {
            mode: "off".into(),
            strength: 0.0,
            curve: vec![0.0; 11],
        })
        .unwrap();
        let valid0 = (if self.trigger_active { 0x0c } else { 0 })
            | if self.rumble_active {
                self.rumble_flags.0
            } else {
                0
            };
        let report = protocol::usb_report(
            valid0,
            if self.light_active { 0x08 } else { 0 },
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
        self.write(report)?;
        self.trigger_active = false;
        self.light_active = false;
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
                    rumble_active: false,
                    trigger_until: None,
                    rumble_until: None,
                    haptic: None,
                    last_input_sent: Instant::now() - Duration::from_secs(1),
                    last_valid_report: Instant::now(),
                    panic_latched: false,
                    firmware,
                    last_trigger: None,
                    last_light: None,
                    battery_light: false,
                    last_rumble: None,
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

fn drop_device(
    device: &mut Option<Connected>,
    mouse: &mut MouseMapper,
    gyro: &mut GyroAimMapper,
    remap: &mut RemapMapper,
    xinput: &mut XInputBridge,
    proxy: &Sender<serde_json::Value>,
    message: &str,
) {
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
                Err(error) => emit(proxy, json!({"type":"error","message":error})),
            }
            return true;
        }
        DeviceCommand::SetTouchMouse { enabled } => {
            if enabled && device.is_none() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Connect a controller first."}),
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
        } => {
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
            if enabled && device.is_none() {
                emit(
                    proxy,
                    json!({"type":"error","message":"Connect a controller before enabling remap."}),
                );
                return true;
            }
            let mut seen = std::collections::HashSet::new();
            if bindings.len() > 23
                || bindings
                    .iter()
                    .any(|b| b.button >= 23 || b.vk == 0 || b.vk > 255 || !seen.insert(b.button))
            {
                emit(
                    proxy,
                    json!({"type":"error","message":"Remap bindings must use distinct buttons and valid Windows keys."}),
                );
                return true;
            }
            let mapped = bindings
                .into_iter()
                .map(|b| RemapBinding {
                    button: b.button,
                    vk: b.vk,
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
                    let changed = device.last_trigger != Some((left, right));
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
        DeviceCommand::ReleaseLightbar => {
            device.battery_light = false;
            emit(proxy, json!({"type":"lightMode","battery":false}));
            device.release_lightbar().map(|_| "Lightbar released")
        }
        DeviceCommand::BatteryLightbar { enabled } => {
            device.battery_light = enabled;
            emit(proxy, json!({"type":"lightMode","battery":enabled}));
            if enabled {
                Ok("Battery light on")
            } else {
                device.release_lightbar().map(|_| "Battery light off")
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
            let clip = HapticClip::new(shape, frequency, attack, release, strength, pan);
            let duration = clip.duration();
            device.haptic = Some(clip);
            device.rumble_until = Some(Instant::now() + duration);
            Ok("Haptic playing")
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
        | DeviceCommand::SetXinput { .. } => unreachable!(),
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
    emit(&proxy, xinput.status_event());
    let mut preferred_path: Option<String> = None;
    let mut next_scan = Instant::now();
    let mut input_report = [0u8; 78];
    let mut next_process_check = Instant::now();
    let mut last_conflicts = Vec::<&'static str>::new();
    let mut first_process_check = true;
    let mut last_foreground = None::<String>;
    let mut raw_reports = 0u32;
    let mut sequence_gaps = 0u32;
    let mut invalid_reports = 0u32;
    let mut next_rate = Instant::now() + Duration::from_secs(1);
    loop {
        if Instant::now() >= next_rate {
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
            if status.foreground_exe != last_foreground {
                emit(
                    &proxy,
                    json!({"type":"foreground","exe":status.foreground_exe.as_deref().unwrap_or("")}),
                );
                last_foreground = status.foreground_exe;
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
        if let Some(connected) = device.as_mut() {
            if connected
                .trigger_until
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                let off = protocol::encode_trigger(&TriggerSetting {
                    mode: "off".into(),
                    strength: 0.0,
                    curve: vec![0.0; 11],
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
                    if let Some(input) = protocol::parse_input(&input_report[..length]) {
                        connected.last_valid_report = Instant::now();
                        raw_reports += 1;
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
                        mouse.update(&input);
                        gyro.update(&input);
                        remap.update(&input);
                        if let Some(event) = xinput.update(&input) {
                            emit(&proxy, event.clone());
                            if let Some(message) = event["message"].as_str() {
                                emit(&proxy, json!({ "type": "error", "message": message }));
                            }
                        }
                        if input.panic_combo && !connected.panic_latched {
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
                        if connected.last_input_sent.elapsed() >= Duration::from_millis(32) {
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
