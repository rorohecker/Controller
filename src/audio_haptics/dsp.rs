//! Pure, allocation-free DSP helpers shared by the bridge, file conversion and
//! authored clips. Nothing here locks, allocates or reads the clock, so every
//! function is safe to call from the audio callback.
use std::f32::consts::TAU;

/// Deterministic hash noise in [-1, 1] (xorshift32 with a multiplicative finalizer).
pub fn hash_unit(seed: u32, index: u32) -> f32 {
    let mut x = (seed ^ index.wrapping_mul(0x9E37_79B9)).wrapping_add(0x6D2B_79F5);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Seeded noise low-passed into the tactile band. It is a pure function of
/// `seconds`, so previews, replays and the timeline render identical samples.
/// Random lattice points at `2 * cutoff_hz` are joined with a smoothstep, which
/// removes content far above `cutoff_hz`.
pub fn band_noise(seconds: f32, cutoff_hz: f32, seed: u32) -> f32 {
    // clamp(NaN) is NaN; a non-finite cutoff falls back to a mid tactile value.
    let cutoff_hz = if cutoff_hz.is_finite() {
        cutoff_hz.clamp(1.0, 400.0)
    } else {
        90.0
    };
    let position = seconds.max(0.0) * cutoff_hz * 2.0;
    let cell = position.floor();
    let fraction = position - cell;
    let index = cell as u32;
    let a = hash_unit(seed, index);
    let b = hash_unit(seed, index.wrapping_add(1));
    let smooth = fraction * fraction * (3.0 - 2.0 * fraction);
    ((a + (b - a) * smooth) * 1.25).clamp(-1.0, 1.0)
}

/// Seed for a cue so each cue in a clip has its own, repeatable noise.
pub fn cue_seed(index: usize, start_ms: u32) -> u32 {
    (index as u32)
        .wrapping_mul(0x27D4_EB2F)
        .wrapping_add(start_ms.wrapping_mul(0x1656_67B1))
        ^ 0xA5A5_1234
}

/// Shapes whose character comes from timing and envelope rather than harmonic
/// content, so they still feel different after the actuators low-pass the drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Texture {
    Triangle,
    /// Train of short, sharp bursts: a buzzy ratchet.
    Pulse,
    /// Two-hit "lub-dub" pattern.
    Double,
    /// Sine under a smooth, slow raised-cosine envelope.
    Swell,
}

/// Length of one decaying hit in the pulse train and double patterns.
const PULSE_WIDTH: f32 = 0.012;
/// Delay of the second hit of the double pattern. The pattern plays once from the start of
/// the clip (under its envelope); it does not repeat, so long clips never hold a second one.
pub const DOUBLE_GAP: f32 = 0.17;

/// Raised-cosine gain for a linear 0..1 ramp. The swell uses this both as the clip
/// envelope (`VoiceClip`) and for timeline cues, so previews match the timeline.
pub fn swell_gain(ramp: f32) -> f32 {
    0.5 - 0.5 * (ramp.clamp(0.0, 1.0) * std::f32::consts::PI).cos()
}

impl Texture {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "triangle" => Some(Self::Triangle),
            "pulse" => Some(Self::Pulse),
            "double" => Some(Self::Double),
            "swell" => Some(Self::Swell),
            _ => None,
        }
    }

    /// Pulses per second for the ratchet: a fraction of the carrier frequency.
    pub fn pulse_rate(frequency: f32) -> f32 {
        (frequency / 6.0).clamp(8.0, 40.0)
    }

    /// One damped hit starting at `t` seconds (0 before it starts).
    fn hit(t: f32, frequency: f32) -> f32 {
        if t < 0.0 {
            return 0.0;
        }
        (t / 0.004).min(1.0) * (-t / 0.07).exp() * (t * frequency * TAU).sin()
    }

    /// Raw wave in [-1, 1]. `total` is the clip length (only the swell reads it).
    /// Pure in its inputs and finite for any finite input.
    pub fn sample(self, elapsed: f32, total: f32, frequency: f32) -> f32 {
        let elapsed = elapsed.max(0.0);
        let frequency = if frequency.is_finite() {
            frequency.clamp(1.0, 400.0)
        } else {
            90.0
        };
        match self {
            Self::Triangle => 4.0 * ((elapsed * frequency).fract() - 0.5).abs() - 1.0,
            Self::Pulse => {
                let period = 1.0 / Self::pulse_rate(frequency);
                let local = elapsed % period;
                let gate = (1.0 - local / PULSE_WIDTH).max(0.0);
                (local * frequency * TAU).cos() * gate
            }
            Self::Double => (Self::hit(elapsed, frequency) * 1.25
                + Self::hit(elapsed - DOUBLE_GAP, frequency) * 1.0)
                .clamp(-1.0, 1.0),
            Self::Swell => {
                let x = if total > 0.0 {
                    (elapsed / total).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                // Same raised cosine as the `VoiceClip` envelope (a symmetric rise and fall).
                let bell = swell_gain(1.0 - (2.0 * x - 1.0).abs());
                (elapsed * frequency * TAU).sin() * bell
            }
        }
    }
}

/// Thermal duty decays continuously (see `soft_limit`). While a stream is closed
/// no samples run, so elapsed wall time is applied in one step.
pub fn decay_duty(duty: f32, seconds: f32) -> f32 {
    if !duty.is_finite() || duty <= 0.0 {
        return 0.0;
    }
    // 0.99998 per sample at 48 kHz is exp(-0.96) per second.
    let decayed = duty * (-0.96 * seconds.clamp(0.0, 3600.0)).exp();
    // Flush denormal-range leftovers so later per-sample math stays fast.
    if decayed < 1e-20 { 0.0 } else { decayed }
}

/// How a file or the bridge is turned into grip vibration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvMode {
    Music,
    Bass,
    Impact,
    Texture,
    /// Sub / punch / texture split with per-band gain.
    Bands,
    /// Full music conversion plus onset-triggered impact pulses.
    Beats,
}
impl ConvMode {
    pub fn parse(kind: &str) -> Self {
        match kind {
            "bass" => Self::Bass,
            "impact" => Self::Impact,
            "texture" => Self::Texture,
            "bands" => Self::Bands,
            "beats" => Self::Beats,
            _ => Self::Music,
        }
    }
}

/// Three-band tactile split: sub (< 70 Hz), punch (70-250 Hz) and texture
/// (> 250 Hz, re-synthesised as an envelope on a 120 Hz carrier).
#[derive(Clone, Default)]
pub struct ThreeBand {
    sub: f32,
    mid: f32,
    env: f32,
    phase: f32,
}
impl ThreeBand {
    pub fn frame(&mut self, x: f32, rate: f32, gains: [f32; 3]) -> f32 {
        let x = if x.is_finite() {
            x.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let rate = rate.max(8_000.0);
        self.sub += (TAU * 70.0 / rate).min(0.5) * (x - self.sub);
        self.mid += (TAU * 250.0 / rate).min(0.5) * (x - self.mid);
        let punch = self.mid - self.sub;
        let texture = x - self.mid;
        let target = texture.abs();
        let alpha = if target > self.env { 0.05 } else { 0.002 };
        self.env += alpha * (target - self.env);
        self.phase = (self.phase + TAU * 120.0 / rate) % TAU;
        let sub = self.sub * gains[0] * 3.0;
        let punch = punch * gains[1] * 2.5;
        let texture = self.phase.sin() * self.env * gains[2] * 2.0;
        // Denormals slow the per-sample filters after silence; flush them.
        for state in [&mut self.sub, &mut self.mid, &mut self.env] {
            if state.abs() < 1e-20 {
                *state = 0.0;
            }
        }
        (sub + punch + texture).clamp(-1.0, 1.0)
    }
}

/// Spectral-flux onset detector over a three-band filter bank. Every 5 ms hop it
/// sums the positive change of each band's RMS and compares that to a running
/// average. An onset starts a short 90 Hz impact pulse; `push` returns the pulse.
#[derive(Clone)]
pub struct OnsetDetector {
    rate: f32,
    low: f32,
    mid: f32,
    acc: [f32; 3],
    prev: [f32; 3],
    count: u32,
    hop: u32,
    average: f32,
    cooldown: u32,
    pulse_left: u32,
    pulse_len: u32,
    pulse_amp: f32,
    pulse_phase: f32,
}
impl OnsetDetector {
    pub fn new(rate: u32) -> Self {
        let rate = rate.clamp(8_000, 192_000);
        Self {
            rate: rate as f32,
            low: 0.0,
            mid: 0.0,
            acc: [0.0; 3],
            prev: [0.0; 3],
            count: 0,
            hop: (rate / 200).max(1),
            average: 0.0,
            cooldown: 0,
            pulse_left: 0,
            pulse_len: (rate as f32 * 0.04) as u32,
            pulse_amp: 0.0,
            pulse_phase: 0.0,
        }
    }
    pub fn push(&mut self, x: f32) -> f32 {
        let x = if x.is_finite() {
            x.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.low += (TAU * 150.0 / self.rate).min(0.5) * (x - self.low);
        self.mid += (TAU * 1500.0 / self.rate).min(0.5) * (x - self.mid);
        let bands = [self.low, self.mid - self.low, x - self.mid];
        for (acc, band) in self.acc.iter_mut().zip(bands) {
            *acc += band * band;
        }
        self.count += 1;
        if self.count >= self.hop {
            let mut flux = 0.0;
            for band in 0..3 {
                let energy = (self.acc[band] / self.hop as f32).sqrt();
                flux += (energy - self.prev[band]).max(0.0);
                self.prev[band] = energy;
                self.acc[band] = 0.0;
            }
            self.count = 0;
            if self.cooldown > 0 {
                self.cooldown -= 1;
            }
            if flux > self.average * 1.8 + 0.03 && self.cooldown == 0 {
                self.pulse_left = self.pulse_len;
                self.pulse_amp = (flux * 3.0).clamp(0.3, 1.0);
                self.pulse_phase = 0.0;
                self.cooldown = 16; // 80 ms refractory period
            }
            self.average += 0.1 * (flux - self.average);
        }
        if self.pulse_left == 0 {
            return 0.0;
        }
        let progress = self.pulse_left as f32 / self.pulse_len.max(1) as f32;
        self.pulse_left -= 1;
        self.pulse_phase = (self.pulse_phase + TAU * 90.0 / self.rate) % TAU;
        self.pulse_phase.sin() * self.pulse_amp * progress * progress
    }
}

/// Emphasise left/right differences so the side a sound comes from rattles harder.
/// `amount` 0 returns the input unchanged.
pub fn pan_emphasis(frame: [f32; 2], amount: f32) -> [f32; 2] {
    if amount.is_nan() || amount <= 0.0 {
        return frame;
    }
    let width = 1.0 + amount.min(4.0);
    let mid = (frame[0] + frame[1]) * 0.5;
    let side = (frame[0] - frame[1]) * 0.5 * width;
    [(mid + side).clamp(-1.0, 1.0), (mid - side).clamp(-1.0, 1.0)]
}

/// Linear interpolation position of an output sample inside the latest input
/// interval. `accumulator` is the resampler phase before subtracting `rate`.
pub fn interpolation_fraction(accumulator: u64, rate: u32, base: u32) -> f32 {
    let behind = accumulator.saturating_sub(u64::from(rate)) as f32 / base as f32;
    (1.0 - behind).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn energy(samples: impl Iterator<Item = f32>) -> f32 {
        samples.map(|x| x * x).sum()
    }
    #[test]
    fn band_noise_is_deterministic_bounded_and_low_passed() {
        let a: Vec<f32> = (0..4800)
            .map(|i| band_noise(i as f32 / 48_000.0, 90.0, 7))
            .collect();
        let b: Vec<f32> = (0..4800)
            .map(|i| band_noise(i as f32 / 48_000.0, 90.0, 7))
            .collect();
        let other: Vec<f32> = (0..4800)
            .map(|i| band_noise(i as f32 / 48_000.0, 90.0, 8))
            .collect();
        assert_eq!(a, b);
        assert_ne!(a, other);
        assert!(a.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        assert!(energy(a.iter().copied()) > 1.0, "noise must carry energy");
        // Smoothness: sample-to-sample steps are tiny compared with white noise.
        let step = a
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.1, "step {step}");
        assert!(a.iter().any(|x| *x > 0.1) && a.iter().any(|x| *x < -0.1));
    }
    #[test]
    fn band_noise_survives_non_finite_cutoff_and_flushes_tiny_duty() {
        assert!(band_noise(0.3, f32::NAN, 1).is_finite());
        assert!(band_noise(0.3, f32::INFINITY, 1).is_finite());
        assert_eq!(decay_duty(1e-19, 3600.0), 0.0);
        let mut band = ThreeBand::default();
        band.frame(0.5, 48_000.0, [1.0; 3]);
        for _ in 0..200_000 {
            band.frame(0.0, 48_000.0, [1.0; 3]);
        }
        assert!(band.sub == 0.0 || band.sub.abs() >= 1e-20);
    }
    #[test]
    fn duty_decays_with_elapsed_time() {
        assert_eq!(decay_duty(0.0, 5.0), 0.0);
        assert_eq!(decay_duty(f32::NAN, 5.0), 0.0);
        let d = decay_duty(0.2, 1.0);
        assert!((d - 0.2 * (-0.96f32).exp()).abs() < 1e-6);
        assert!(decay_duty(0.2, 10.0) < d);
        assert_eq!(decay_duty(0.2, 0.0), 0.2);
    }
    #[test]
    fn three_band_split_routes_each_band_to_its_gain() {
        let run = |hz: f32, gains: [f32; 3]| {
            let mut band = ThreeBand::default();
            energy((0..9600).map(|i| {
                band.frame(
                    (i as f32 / 48_000.0 * hz * TAU).sin() * 0.5,
                    48_000.0,
                    gains,
                )
            }))
        };
        assert!(run(40.0, [1.0, 0.0, 0.0]) > run(40.0, [0.0, 1.0, 1.0]) * 3.0);
        assert!(run(150.0, [0.0, 1.0, 0.0]) > 0.5);
        assert_eq!(run(150.0, [0.0, 0.0, 0.0]), 0.0);
        assert!(run(3000.0, [0.0, 0.0, 1.0]) > run(3000.0, [1.0, 1.0, 0.0]) * 5.0);
        assert_eq!(run(3000.0, [0.0; 3]), 0.0);
        assert!(
            ThreeBand::default()
                .frame(f32::NAN, 48_000.0, [1.0; 3])
                .is_finite()
        );
    }
    #[test]
    fn onsets_fire_on_attacks_but_not_steady_tones() {
        let mut detector = OnsetDetector::new(48_000);
        let tone = |i: usize| (i as f32 / 48_000.0 * 100.0 * TAU).sin() * 0.6;
        let silence: f32 = (0..24_000).map(|_| detector.push(0.0).abs()).sum();
        assert_eq!(silence, 0.0);
        let attack: f32 = (0..4_800).map(|i| detector.push(tone(i)).abs()).sum();
        assert!(attack > 1.0, "attack produced {attack}");
        for i in 0..48_000 {
            detector.push(tone(i));
        }
        let steady: f32 = (0..48_000).map(|i| detector.push(tone(i)).abs()).sum();
        assert_eq!(steady, 0.0, "a steady tone must not retrigger");
        for _ in 0..24_000 {
            detector.push(0.0);
        }
        let second: f32 = (0..4_800).map(|i| detector.push(tone(i)).abs()).sum();
        assert!(second > 1.0);
        assert!(detector.push(f32::NAN).is_finite());
    }
    #[test]
    fn pan_emphasis_widens_without_changing_mono() {
        assert_eq!(pan_emphasis([0.3, 0.1], 0.0), [0.3, 0.1]);
        assert_eq!(pan_emphasis([0.4, 0.4], 2.0), [0.4, 0.4]);
        let out = pan_emphasis([0.4, 0.2], 1.0);
        assert!(out[0] > 0.4 && out[1] < 0.2);
        assert!(
            pan_emphasis([1.0, -1.0], 4.0)
                .iter()
                .all(|x| x.abs() <= 1.0)
        );
    }
    #[test]
    fn interpolation_is_linear_between_input_samples() {
        assert_eq!(interpolation_fraction(48_000, 48_000, 48_000), 1.0);
        assert_eq!(interpolation_fraction(48_000 + 24_000, 48_000, 48_000), 0.5);
        assert_eq!(interpolation_fraction(10, 48_000, 48_000), 1.0);
        assert_eq!(ConvMode::parse("bands"), ConvMode::Bands);
        assert_eq!(ConvMode::parse("whatever"), ConvMode::Music);
    }
}
