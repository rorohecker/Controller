//! Pure DualSense report parsing and output construction.
//! Layout references: Linux hid-playstation.c and Nielk1's public trigger research.

use serde::{Deserialize, Serialize};

pub const SONY_VID: u16 = 0x054c;
pub const DUALSENSE_PID: u16 = 0x0ce6;
pub const EDGE_PID: u16 = 0x0df2;
pub const USB_OUTPUT_LEN: usize = 63;
pub const BT_OUTPUT_LEN: usize = 78;
pub const BT_HAPTIC_LEN: usize = 141;
pub const MAX_TRIGGER_LEVEL: u8 = 6; // conservative ceiling; firmware supports 0..8

/// The 2.21 firmware feature version introduced compatible vibration v2.
pub fn rumble_flags(feature_version: Option<u16>, is_edge: bool) -> (u8, u8) {
    if is_edge || feature_version.is_some_and(|version| version >= 0x0215) {
        (0x02, 0x04)
    } else {
        (0x03, 0)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSetting {
    pub mode: String,
    pub strength: f32,
    pub curve: Vec<f32>,
    #[serde(default)]
    pub zone_forces: Option<Vec<f32>>,
    #[serde(default)]
    pub start_zone: Option<u8>,
    #[serde(default)]
    pub end_zone: Option<u8>,
    #[serde(default)]
    pub snap_strength: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_cap: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_point: Option<f32>,
}

impl TriggerSetting {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .release_point
            .is_some_and(|point| !point.is_finite() || !(0.1..=1.0).contains(&point))
        {
            return Err("Bow release point must be between 10 and 100 percent");
        }
        if !matches!(
            self.mode.as_str(),
            "off" | "resistance" | "weapon" | "vibration" | "bow"
        ) {
            return Err("Unknown trigger mode");
        }
        if !self.strength.is_finite() || !(0.0..=100.0).contains(&self.strength) {
            return Err("Trigger strength must be 0–100");
        }
        if self.curve.len() != 11
            || self
                .curve
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("Curve must have eleven points between 0 and 1");
        }
        if self.zone_forces.as_ref().is_some_and(|zones| {
            zones.len() != 10
                || zones
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        }) {
            return Err("Zone forces must have ten values between 0 and 1");
        }
        if self
            .snap_strength
            .is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
        {
            return Err("Snap strength must be 0–100");
        }
        if self
            .force_cap
            .is_some_and(|level| !(1..=8).contains(&level))
        {
            return Err("Trigger force cap must be 1–8");
        }
        if matches!(self.mode.as_str(), "weapon" | "bow") {
            let (max_start, max_end) = if self.mode == "weapon" {
                (7, 8)
            } else {
                (7, 9)
            };
            if self
                .start_zone
                .is_some_and(|zone| zone > max_start || (self.mode == "weapon" && zone < 2))
                || self.end_zone.is_some_and(|zone| zone > max_end || zone < 1)
                || self
                    .start_zone
                    .zip(self.end_zone)
                    .is_some_and(|(start, end)| end <= start)
            {
                return Err("Break zones must be ordered within the trigger's range");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct TouchPoint {
    pub x: f32,
    pub y: f32,
}

/// Charge nibble from Linux hid-playstation `DS_STATUS0_CHARGING`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChargeState {
    Discharging,
    Charging,
    Full,
    VoltageOrTempFault,
    TemperatureFault,
    ChargingError,
    Unknown,
}

impl ChargeState {
    pub fn from_code(code: u8) -> Self {
        match code {
            0x0 => Self::Discharging,
            0x1 => Self::Charging,
            0x2 => Self::Full,
            0xa => Self::VoltageOrTempFault,
            0xb => Self::TemperatureFault,
            0xf => Self::ChargingError,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Discharging => "Discharging",
            Self::Charging => "Charging",
            Self::Full => "Full",
            Self::VoltageOrTempFault => "Voltage / temp fault",
            Self::TemperatureFault => "Temperature fault",
            Self::ChargingError => "Charging error",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    /// Approximate percent 0–100 when the charge nibble is valid.
    pub percent: Option<u8>,
    /// Raw capacity nibble 0–10 (each step ≈ 10%).
    pub level_tenths: Option<u8>,
    pub charging: bool,
    pub charge_state: ChargeState,
    pub charge_code: u8,
    /// Uncalibrated, reserved byte at body[31]. Absent on short Bluetooth reports.
    pub sensor_byte: Option<u8>,
    pub headphone_jack: bool,
    pub microphone_jack: bool,
    pub microphone_muted: bool,
}

impl BatteryInfo {
    pub fn from_status(status0: u8, status1: u8, sensor_byte: Option<u8>) -> Self {
        let level = status0 & 0x0f;
        let charge_code = status0 >> 4;
        let charge_state = ChargeState::from_code(charge_code);
        let (percent, level_tenths) = match charge_state {
            ChargeState::Full => (Some(100), Some(level.min(10))),
            ChargeState::Discharging | ChargeState::Charging => {
                let tenths = level.min(10);
                (Some((tenths * 10 + 5).min(100)), Some(tenths))
            }
            _ => (None, if level <= 10 { Some(level) } else { None }),
        };
        Self {
            percent,
            level_tenths,
            charging: matches!(charge_state, ChargeState::Charging),
            charge_state,
            charge_code,
            sensor_byte,
            headphone_jack: status1 & 0x01 != 0,
            microphone_jack: status1 & 0x02 != 0,
            microphone_muted: status1 & 0x04 != 0,
        }
    }

    fn empty() -> Self {
        Self {
            percent: None,
            level_tenths: None,
            charging: false,
            charge_state: ChargeState::Unknown,
            charge_code: 0,
            sensor_byte: None,
            headphone_jack: false,
            microphone_jack: false,
            microphone_muted: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputState {
    pub buttons: [f32; 23],
    pub axes: [f32; 4],
    /// Convenience mirror of `battery_info.percent`.
    pub battery: Option<u8>,
    pub charging: bool,
    pub battery_info: BatteryInfo,
    pub touch: [Option<TouchPoint>; 2],
    pub gyro: [i16; 3],
    /// Incrementing report sequence in full USB/Bluetooth reports.
    pub sequence: Option<u8>,
    pub panic_combo: bool,
    pub full_report: bool,
}

fn normalized_axis(byte: u8) -> f32 {
    ((byte as f32 - 128.0) / 127.0).clamp(-1.0, 1.0)
}

fn touch_point(bytes: &[u8]) -> Option<TouchPoint> {
    if bytes.len() < 4 || bytes[0] & 0x80 != 0 {
        return None;
    }
    let x = bytes[1] as u16 | (((bytes[2] & 0x0f) as u16) << 8);
    let y = ((bytes[2] >> 4) as u16) | ((bytes[3] as u16) << 4);
    Some(TouchPoint {
        x: (x as f32 / 1920.0).clamp(0.0, 1.0),
        y: (y as f32 / 1080.0).clamp(0.0, 1.0),
    })
}

fn decode_buttons(b0: u8, b1: u8, b2: u8, l2: u8, r2: u8) -> [f32; 23] {
    let mut buttons = [0.0; 23];
    buttons[0] = ((b0 >> 5) & 1) as f32;
    buttons[1] = ((b0 >> 6) & 1) as f32;
    buttons[2] = ((b0 >> 4) & 1) as f32;
    buttons[3] = ((b0 >> 7) & 1) as f32;
    for i in 0..8 {
        buttons[4 + i] = ((b1 >> i) & 1) as f32;
    }
    buttons[6] = l2 as f32 / 255.0;
    buttons[7] = r2 as f32 / 255.0;
    let hat = b0 & 0x0f;
    buttons[12] = u8::from(matches!(hat, 0 | 1 | 7)) as f32;
    buttons[13] = u8::from(matches!(hat, 3..=5)) as f32;
    buttons[14] = u8::from(matches!(hat, 5..=7)) as f32;
    buttons[15] = u8::from(matches!(hat, 1..=3)) as f32;
    buttons[16] = (b2 & 1) as f32;
    buttons[17] = ((b2 >> 1) & 1) as f32;
    buttons[18] = ((b2 >> 2) & 1) as f32;
    buttons[19] = ((b2 >> 4) & 1) as f32;
    buttons[20] = ((b2 >> 5) & 1) as f32;
    buttons[21] = ((b2 >> 6) & 1) as f32;
    buttons[22] = ((b2 >> 7) & 1) as f32;
    buttons
}

pub fn parse_input(data: &[u8]) -> Option<InputState> {
    if data.len() == 10 && data[0] == 0x01 {
        let battery_info = BatteryInfo::empty();
        return Some(InputState {
            buttons: decode_buttons(data[5], data[6], data[7], data[8], data[9]),
            axes: [
                normalized_axis(data[1]),
                normalized_axis(data[2]),
                normalized_axis(data[3]),
                normalized_axis(data[4]),
            ],
            battery: None,
            charging: false,
            battery_info,
            touch: [None, None],
            gyro: [0; 3],
            sequence: None,
            panic_combo: data[7] & 1 != 0 && data[6] & 0x20 != 0,
            full_report: false,
        });
    }
    let start = match (data.first().copied()?, data.len()) {
        (0x01, 64..) => 1,
        (0x31, 78..) => {
            let expected = u32::from_le_bytes(data[74..78].try_into().ok()?);
            if crc32_seeded(0xa1, &data[..74]) != expected {
                return None;
            }
            2
        }
        _ => return None,
    };
    let body = data.get(start..start + 63)?;
    let b0 = body[7];
    let b1 = body[8];
    let b2 = body[9];
    let buttons = decode_buttons(b0, b1, b2, body[4], body[5]);
    // Linux labels body[31] reserved2. Expose the raw value without inferring degrees or health.
    let battery_info = BatteryInfo::from_status(body[52], body[53], Some(body[31]));
    let gyro = [
        i16::from_le_bytes([body[15], body[16]]),
        i16::from_le_bytes([body[17], body[18]]),
        i16::from_le_bytes([body[19], body[20]]),
    ];
    Some(InputState {
        buttons,
        axes: [
            normalized_axis(body[0]),
            normalized_axis(body[1]),
            normalized_axis(body[2]),
            normalized_axis(body[3]),
        ],
        battery: battery_info.percent,
        charging: battery_info.charging,
        battery_info,
        touch: [touch_point(&body[32..36]), touch_point(&body[36..40])],
        gyro,
        sequence: Some(body[6]),
        panic_combo: b2 & 1 != 0 && b1 & 0x20 != 0, // PS + Options
        full_report: true,
    })
}

pub fn encode_trigger(setting: &TriggerSetting) -> Result<[u8; 11], &'static str> {
    setting.validate()?;
    let max_level = setting.force_cap.unwrap_or(MAX_TRIGGER_LEVEL);
    let mut block = [0u8; 11];
    if setting.mode == "off" || setting.strength < 1.0 {
        block[0] = 0x05;
        return Ok(block);
    }
    let mut levels = [0u8; 10];
    for (i, level) in levels.iter_mut().enumerate() {
        let midpoint = setting
            .zone_forces
            .as_ref()
            .map_or((setting.curve[i] + setting.curve[i + 1]) / 2.0, |zones| {
                zones[i]
            });
        *level = (midpoint * setting.strength / 100.0 * max_level as f32).round() as u8;
    }
    if matches!(setting.mode.as_str(), "resistance" | "vibration")
        && levels.iter().all(|&level| level == 0)
    {
        block[0] = 0x05;
        return Ok(block);
    }
    match setting.mode.as_str() {
        "resistance" | "vibration" => {
            block[0] = if setting.mode == "vibration" {
                0x26
            } else {
                0x21
            };
            let mut active = 0u16;
            let mut packed = 0u32;
            for (i, level) in levels.iter().enumerate() {
                if *level > 0 {
                    active |= 1 << i;
                    packed |= ((*level - 1) as u32) << (i * 3);
                }
            }
            block[1..3].copy_from_slice(&active.to_le_bytes());
            block[3..7].copy_from_slice(&packed.to_le_bytes());
            if setting.mode == "vibration" {
                block[9] = 45;
            }
        }
        "weapon" => {
            block[0] = 0x25;
            let inferred_start = (2..=7)
                .max_by(|&a, &b| {
                    (setting.curve[a + 1] - setting.curve[a])
                        .total_cmp(&(setting.curve[b + 1] - setting.curve[b]))
                })
                .unwrap_or(4);
            let start = setting
                .start_zone
                .map(usize::from)
                .unwrap_or(inferred_start);
            let stop = setting
                .end_zone
                .map(usize::from)
                .unwrap_or(start + 1)
                .clamp(start + 1, 8);
            let mask = (1u16 << start) | (1u16 << stop);
            block[1..3].copy_from_slice(&mask.to_le_bytes());
            block[3] = ((setting.strength / 100.0 * max_level as f32).round() as u8).max(1) - 1;
        }
        "bow" => {
            // Unofficial mode 0x22. Choose a start and peak from the authored curve;
            // keep both force components below our normal six-level ceiling.
            let inferred_peak = (1..=8)
                .max_by(|&a, &b| setting.curve[a].total_cmp(&setting.curve[b]))
                .unwrap_or(8);
            let inferred_start = (0..inferred_peak)
                .find(|&i| setting.curve[i] >= setting.curve[inferred_peak] * 0.15)
                .unwrap_or(0);
            let start = setting
                .start_zone
                .map(usize::from)
                .unwrap_or(inferred_start);
            let peak = setting
                .end_zone
                .map(usize::from)
                .unwrap_or(inferred_peak)
                .clamp(start + 1, 9);
            let mask = (1u16 << start) | (1u16 << peak);
            let force =
                ((setting.strength / 100.0 * max_level as f32).round() as u8).clamp(1, max_level);
            let snap = setting
                .snap_strength
                .map(|strength| {
                    ((strength / 100.0 * max_level as f32).round() as u8).clamp(1, max_level)
                })
                .unwrap_or((force / 2).max(1));
            block[0] = 0x22;
            block[1..3].copy_from_slice(&mask.to_le_bytes());
            block[3] = (force - 1) | ((snap - 1) << 3);
        }
        _ => unreachable!(),
    }
    Ok(block)
}

#[allow(clippy::too_many_arguments)] // Mirrors the independent protocol control fields.
pub fn usb_report(
    valid0: u8,
    valid1: u8,
    valid2: u8,
    right: Option<[u8; 11]>,
    left: Option<[u8; 11]>,
    rgb: Option<[u8; 3]>,
    rumble: Option<[u8; 2]>,
    release_leds: bool,
) -> [u8; USB_OUTPUT_LEN] {
    let mut report = [0u8; USB_OUTPUT_LEN];
    report[0] = 0x02;
    report[1] = valid0;
    report[2] = valid1 | if release_leds { 0x08 } else { 0 };
    report[39] = valid2;
    if let Some(right) = right {
        report[11..22].copy_from_slice(&right);
    }
    if let Some(left) = left {
        report[22..33].copy_from_slice(&left);
    }
    if let Some([r, g, b]) = rgb {
        report[45] = r;
        report[46] = g;
        report[47] = b;
    }
    if let Some([left, right]) = rumble {
        report[3] = right;
        report[4] = left;
    }
    report
}

pub fn player_leds_report(mask: u8) -> [u8; USB_OUTPUT_LEN] {
    let mut report = usb_report(0, 0x10, 0, None, None, None, None, false);
    report[44] = mask & 0x1f;
    report
}

/// Clear the controller's power-save audio mute so USB PCM can reach the grip coils.
/// Do not set HAPTICS_SELECT here: that hands the coils to classic motor rumble.
pub fn audio_haptics_enable_report() -> [u8; USB_OUTPUT_LEN] {
    let mut report = usb_report(0, 0x02, 0, None, None, None, None, false);
    report[10] = 0;
    report
}

/// The mute button is reported as input; the host controls the microphone and its LED.
pub fn microphone_mute_report(muted: bool) -> [u8; USB_OUTPUT_LEN] {
    let mut report = usb_report(0, 0x03, 0, None, None, None, None, false);
    report[9] = u8::from(muted);
    report[10] = if muted { 0x10 } else { 0 };
    report
}

/// Select the built-in speaker for USB channels 1/2. The speaker uses channel 2.
/// Audio control offsets follow Sony's Linux hid-playstation output report layout.
pub fn speaker_route_report(enabled: bool) -> [u8; USB_OUTPUT_LEN] {
    let mut report = usb_report(
        0x80 | if enabled { 0x20 } else { 0 },
        0x02 | if enabled { 0x80 } else { 0 },
        0,
        None,
        None,
        None,
        None,
        false,
    );
    if enabled {
        report[6] = 0x64; // speaker volume
        report[8] = 3 << 4; // speaker-only output path
        report[38] = 0x02; // modest speaker preamp gain
    }
    report[10] = 0; // clear speaker and haptic audio mute
    report
}

pub fn bt_report(usb: &[u8; USB_OUTPUT_LEN], sequence: u8) -> [u8; BT_OUTPUT_LEN] {
    let mut report = [0u8; BT_OUTPUT_LEN];
    report[0] = 0x31;
    report[1] = (sequence & 0x0f) << 4;
    report[2] = 0x10;
    report[3..50].copy_from_slice(&usb[1..48]);
    let crc = crc32_seeded(0xa2, &report[..74]);
    report[74..78].copy_from_slice(&crc.to_le_bytes());
    report
}

/// Experimental Bluetooth voice-coil frame, kept separate from 0x31 controls.
/// Wire format and 3 kHz signed stereo PCM basis: egormanga/SAxense (MPL-2.0 research).
/// This is a pure encoder; the app does not send 0x32 frames without hardware validation.
pub fn bt_haptic_report(pcm: &[i8; 64], sequence: u8) -> [u8; BT_HAPTIC_LEN] {
    let mut report = [0u8; BT_HAPTIC_LEN];
    report[0] = 0x32;
    report[1] = (sequence & 0x0f) << 4;
    report[2..11].copy_from_slice(&[0x91, 7, 0xfe, 0, 0, 0, 0, 0xff, sequence]);
    report[11..13].copy_from_slice(&[0x92, 64]);
    for (destination, sample) in report[13..77].iter_mut().zip(pcm.iter()) {
        *destination = *sample as u8;
    }
    let crc = crc32_seeded(0xa2, &report[..137]);
    report[137..141].copy_from_slice(&crc.to_le_bytes());
    report
}

pub fn crc32_seeded(seed: u8, data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in std::iter::once(&seed).chain(data.iter()) {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_mute_controls_the_hardware_and_its_light() {
        let muted = microphone_mute_report(true);
        assert_eq!((muted[2], muted[9], muted[10]), (0x03, 1, 0x10));
        let live = microphone_mute_report(false);
        assert_eq!((live[2], live[9], live[10]), (0x03, 0, 0));
        assert_eq!(muted[10] & 0x40, 0);
    }

    fn linear(mode: &str) -> TriggerSetting {
        TriggerSetting {
            mode: mode.into(),
            strength: 60.0,
            curve: (0..=10).map(|i| i as f32 / 10.0).collect(),
            ..TriggerSetting::default()
        }
    }

    #[test]
    fn usb_layout_and_flags() {
        let right = encode_trigger(&linear("resistance")).unwrap();
        let left = encode_trigger(&linear("weapon")).unwrap();
        let report = usb_report(
            0x0c,
            0x04,
            0,
            Some(right),
            Some(left),
            Some([1, 2, 3]),
            None,
            false,
        );
        assert_eq!(report.len(), 63);
        assert_eq!((report[0], report[1], report[2]), (0x02, 0x0c, 0x04));
        assert_eq!(report[11], 0x21);
        assert_eq!(report[22], 0x25);
        assert_eq!(&report[45..48], &[1, 2, 3]);
    }

    #[test]
    fn player_indicators_are_independent_from_rgb_lightbar() {
        let report = player_leds_report(0b1_0101);
        assert_eq!((report[0], report[1], report[2]), (0x02, 0, 0x10));
        assert_eq!(report[44], 0b1_0101);
        assert_eq!(&report[45..48], &[0, 0, 0]);
    }

    #[test]
    fn speaker_route_sets_audio_path_and_can_restore_headphones() {
        let haptics = audio_haptics_enable_report();
        assert_eq!(
            [haptics[0], haptics[1], haptics[2], haptics[10]],
            [0x02, 0, 0x02, 0]
        );
        let on = speaker_route_report(true);
        assert_eq!(
            [on[0], on[1], on[2], on[6], on[8], on[10], on[38]],
            [0x02, 0xa0, 0x82, 0x64, 0x30, 0, 0x02]
        );
        let off = speaker_route_report(false);
        assert_eq!(
            [off[0], off[1], off[2], off[8], off[10]],
            [0x02, 0x80, 0x02, 0, 0]
        );
    }

    #[test]
    fn bt_framing_and_crc() {
        let usb = usb_report(0, 0x04, 0, None, None, Some([6, 7, 8]), None, false);
        let bt = bt_report(&usb, 5);
        assert_eq!((bt[0], bt[1], bt[2]), (0x31, 0x50, 0x10));
        assert_eq!(&bt[47..50], &[6, 7, 8]);
        assert_eq!(
            u32::from_le_bytes(bt[74..78].try_into().unwrap()),
            0x82d3_7342 // independent Python zlib CRC32 of the seeded frame
        );
    }

    #[test]
    fn experimental_bt_haptic_framing_matches_independent_crc() {
        let report = bt_haptic_report(&[0; 64], 3);
        assert_eq!(report.len(), 141);
        assert_eq!(&report[..4], &[0x32, 0x30, 0x91, 7]);
        assert_eq!(&report[9..13], &[0xff, 3, 0x92, 64]);
        assert_eq!(
            u32::from_le_bytes(report[137..141].try_into().unwrap()),
            0x2d0f_ab86
        );
    }

    #[test]
    fn validates_curve_before_hardware_output() {
        let mut invalid = linear("resistance");
        invalid.curve[3] = f32::NAN;
        assert!(encode_trigger(&invalid).is_err());
        invalid.curve[3] = 0.3;
        invalid.strength = 101.0;
        assert!(encode_trigger(&invalid).is_err());
    }

    #[test]
    fn independent_zone_forces_encode_only_the_selected_zone() {
        let mut setting = linear("resistance");
        setting.strength = 100.0;
        setting.curve = vec![0.0; 11];
        let mut zones = vec![0.0; 10];
        zones[3] = 1.0;
        setting.zone_forces = Some(zones);
        let encoded = encode_trigger(&setting).unwrap();
        assert_eq!(encoded[0], 0x21);
        assert_eq!(u16::from_le_bytes([encoded[1], encoded[2]]), 1 << 3);
        assert_eq!(
            u32::from_le_bytes(encoded[3..7].try_into().unwrap()),
            5 << 9
        );
        setting.zone_forces = Some(vec![0.5; 9]);
        assert!(encode_trigger(&setting).is_err());
    }

    #[test]
    fn silent_curves_release_triggers_and_firmware_selects_rumble_flags() {
        let zero = TriggerSetting {
            mode: "vibration".into(),
            strength: 75.0,
            curve: vec![0.0; 11],
            ..TriggerSetting::default()
        };
        assert_eq!(encode_trigger(&zero).unwrap()[0], 0x05);
        assert_eq!(rumble_flags(Some(0x0214), false), (0x03, 0));
        assert_eq!(rumble_flags(Some(0x0215), false), (0x02, 0x04));
        assert_eq!(rumble_flags(None, true), (0x02, 0x04));
    }

    #[test]
    fn bow_uses_two_zones_and_bounded_force() {
        let mut setting = linear("bow");
        setting.strength = 100.0;
        let encoded = encode_trigger(&setting).unwrap();
        assert_eq!(encoded[0], 0x22);
        assert_eq!(u16::from_le_bytes([encoded[1], encoded[2]]).count_ones(), 2);
        assert!(encoded[3] & 7 < MAX_TRIGGER_LEVEL);
        assert!(encoded[3] >> 3 < MAX_TRIGGER_LEVEL);
        setting.force_cap = Some(8);
        setting.snap_strength = Some(100.0);
        let full = encode_trigger(&setting).unwrap();
        assert_eq!(full[3] & 7, 7);
        assert_eq!(full[3] >> 3, 7);
        setting.force_cap = Some(9);
        assert!(encode_trigger(&setting).is_err());
    }

    #[test]
    fn authored_weapon_and_bow_break_points_encode_requested_zones() {
        let mut weapon = linear("weapon");
        weapon.start_zone = Some(3);
        weapon.end_zone = Some(7);
        let encoded = encode_trigger(&weapon).unwrap();
        assert_eq!(
            u16::from_le_bytes([encoded[1], encoded[2]]),
            (1 << 3) | (1 << 7)
        );

        let mut bow = linear("bow");
        bow.start_zone = Some(2);
        bow.end_zone = Some(8);
        bow.snap_strength = Some(100.0);
        let encoded = encode_trigger(&bow).unwrap();
        assert_eq!(
            u16::from_le_bytes([encoded[1], encoded[2]]),
            (1 << 2) | (1 << 8)
        );
        assert_eq!(encoded[3] >> 3, MAX_TRIGGER_LEVEL - 1);
        bow.end_zone = Some(2);
        assert!(encode_trigger(&bow).is_err());
    }

    #[test]
    fn usb_input_positions() {
        let mut report = [0u8; 64];
        report[0] = 1;
        report[1] = 128;
        report[2] = 128;
        report[5] = 255;
        report[6] = 64;
        report[8] = 0x20;
        report[9] = 0x20;
        report[32] = 38; // uncalibrated reserved byte at body[31]
        report[53] = 0x15; // charging, approximately 55%
        report[54] = 0x01; // headphone jack
        let state = parse_input(&report).unwrap();
        assert_eq!(state.buttons[0], 1.0);
        assert_eq!(state.buttons[9], 1.0);
        assert_eq!(state.buttons[6], 1.0);
        assert_eq!(state.battery, Some(55));
        assert!(state.charging);
        assert_eq!(state.battery_info.charge_state, ChargeState::Charging);
        assert_eq!(state.battery_info.sensor_byte, Some(38));
        assert!(state.battery_info.headphone_jack);
        assert!(!state.battery_info.microphone_jack);
        assert_eq!(serde_json::to_value(&state).unwrap()["fullReport"], true);
        assert_eq!(
            serde_json::to_value(&state).unwrap()["batteryInfo"]["chargeState"],
            "charging"
        );
    }

    #[test]
    fn battery_fault_and_raw_sensor_byte() {
        let mut report = [0u8; 64];
        report[0] = 1;
        report[32] = 56;
        report[53] = 0xb0; // temperature fault
        let fault = parse_input(&report).unwrap();
        assert!(fault.battery.is_none());
        assert_eq!(fault.battery_info.sensor_byte, Some(56));
        assert_eq!(
            fault.battery_info.charge_state,
            ChargeState::TemperatureFault
        );

        report[53] = 0x05; // discharging ~55%
        report[32] = 1;
        let raw = parse_input(&report).unwrap();
        assert_eq!(raw.battery_info.sensor_byte, Some(1));

        report[32] = 0;
        let zero = parse_input(&report).unwrap();
        assert_eq!(zero.battery_info.sensor_byte, Some(0));

        report[53] = 0x20; // full
        report[32] = 40;
        let full = parse_input(&report).unwrap();
        assert_eq!(full.battery, Some(100));
        assert_eq!(full.battery_info.charge_state, ChargeState::Full);
        assert!(!full.charging);
    }

    #[test]
    fn bt_input_checks_crc_and_edge_buttons() {
        let mut report = [0u8; 78];
        report[0] = 0x31;
        report[9] = 0x20; // cross: body starts at byte 2
        report[11] = 0xf4; // mic mute and Edge FN/paddle bits
        let crc = crc32_seeded(0xa1, &report[..74]);
        report[74..78].copy_from_slice(&crc.to_le_bytes());
        let state = parse_input(&report).expect("valid Bluetooth report");
        assert_eq!(state.buttons[0], 1.0);
        assert_eq!(&state.buttons[18..23], &[1.0; 5]);
        report[11] ^= 0x10;
        assert!(parse_input(&report).is_none());
    }

    #[test]
    fn bt_minimal_input_preserves_buttons_and_axes() {
        let report = [0x01, 0x7d, 0x7e, 0x83, 0x82, 0x28, 0x01, 0, 0x80, 0];
        let state = parse_input(&report).expect("Bluetooth short input");
        assert!(!state.full_report);
        assert_eq!(state.buttons[0], 1.0);
        assert_eq!(state.buttons[4], 1.0);
        assert!((state.buttons[6] - 0.5).abs() < 0.01);
        assert!(state.touch[0].is_none());
    }
}
