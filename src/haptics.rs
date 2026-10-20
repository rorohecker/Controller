//! Time-sliced rumble envelopes derived from the Haptic Lab waveform settings.

use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct HapticClip {
    pub shape: String,
    pub frequency: u16,
    pub attack_ms: u16,
    pub release_ms: u16,
    pub strength: u8,
    pub pan: i16,
    started: Instant,
    pub next_tick: Instant,
}

impl HapticClip {
    pub fn new(
        shape: String,
        frequency: u16,
        attack_ms: u16,
        release_ms: u16,
        strength: u8,
        pan: i16,
    ) -> Self {
        let now = Instant::now();
        Self {
            shape,
            frequency: frequency.clamp(40, 300),
            attack_ms: attack_ms.clamp(2, 200),
            release_ms: release_ms.clamp(25, 650),
            strength: strength.min(100),
            pan: pan.clamp(-100, 100),
            started: now,
            next_tick: now,
        }
    }

    pub fn duration(&self) -> Duration {
        Duration::from_millis(u64::from(self.attack_ms) + u64::from(self.release_ms))
    }

    pub fn finished(&self) -> bool {
        self.started.elapsed() >= self.duration()
    }

    /// Returns left/right motor intensities for the current envelope sample, or None when done.
    pub fn sample_motors(&mut self) -> Option<[u8; 2]> {
        if self.finished() {
            return None;
        }
        let elapsed = self.started.elapsed().as_secs_f32();
        let attack = self.attack_ms as f32 / 1000.0;
        let total = self.duration().as_secs_f32().max(0.001);
        let env = if elapsed < attack {
            (elapsed / attack.max(0.001)).clamp(0.0, 1.0)
        } else {
            (1.0 - (elapsed - attack) / (total - attack).max(0.001)).clamp(0.0, 1.0)
        };
        let phase = elapsed * self.frequency as f32;
        let wave = match self.shape.as_str() {
            "square" => {
                if (phase % 1.0) < 0.5 {
                    1.0
                } else {
                    0.15
                }
            }
            "saw" => (phase % 1.0) * 0.85 + 0.15,
            "noise" => {
                let n = ((phase * 12.9898).sin() * 43_758.547).fract().abs();
                0.2 + n * 0.8
            }
            _ => ((phase * std::f32::consts::TAU).sin() * 0.5 + 0.5).clamp(0.15, 1.0),
        };
        let amp = env * (self.strength as f32 / 100.0) * wave;
        let left = (amp
            * if self.pan > 0 {
                1.0 - self.pan as f32 / 150.0
            } else {
                1.0
            }
            * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let right = (amp
            * if self.pan < 0 {
                1.0 + self.pan as f32 / 150.0
            } else {
                1.0
            }
            * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        // Drive motors at ~frequency-bounded update rate without flooding HID.
        let period_ms = (1000u64 / (self.frequency as u64 / 4).max(8)).clamp(12, 40);
        self.next_tick = Instant::now() + Duration::from_millis(period_ms);
        Some([left, right])
    }

    pub fn due(&self) -> bool {
        Instant::now() >= self.next_tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_produces_nonzero_then_ends() {
        let mut clip = HapticClip::new("sine".into(), 90, 12, 80, 60, 0);
        let sample = clip.sample_motors().unwrap();
        assert!(sample[0] > 0 || sample[1] > 0);
        clip.started = Instant::now() - Duration::from_millis(200);
        assert!(clip.sample_motors().is_none());
    }
}
