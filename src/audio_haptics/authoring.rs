//! Bounded, portable tactile clips. HID timing is quantized to 10 ms; audio
//! lanes retain the audio clock. These are output requests, not measured force.
use aftertouch_protocol::{TriggerSetting, encode_trigger};
use serde::{Deserialize, Serialize};

pub const QUANTUM_MS: u32 = 10;
pub fn rounded_ms(value: u32) -> u32 {
    value.div_ceil(QUANTUM_MS) * QUANTUM_MS
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectClip {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub source_url: String,
    pub cues: Vec<Cue>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cue {
    pub lane: String,
    pub kind: String,
    pub start_ms: u32,
    pub duration_ms: u32,
    pub strength: f32,
    #[serde(default)]
    pub from_strength: f32,
    #[serde(default = "frequency")]
    pub frequency: u16,
    #[serde(default = "shape")]
    pub shape: String,
    #[serde(default = "start_zone")]
    pub start_zone: u8,
    #[serde(default = "end_zone")]
    pub end_zone: u8,
}
fn frequency() -> u16 {
    90
}
fn shape() -> String {
    "sine".into()
}
fn start_zone() -> u8 {
    2
}
fn end_zone() -> u8 {
    7
}

impl Cue {
    pub fn start(&self) -> u32 {
        rounded_ms(self.start_ms)
    }
    pub fn end(&self) -> u32 {
        rounded_ms(self.start_ms + self.duration_ms).max(self.start() + QUANTUM_MS)
    }
    pub fn trigger(&self, elapsed_ms: u32, cap: u8) -> Result<[u8; 11], &'static str> {
        let strength = if self.kind == "load" {
            let progress =
                elapsed_ms.saturating_sub(self.start()) as f32 / (self.end() - self.start()) as f32;
            self.from_strength + (self.strength - self.from_strength) * progress.clamp(0.0, 1.0)
        } else {
            self.strength
        };
        let mode = match self.kind.as_str() {
            "release" => "off",
            "kick" => "weapon",
            _ => "resistance",
        };
        let zones = (0..10)
            .map(|i| {
                if i >= self.start_zone && i <= self.end_zone {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();
        encode_trigger(&TriggerSetting {
            mode: mode.into(),
            strength,
            curve: vec![1.0; 11],
            zone_forces: Some(zones),
            start_zone: Some(self.start_zone),
            end_zone: Some(self.end_zone),
            force_cap: Some(8),
            ..Default::default()
        })
        .map(|block| limit_trigger(block, cap))
    }
}
impl EffectClip {
    pub fn validate(&self) -> Result<(), &'static str> {
        let text = |s: &str, max| s.chars().count() <= max && !s.contains(char::is_control);
        if self.id.is_empty()
            || !text(&self.id, 80)
            || self.name.trim().is_empty()
            || !text(&self.name, 40)
            || self.tags.len() > 8
            || self.tags.iter().any(|s| !text(s, 24))
            || !text(&self.author, 80)
            || !text(&self.license, 120)
            || self.source_url.len() > 1024
            || (!self.source_url.is_empty()
                && (!self.source_url.starts_with("https://")
                    || self.source_url.contains(char::is_whitespace)
                    || self.source_url.contains(char::is_control)))
            || self.cues.is_empty()
            || self.cues.len() > 32
        {
            return Err("A clip needs a name and 1–32 cues");
        }
        for cue in &self.cues {
            let grip = matches!(cue.lane.as_str(), "leftGrip" | "rightGrip");
            if !grip && !matches!(cue.lane.as_str(), "leftTrigger" | "rightTrigger")
                || !(10..=3000).contains(&cue.duration_ms)
                || cue.start_ms > 7990
                || cue.start_ms.saturating_add(cue.duration_ms) > 8000
                || !cue.strength.is_finite()
                || !(0.0..=100.0).contains(&cue.strength)
                || !cue.from_strength.is_finite()
                || !(0.0..=100.0).contains(&cue.from_strength)
                || !(20..=300).contains(&cue.frequency)
                || !matches!(cue.shape.as_str(), "sine" | "square" | "saw" | "noise")
                || cue.start_zone > 7
                || cue.end_zone <= cue.start_zone
                || cue.end_zone > 8
                || (grip && !matches!(cue.kind.as_str(), "pulse" | "texture"))
                || (!grip && !matches!(cue.kind.as_str(), "load" | "wall" | "kick" | "release"))
            {
                return Err("Cue timing, strength, lane or trigger zones are out of range");
            }
        }
        for (i, a) in self
            .cues
            .iter()
            .enumerate()
            .filter(|(_, c)| c.lane.ends_with("Trigger"))
        {
            if self.cues[i + 1..]
                .iter()
                .any(|b| a.lane == b.lane && a.start() < b.end() && b.start() < a.end())
            {
                return Err("Trigger cues on the same lane cannot overlap after timing is rounded");
            }
        }
        Ok(())
    }
    pub fn duration_ms(&self) -> u32 {
        self.cues.iter().map(Cue::end).max().unwrap_or(0)
    }
    pub fn has_grips(&self) -> bool {
        self.cues.iter().any(|c| c.lane.ends_with("Grip"))
    }
    pub fn trigger_mask(&self) -> u8 {
        (if self.cues.iter().any(|c| c.lane == "leftTrigger") {
            8
        } else {
            0
        }) | (if self.cues.iter().any(|c| c.lane == "rightTrigger") {
            4
        } else {
            0
        })
    }
    pub fn triggers(&self, elapsed: u32, cap: u8) -> [Option<[u8; 11]>; 2] {
        ["leftTrigger", "rightTrigger"].map(|lane| {
            if !self.cues.iter().any(|c| c.lane == lane) {
                return None;
            }
            self.cues
                .iter()
                .find(|c| c.lane == lane && elapsed >= c.start() && elapsed < c.end())
                .map_or_else(
                    || Some([0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
                    |c| c.trigger(elapsed, cap).ok(),
                )
        })
    }
    pub fn sample(&self, seconds: f32, personal: &PersonalSettings) -> [f32; 2] {
        let ms = seconds * 1000.0;
        let mut output = [0.0; 2];
        for cue in &self.cues {
            let side = match cue.lane.as_str() {
                "leftGrip" => 0,
                "rightGrip" => 1,
                _ => continue,
            };
            let elapsed = (ms - cue.start() as f32) / 1000.0;
            let duration = (cue.end() - cue.start()) as f32 / 1000.0;
            if elapsed < 0.0 || elapsed >= duration {
                continue;
            }
            let edge = (duration / 4.0).min(0.005);
            let envelope = (elapsed / edge)
                .min((duration - elapsed) / edge)
                .clamp(0.0, 1.0);
            let phase = elapsed * f32::from(cue.frequency);
            let wave = match cue.shape.as_str() {
                "square" => {
                    if phase.fract() < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                "saw" => phase.fract() * 2.0 - 1.0,
                "noise" => (phase * 73.37).sin() * (phase * 127.91).cos(),
                _ => (phase * std::f32::consts::TAU).sin(),
            };
            output[side] +=
                wave * envelope * personal.authored_strength(cue.strength / 100.0, side);
        }
        output
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalSettings {
    pub enabled: bool,
    pub minimum: [u8; 2],
    pub maximum: [u8; 2],
    pub balance: i16,
    pub trigger_limit: u8,
}
impl Default for PersonalSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            minimum: [0; 2],
            maximum: [100; 2],
            balance: 0,
            trigger_limit: 8,
        }
    }
}
impl PersonalSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .minimum
            .iter()
            .zip(self.maximum)
            .any(|(&min, max)| min >= max || max > 100)
            || !(-100..=100).contains(&self.balance)
            || !(1..=8).contains(&self.trigger_limit)
        {
            return Err("Minimum must be below maximum; use 1–8 trigger force steps");
        }
        Ok(())
    }
    pub fn cap(&self) -> u8 {
        if self.enabled { self.trigger_limit } else { 8 }
    }
    pub fn side_limit(&self, side: usize) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        let attenuation = if side == 0 {
            self.balance.max(0)
        } else {
            -self.balance.min(0)
        } as f32
            / 100.0;
        f32::from(self.maximum[side]) / 100.0 * (1.0 - attenuation)
    }
    pub fn authored_strength(&self, value: f32, side: usize) -> f32 {
        if value <= 0.0 || !self.enabled {
            return value;
        }
        let floor = f32::from(self.minimum[side]) / f32::from(self.maximum[side].max(1));
        floor + (1.0 - floor) * value.clamp(0.0, 1.0)
    }
}

pub fn levels(block: &[u8; 11]) -> [u8; 10] {
    let mask = u16::from_le_bytes([block[1], block[2]]);
    let packed = u32::from_le_bytes([block[3], block[4], block[5], block[6]]);
    std::array::from_fn(|i| {
        if mask & (1 << i) == 0 {
            0
        } else {
            match block[0] {
                0x21 | 0x26 => ((packed >> (3 * i)) & 7) as u8 + 1,
                0x25 => (block[3] & 7) + 1,
                0x22 => {
                    if Some(i) == (0..10).find(|j| mask & (1 << j) != 0) {
                        (block[3] & 7) + 1
                    } else {
                        ((block[3] >> 3) & 7) + 1
                    }
                }
                _ => 0,
            }
        }
    })
}
pub fn limit_trigger(mut block: [u8; 11], cap: u8) -> [u8; 11] {
    let cap = cap.clamp(1, 8);
    match block[0] {
        0x21 | 0x26 => {
            let values = levels(&block);
            let mut packed = 0u32;
            for (i, value) in values.into_iter().enumerate() {
                if value > 0 {
                    packed |= u32::from(value.min(cap) - 1) << (3 * i);
                }
            }
            block[3..7].copy_from_slice(&packed.to_le_bytes());
        }
        0x25 => block[3] = ((block[3] & 7) + 1).min(cap) - 1,
        0x22 => {
            block[3] = (((block[3] & 7) + 1).min(cap) - 1)
                | (((((block[3] >> 3) & 7) + 1).min(cap) - 1) << 3)
        }
        0x05 | 0x00 => {}
        _ => block = [0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }
    block
}

pub fn validate_library(clips: &[EffectClip]) -> Result<(), &'static str> {
    let mut ids = std::collections::HashSet::new();
    if clips.len() > 64 {
        return Err("Up to 64 tactile clips can be saved");
    }
    for clip in clips {
        clip.validate()?;
        if !ids.insert(&clip.id) {
            return Err("Clip IDs must be distinct");
        }
    }
    Ok(())
}

pub fn inspect_clip(
    clip: &EffectClip,
    personal: &PersonalSettings,
) -> Result<serde_json::Value, String> {
    clip.validate().map_err(str::to_string)?;
    personal.validate().map_err(str::to_string)?;
    let cues:Vec<_>=clip.cues.iter().map(|c|{
        let block=if c.lane.ends_with("Trigger"){c.trigger(c.end()-1,personal.cap()).ok()}else{None};
        serde_json::json!({"lane":c.lane,"kind":c.kind,"requestedStartMs":c.start_ms,"requestedDurationMs":c.duration_ms,
            "startMs":c.start(),"durationMs":c.end()-c.start(),"packet":block,"levels":block.map(|b|levels(&b)),"strength":c.strength})
    }).collect();
    Ok(
        serde_json::json!({"durationMs":clip.duration_ms(),"quantumMs":QUANTUM_MS,"cues":cues,"triggerLimit":personal.cap()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn fixture() -> EffectClip {
        serde_json::from_value(serde_json::json!({"id":"test","name":"Test clip","tags":["test"],"author":"Local","license":"CC0-1.0","cues":[
            {"lane":"rightTrigger","kind":"load","startMs":3,"durationMs":97,"strength":75.0,"fromStrength":0.0},
            {"lane":"rightTrigger","kind":"release","startMs":100,"durationMs":100,"strength":0.0},
            {"lane":"leftGrip","kind":"pulse","startMs":0,"durationMs":100,"strength":50.0}
        ]})).unwrap()
    }
    #[test]
    fn rounded_schedule_release_gaps_and_unowned_lanes() {
        let clip = fixture();
        clip.validate().unwrap();
        assert_eq!((clip.cues[0].start(), clip.cues[0].end()), (10, 100));
        assert_eq!(clip.triggers(0, 8)[0], None);
        assert_eq!(clip.triggers(0, 8)[1].unwrap()[0], 5);
        assert!(levels(&clip.triggers(99, 8)[1].unwrap()).contains(&6));
        assert!(levels(&clip.triggers(99, 4)[1].unwrap()).contains(&4));
        assert_eq!(clip.triggers(100, 8)[1].unwrap()[0], 5);
        let preview = inspect_clip(&clip, &PersonalSettings::default()).unwrap();
        assert_eq!(preview["cues"][0]["startMs"], 10);
        assert_eq!(preview["cues"][0]["durationMs"], 90);
    }
    #[test]
    fn rejects_overlaps_after_rounding_overflow_invalid_sources_and_duplicates() {
        let mut clip = fixture();
        clip.cues[1].start_ms = 98;
        assert!(clip.validate().is_ok()); // rounded end/start both 100
        clip.cues[1].start_ms = 89;
        assert!(clip.validate().is_err());
        let mut clip = fixture();
        clip.cues[0].start_ms = u32::MAX;
        assert!(clip.validate().is_err());
        let mut clip = fixture();
        clip.source_url = "javascript:alert(1)".into();
        assert!(clip.validate().is_err());
        assert!(validate_library(&[fixture(), fixture()]).is_err());
    }
    #[test]
    fn force_ceiling_preserves_masks_and_vibration_frequency() {
        for mode in ["resistance", "vibration", "weapon", "bow"] {
            let setting = TriggerSetting {
                mode: mode.into(),
                strength: 100.0,
                curve: vec![1.0; 11],
                force_cap: Some(8),
                start_zone: Some(2),
                end_zone: Some(7),
                frequency_hz: Some(123),
                snap_strength: Some(100.0),
                ..Default::default()
            };
            let original = encode_trigger(&setting).unwrap();
            let limited = limit_trigger(original, 3);
            assert_eq!(&original[..3], &limited[..3]);
            assert!(levels(&limited).iter().all(|level| *level <= 3));
            if mode == "vibration" {
                assert_eq!(limited[9], 123);
            }
        }
    }
    #[test]
    fn stereo_cues_keep_their_lane_and_personal_settings_do_not_invent_signal() {
        let clip = fixture();
        let personal = PersonalSettings::default();
        assert_eq!(clip.sample(0.025, &personal)[1], 0.0);
        assert_eq!(clip.sample(0.3, &personal), [0.0; 2]);
        let calibrated = PersonalSettings {
            enabled: true,
            minimum: [20, 10],
            maximum: [70, 60],
            balance: 25,
            trigger_limit: 4,
        };
        calibrated.validate().unwrap();
        assert_eq!(calibrated.authored_strength(0.0, 0), 0.0);
        assert_eq!(calibrated.authored_strength(1.0, 0), 1.0);
        assert!((calibrated.side_limit(0) - 0.525).abs() < 0.0001);
        let invalid = PersonalSettings {
            minimum: [100, 0],
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }
}
