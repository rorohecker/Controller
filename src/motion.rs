//! Read-only IMU calibration and a six-axis gravity filter. Packet offsets and
//! scales were checked against SDL's PS5 driver and Sony's Linux driver.
use aftertouch_protocol::InputState;
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct FactoryCalibration {
    pub valid: bool,
    pub message: String,
    gyro_bias: [f32; 3],
    gyro_scale: [f32; 3],
    accel_bias: [f32; 3],
    accel_scale: [f32; 3],
}
impl Default for FactoryCalibration {
    fn default() -> Self {
        Self {
            valid: false,
            message: "Factory data unavailable; nominal 16 counts/degree/s and 8192 counts/g"
                .into(),
            gyro_bias: [0.0; 3],
            gyro_scale: [1.0 / 16.0; 3],
            accel_bias: [0.0; 3],
            accel_scale: [1.0 / 8192.0; 3],
        }
    }
}
impl FactoryCalibration {
    pub fn fallback(message: &str) -> Self {
        Self {
            message: format!("{message}; using nominal scales"),
            ..Default::default()
        }
    }
    pub fn parse(data: &[u8], bluetooth: bool) -> Result<Self, &'static str> {
        if data.len() < 35 || data[0] != 5 {
            return Err("Short factory calibration report");
        }
        if bluetooth
            && (data.len() != 41
                || aftertouch_protocol::crc32_seeded(0xa3, &data[..37])
                    != u32::from_le_bytes(data[37..41].try_into().unwrap()))
        {
            return Err("Factory calibration CRC failed");
        }
        let word = |offset| f32::from(i16::from_le_bytes([data[offset], data[offset + 1]]));
        let speed = word(19) + word(21);
        let mut calibration = Self {
            valid: true,
            message: "Factory gyro and accelerometer calibration loaded".into(),
            ..Default::default()
        };
        for axis in 0..3 {
            let bias = word(1 + axis * 2);
            let gyro_span = word(7 + axis * 4) - word(9 + axis * 4);
            let plus = word(23 + axis * 4);
            let minus = word(25 + axis * 4);
            let accel_span = plus - minus;
            let gyro_scale = speed / gyro_span;
            let accel_scale = 2.0 / accel_span;
            if !speed.is_finite()
                || speed <= 0.0
                || bias.abs() > 1024.0
                || !gyro_scale.is_finite()
                || !(1.0 / 32.0..=3.0 / 32.0).contains(&gyro_scale)
                || !accel_scale.is_finite()
                || !(0.5 / 8192.0..=1.5 / 8192.0).contains(&accel_scale)
                || ((plus + minus) * 0.5).abs() > 1024.0
            {
                return Err("Factory calibration values are invalid");
            }
            calibration.gyro_bias[axis] = bias;
            calibration.gyro_scale[axis] = gyro_scale;
            calibration.accel_bias[axis] = (plus + minus) * 0.5;
            calibration.accel_scale[axis] = accel_scale;
        }
        Ok(calibration)
    }
    fn convert(&self, input: &InputState) -> ([f32; 3], [f32; 3]) {
        (
            std::array::from_fn(|i| {
                (f32::from(input.gyro[i]) - self.gyro_bias[i]) * self.gyro_scale[i]
            }),
            std::array::from_fn(|i| {
                (f32::from(input.accel[i]) - self.accel_bias[i]) * self.accel_scale[i]
            }),
        )
    }
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn norm(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn normalize(a: [f32; 3]) -> [f32; 3] {
    let length = norm(a);
    if length > 0.001 {
        a.map(|v| v / length)
    } else {
        [0.0, 0.0, 1.0]
    }
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionReading {
    pub valid: bool,
    pub factory: bool,
    pub factory_message: &'static str,
    pub gyro_dps: [f32; 3],
    pub accel_g: [f32; 3],
    pub gravity: [f32; 3],
    pub pitch_deg: f32,
    pub roll_deg: f32,
    pub yaw_deg: f32,
    pub dt: f32,
    pub clock: &'static str,
    pub still: bool,
    pub calibration: &'static str,
    pub progress: f32,
    pub calibration_complete: bool,
    pub bias_dps: [f32; 3],
}
#[derive(Default)]
struct StillCapture {
    count: u32,
    seconds: f32,
    sum: [f32; 3],
    sum_sq: [f32; 3],
    accel: Option<[f32; 3]>,
}
pub struct MotionProcessor {
    pub factory: FactoryCalibration,
    pub bias: [f32; 3],
    tick: Option<u32>,
    host: Option<Instant>,
    gravity: [f32; 3],
    initialized: bool,
    yaw: f32,
    capture: Option<StillCapture>,
    calibrated: bool,
}
impl Default for MotionProcessor {
    fn default() -> Self {
        Self {
            factory: FactoryCalibration::default(),
            bias: [0.0; 3],
            tick: None,
            host: None,
            gravity: [0.0, 0.0, 1.0],
            initialized: false,
            yaw: 0.0,
            capture: None,
            calibrated: false,
        }
    }
}
impl MotionProcessor {
    pub fn with_factory(factory: FactoryCalibration) -> Self {
        Self {
            factory,
            ..Default::default()
        }
    }
    pub fn set_bias(&mut self, bias: [f32; 3]) -> Result<(), &'static str> {
        if bias.iter().any(|v| !v.is_finite() || v.abs() > 3.0) {
            return Err("Still bias must be within ±3 degrees/second");
        }
        self.bias = bias;
        self.capture = None;
        self.calibrated = true;
        Ok(())
    }
    pub fn calibrate(&mut self, enabled: bool) {
        self.capture = enabled.then(StillCapture::default);
    }
    pub fn reset(&mut self) {
        self.tick = None;
        self.host = None;
        self.initialized = false;
        self.yaw = 0.0;
        self.capture = None;
    }
    pub fn update(&mut self, input: &InputState, now: Instant) -> MotionReading {
        let host_dt = self.host.replace(now).map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f32()
        });
        let (dt, clock) = if !input.full_report {
            self.tick = None;
            (0.0, "unavailable")
        } else if let Some(tick) = input.sensor_timestamp {
            let delta = self.tick.replace(tick).map(|old| tick.wrapping_sub(old));
            match delta {
                Some(0) => (0.0, "duplicate"),
                Some(delta) if delta <= 150_000 => (delta as f32 / 3_000_000.0, "device"),
                _ => (0.0, "resync"),
            }
        } else {
            (host_dt.min(0.05), "host fallback")
        };
        let mut completed = false;
        let (raw_gyro, accel) = self.factory.convert(input);
        let length = norm(accel);
        let stationary = norm(raw_gyro) < 3.0 && (0.9..=1.1).contains(&length);
        if let Some(capture) = self.capture.as_mut() {
            if !stationary
                || clock == "resync"
                || capture
                    .accel
                    .is_some_and(|old| norm(std::array::from_fn(|i| old[i] - accel[i])) > 0.03)
            {
                *capture = StillCapture::default();
            } else if dt > 0.0 {
                capture.accel.get_or_insert(accel);
                capture.count += 1;
                capture.seconds += dt;
                for (i, value) in raw_gyro.into_iter().enumerate() {
                    capture.sum[i] += value;
                    capture.sum_sq[i] += value * value;
                }
                if capture.seconds >= 2.0 && capture.count >= 50 {
                    let mean = capture.sum.map(|v| v / capture.count as f32);
                    let quiet = (0..3).all(|i| {
                        capture.sum_sq[i] / capture.count as f32 - mean[i] * mean[i] < 0.02
                    });
                    if quiet {
                        self.bias = mean;
                        self.calibrated = true;
                        self.capture = None;
                        completed = true;
                    } else {
                        self.capture = Some(StillCapture::default());
                    }
                }
            }
        }
        let gyro = std::array::from_fn(|i| raw_gyro[i] - self.bias[i]);
        if input.full_report {
            if (!self.initialized || clock == "resync") && (0.8..=1.2).contains(&length) {
                self.gravity = normalize(accel);
                self.initialized = true;
            }
            if dt > 0.0 {
                let change = cross(self.gravity, gyro.map(f32::to_radians));
                self.gravity = normalize(std::array::from_fn(|i| self.gravity[i] + change[i] * dt));
                if (0.8..=1.2).contains(&length) {
                    let target = normalize(accel);
                    let blend = 1.0 - (-dt / 0.8).exp();
                    self.gravity = normalize(std::array::from_fn(|i| {
                        self.gravity[i] + (target[i] - self.gravity[i]) * blend
                    }));
                }
                self.yaw =
                    (self.yaw + dot(gyro, self.gravity) * dt + 180.0).rem_euclid(360.0) - 180.0;
            }
        }
        MotionReading {
            valid: input.full_report,
            factory: self.factory.valid,
            factory_message: if self.factory.valid {
                "Factory calibration"
            } else {
                "Nominal fallback"
            },
            gyro_dps: gyro,
            accel_g: accel,
            gravity: self.gravity,
            pitch_deg: self.gravity[1].atan2(self.gravity[2]).to_degrees(),
            roll_deg: (-self.gravity[0]).clamp(-1.0, 1.0).asin().to_degrees(),
            yaw_deg: self.yaw,
            dt,
            clock,
            still: stationary,
            calibration: if self.capture.is_some() {
                "measuring"
            } else if self.calibrated {
                "saved"
            } else {
                "idle"
            },
            calibration_complete: completed,
            progress: self
                .capture
                .as_ref()
                .map_or(0.0, |c| (c.seconds / 2.0).min(1.0)),
            bias_dps: self.bias,
        }
    }
}

pub fn aim_axes(reading: &MotionReading, world: bool) -> [f32; 2] {
    if world {
        let yaw = dot(reading.gyro_dps, reading.gravity);
        let local_right = [1.0, 0.0, 0.0];
        let along = dot(local_right, reading.gravity);
        let right = normalize(std::array::from_fn(|i| {
            local_right[i] - along * reading.gravity[i]
        }));
        [yaw, dot(reading.gyro_dps, right)]
    } else {
        [reading.gyro_dps[2], reading.gyro_dps[0]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> InputState {
        let mut input = aftertouch_protocol::parse_input(&[1; 64]).unwrap();
        input.buttons = [0.0; 23];
        input.axes = [0.0; 4];
        input.gyro = [0; 3];
        input.accel = [0, 0, 8192];
        input.sensor_timestamp = Some(0);
        input
    }
    fn factory() -> [u8; 41] {
        let mut data = [0u8; 41];
        data[0] = 5;
        for (offset, value) in [
            (1, 16i16),
            (3, -16),
            (5, 0),
            (7, 8656),
            (9, -8624),
            (11, 8624),
            (13, -8656),
            (15, 8640),
            (17, -8640),
            (19, 540),
            (21, 540),
            (23, 8242),
            (25, -8142),
            (27, 8192),
            (29, -8192),
            (31, 8192),
            (33, -8192),
        ] {
            data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        data
    }
    #[test]
    fn factory_data_converts_raw_counts_and_rejects_bad_crc_or_ranges() {
        let data = factory();
        let calibration = FactoryCalibration::parse(&data, false).unwrap();
        let mut sample = input();
        sample.gyro = [176, -336, 1440];
        sample.accel = [50, 0, 8192];
        let (gyro, accel) = calibration.convert(&sample);
        assert_eq!(gyro, [10.0, -20.0, 90.0]);
        assert_eq!(accel, [0.0, 0.0, 1.0]);
        let mut bluetooth = data;
        let checksum = aftertouch_protocol::crc32_seeded(0xa3, &bluetooth[..37]);
        bluetooth[37..].copy_from_slice(&checksum.to_le_bytes());
        assert!(FactoryCalibration::parse(&bluetooth, true).is_ok());
        bluetooth[7] ^= 1;
        assert!(FactoryCalibration::parse(&bluetooth, true).is_err());
        let mut bad = data;
        bad[7..11].fill(0);
        assert!(FactoryCalibration::parse(&bad, false).is_err());
        assert!(FactoryCalibration::parse(&data[..20], false).is_err());
        let fallback = FactoryCalibration::default();
        assert_eq!(fallback.convert(&sample).0[2], 90.0);
    }
    #[test]
    fn device_clock_handles_wrap_duplicates_and_long_gaps() {
        let now = Instant::now();
        let mut processor = MotionProcessor::default();
        let mut sample = input();
        sample.sensor_timestamp = Some(u32::MAX - 1499);
        assert_eq!(processor.update(&sample, now).dt, 0.0);
        sample.sensor_timestamp = Some(1500);
        assert!((processor.update(&sample, now).dt - 0.001).abs() < 1e-7);
        assert_eq!(processor.update(&sample, now).clock, "duplicate");
        sample.sensor_timestamp = Some(900000);
        assert_eq!(processor.update(&sample, now).clock, "resync");
        sample.sensor_timestamp = Some(903000);
        assert!((processor.update(&sample, now).dt - 0.001).abs() < 1e-7);
    }
    #[test]
    fn still_capture_requires_consecutive_samples_and_cancel_keeps_bias() {
        let mut processor = MotionProcessor::default();
        processor.set_bias([0.1, 0.2, 0.3]).unwrap();
        processor.calibrate(true);
        let mut sample = input();
        for i in 0..1500 {
            sample.sensor_timestamp = Some(i * 3000);
            processor.update(&sample, Instant::now());
        }
        sample.sensor_timestamp = Some(15_000_000);
        assert_eq!(processor.update(&sample, Instant::now()).progress, 0.0);
        for i in 1..1500 {
            sample.sensor_timestamp = Some(15_000_000 + i * 3000);
            assert!(
                !processor
                    .update(&sample, Instant::now())
                    .calibration_complete
            );
        }
        processor.calibrate(false);
        assert_eq!(processor.bias, [0.1, 0.2, 0.3]);
    }
    #[test]
    fn integrated_rotation_is_independent_of_usb_or_bluetooth_sample_rate() {
        let run = |step| {
            let mut processor = MotionProcessor::default();
            let mut sample = input();
            sample.gyro[2] = 1440;
            let mut result = None;
            for tick in (0..=3_000_000u32).step_by(step) {
                sample.sensor_timestamp = Some(tick);
                result = Some(processor.update(&sample, Instant::now()));
            }
            result.unwrap().yaw_deg
        };
        assert!((run(3000) - 90.0).abs() < 0.02);
        assert!((run(12000) - 90.0).abs() < 0.02);
    }
    #[test]
    fn gravity_projection_tracks_tilt_and_still_capture_restarts_on_motion() {
        let mut processor = MotionProcessor::default();
        let mut sample = input();
        sample.accel = [0, 5793, 5793];
        let mut reading = processor.update(&sample, Instant::now());
        assert!((reading.pitch_deg - 45.0).abs() < 0.02);
        reading.gyro_dps = [30.0, 63.63961, 63.63961];
        assert!((aim_axes(&reading, true)[0] - 90.0).abs() < 0.02);
        assert!((aim_axes(&reading, true)[1] - 30.0).abs() < 0.02);
        processor.calibrate(true);
        sample.accel = [0, 0, 8192];
        sample.gyro = [2, -3, 1];
        for i in 1..=1500 {
            sample.sensor_timestamp = Some(i * 3000);
            processor.update(&sample, Instant::now());
        }
        sample.gyro = [1000, 0, 0];
        sample.sensor_timestamp = Some(1501 * 3000);
        assert_eq!(processor.update(&sample, Instant::now()).progress, 0.0);
        sample.gyro = [2, -3, 1];
        let mut complete = 0;
        for i in 1502..=3700 {
            sample.sensor_timestamp = Some(i * 3000);
            if processor
                .update(&sample, Instant::now())
                .calibration_complete
            {
                complete += 1;
            }
        }
        assert_eq!(complete, 1);
        assert_eq!(processor.bias, [0.125, -0.1875, 0.0625]);
        assert!(processor.set_bias([f32::NAN, 0.0, 0.0]).is_err());
    }
}
