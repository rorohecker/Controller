//! Pure DualSense report parsing and output construction.
//! Layout references: Linux hid-playstation.c and Nielk1's public trigger research.

use serde::{Deserialize, Serialize};

pub const SONY_VID: u16 = 0x054c;
pub const DUALSENSE_PID: u16 = 0x0ce6;
pub const EDGE_PID: u16 = 0x0df2;
pub const USB_OUTPUT_LEN: usize = 63;
pub const BT_OUTPUT_LEN: usize = 78;
pub const MAX_TRIGGER_LEVEL: u8 = 6; // conservative ceiling; firmware supports 0..8

/// The 2.21 firmware feature version introduced compatible vibration v2.
pub fn rumble_flags(feature_version: Option<u16>, is_edge: bool) -> (u8, u8) {
    if is_edge || feature_version.is_some_and(|version| version >= 0x0215) {
        (0x02, 0x04)
    } else {
        (0x03, 0)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerSetting {
    pub mode: String,
    pub strength: f32,
    pub curve: Vec<f32>,
}

impl TriggerSetting {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(
            self.mode.as_str(),
            "off" | "resistance" | "weapon" | "vibration"
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
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct TouchPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputState {
    pub buttons: [f32; 23],
    pub axes: [f32; 4],
    pub battery: Option<u8>,
    pub charging: bool,
    pub touch: [Option<TouchPoint>; 2],
    pub gyro: [i16; 3],
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
            touch: [None, None],
            gyro: [0; 3],
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
    let battery_status = body[52];
    let level = battery_status & 0x0f;
    let charge_code = battery_status >> 4;
    let battery = if charge_code <= 2 {
        Some(if charge_code == 2 {
            100
        } else {
            (level * 10 + 5).min(100)
        })
    } else {
        None
    };
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
        battery,
        charging: charge_code == 1,
        touch: [touch_point(&body[32..36]), touch_point(&body[36..40])],
        gyro,
        panic_combo: b2 & 1 != 0 && b1 & 0x20 != 0, // PS + Options
        full_report: true,
    })
}

pub fn encode_trigger(setting: &TriggerSetting) -> Result<[u8; 11], &'static str> {
    setting.validate()?;
    let mut block = [0u8; 11];
    if setting.mode == "off" || setting.strength < 1.0 {
        block[0] = 0x05;
        return Ok(block);
    }
    let mut levels = [0u8; 10];
    for (i, level) in levels.iter_mut().enumerate() {
        let midpoint = (setting.curve[i] + setting.curve[i + 1]) / 2.0;
        *level = (midpoint * setting.strength / 100.0 * MAX_TRIGGER_LEVEL as f32).round() as u8;
    }
    if setting.mode != "weapon" && levels.iter().all(|&level| level == 0) {
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
            let start = (2..=7)
                .max_by(|&a, &b| {
                    (setting.curve[a + 1] - setting.curve[a])
                        .total_cmp(&(setting.curve[b + 1] - setting.curve[b]))
                })
                .unwrap_or(4);
            let stop = (start + 1).min(8);
            let mask = (1u16 << start) | (1u16 << stop);
            block[1..3].copy_from_slice(&mask.to_le_bytes());
            block[3] =
                ((setting.strength / 100.0 * MAX_TRIGGER_LEVEL as f32).round() as u8).max(1) - 1;
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

    fn linear(mode: &str) -> TriggerSetting {
        TriggerSetting {
            mode: mode.into(),
            strength: 60.0,
            curve: (0..=10).map(|i| i as f32 / 10.0).collect(),
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
    fn validates_curve_before_hardware_output() {
        let mut invalid = linear("resistance");
        invalid.curve[3] = f32::NAN;
        assert!(encode_trigger(&invalid).is_err());
        invalid.curve[3] = 0.3;
        invalid.strength = 101.0;
        assert!(encode_trigger(&invalid).is_err());
    }

    #[test]
    fn silent_curves_release_triggers_and_firmware_selects_rumble_flags() {
        let zero = TriggerSetting {
            mode: "vibration".into(),
            strength: 75.0,
            curve: vec![0.0; 11],
        };
        assert_eq!(encode_trigger(&zero).unwrap()[0], 0x05);
        assert_eq!(rumble_flags(Some(0x0214), false), (0x03, 0));
        assert_eq!(rumble_flags(Some(0x0215), false), (0x02, 0x04));
        assert_eq!(rumble_flags(None, true), (0x02, 0x04));
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
        report[53] = 0x15; // charging, approximately 55%
        let state = parse_input(&report).unwrap();
        assert_eq!(state.buttons[0], 1.0);
        assert_eq!(state.buttons[9], 1.0);
        assert_eq!(state.buttons[6], 1.0);
        assert_eq!(state.battery, Some(55));
        assert!(state.charging);
        assert_eq!(serde_json::to_value(&state).unwrap()["fullReport"], true);
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
