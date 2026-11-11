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
        let wave = motor_wave(&self.shape, elapsed, total, self.frequency as f32);
        // Motor levels are percentages (0-100); the device layer scales them to report bytes.
        let amp = env * (self.strength as f32 / 100.0) * wave;
        let left =
            (amp * if self.pan > 0 {
                1.0 - self.pan as f32 / 150.0
            } else {
                1.0
            } * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8;
        let right =
            (amp * if self.pan < 0 {
                1.0 + self.pan as f32 / 150.0
            } else {
                1.0
            } * 100.0)
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

/// Motor-level approximation (0.15..1 or 0..1) of a shape. Motors cannot follow a
/// 90 Hz carrier, so each shape keeps its character at motor-friendly rates.
fn motor_wave(shape: &str, elapsed: f32, total: f32, frequency: f32) -> f32 {
    let phase = elapsed * frequency;
    match shape {
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
        // Smooth rise and fall at the carrier rate, without the sine's hard floor.
        "triangle" => 0.15 + 0.85 * (1.0 - (2.0 * (phase % 1.0) - 1.0).abs()),
        // Ratcheted on/off at the pulse rate: 40 % on, then fully off.
        "pulse" => {
            let rate = (frequency / 10.0).clamp(4.0, 16.0);
            if (elapsed * rate) % 1.0 < 0.4 {
                1.0
            } else {
                0.0
            }
        }
        // Two hits: now and again after the gap, each decaying; plays once.
        "double" => {
            let hit = |t: f32| {
                if t < 0.0 { 0.0 } else { (-t / 0.07).exp() }
            };
            (hit(elapsed) + hit(elapsed - crate::audio_haptics::DOUBLE_GAP) * 0.85).clamp(0.0, 1.0)
        }
        // Raised cosine over the whole clip (same gain as the voice-coil swell).
        "swell" => {
            let x = if total > 0.0 {
                (elapsed / total).clamp(0.0, 1.0)
            } else {
                0.0
            };
            crate::audio_haptics::swell_gain(1.0 - (2.0 * x - 1.0).abs())
        }
        _ => ((phase * std::f32::consts::TAU).sin() * 0.5 + 0.5).clamp(0.15, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_produces_nonzero_then_ends() {
        let mut clip = HapticClip::new("sine".into(), 90, 12, 80, 60, 0);
        clip.started = Instant::now() - Duration::from_millis(20);
        let sample = clip.sample_motors().unwrap();
        assert!(sample[0] > 0 || sample[1] > 0);
        clip.started = Instant::now() - Duration::from_millis(200);
        assert!(clip.sample_motors().is_none());
    }

    #[test]
    fn motor_samples_stay_within_percent_range() {
        let mut clip = HapticClip::new("square".into(), 90, 1, 200, 100, 0);
        clip.started = Instant::now() - Duration::from_millis(20);
        let [left, right] = clip.sample_motors().unwrap();
        assert!(left <= 100 && right <= 100);
    }

    #[test]
    fn fallback_shapes_are_distinct_and_bounded() {
        let sample = |shape: &str, t: f32| motor_wave(shape, t, 0.6, 90.0);
        let sine: Vec<f32> = (0..60).map(|i| sample("sine", i as f32 / 100.0)).collect();
        for shape in ["triangle", "pulse", "double", "swell"] {
            let wave: Vec<f32> = (0..60).map(|i| sample(shape, i as f32 / 100.0)).collect();
            assert!(wave.iter().all(|v| (0.0..=1.0).contains(v)), "{shape}");
            assert!(wave.iter().any(|v| *v > 0.5), "{shape} is silent");
            assert_ne!(wave, sine, "{shape} must not fall back to sine");
        }
        // Pulse is fully off between ratchets; the sine never is.
        assert!(sample("pulse", 0.07) == 0.0 && sample("pulse", 0.0) == 1.0);
        // Double: two separate hits, a trough in between and nothing repeating late.
        assert!(sample("double", 0.0) > 0.9 && sample("double", 0.17) > 0.8);
        assert!(sample("double", 0.1) < sample("double", 0.17));
        assert!(sample("double", 0.55) < 0.02);
        // Swell: raised cosine, zero at both ends, peak in the middle.
        assert!(sample("swell", 0.0) < 0.01 && sample("swell", 0.6) < 0.01);
        assert!(sample("swell", 0.3) > 0.99 && sample("swell", 0.15) < sample("swell", 0.3));
    }
}
