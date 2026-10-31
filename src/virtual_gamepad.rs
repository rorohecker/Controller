//! Portable gamepad math. Linux performs no device I/O until explicitly enabled.
use aftertouch_protocol::InputState;
use std::time::{Duration, Instant};

pub const BUTTONS: [usize; 11] = [0, 1, 2, 3, 4, 5, 8, 9, 16, 10, 11];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub keys: [i32; 11],
    pub axes: [i32; 8],
}
impl Report {
    pub fn from_input(input: &InputState) -> Self {
        fn axis(v: f32) -> i32 {
            if !v.is_finite() {
                0
            } else {
                (v.clamp(-1.0, 1.0) * if v < 0.0 { 32768.0 } else { 32767.0 }).round() as i32
            }
        }
        fn trigger(v: f32) -> i32 {
            if !v.is_finite() {
                0
            } else {
                (v.clamp(0.0, 1.0) * 255.0).round() as i32
            }
        }
        Self {
            keys: BUTTONS.map(|i| i32::from(input.buttons[i] > 0.5)),
            axes: [
                axis(input.axes[0]),
                axis(input.axes[1]),
                axis(input.axes[2]),
                axis(input.axes[3]),
                trigger(input.buttons[6]),
                trigger(input.buttons[7]),
                i32::from(input.buttons[15] > 0.5) - i32::from(input.buttons[14] > 0.5),
                i32::from(input.buttons[13] > 0.5) - i32::from(input.buttons[12] > 0.5),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Effect {
    pub levels: [u16; 2],
    pub length_ms: u16,
    pub delay_ms: u16,
}
#[derive(Debug, PartialEq, Eq)]
pub enum UploadError {
    Full,
    InvalidId,
}
#[derive(Default)]
pub struct RumbleBank {
    slots: [Option<Effect>; 16],
    active: [Option<(Instant, Instant)>; 16],
    gain: u16,
}
impl RumbleBank {
    pub fn new() -> Self {
        Self {
            gain: u16::MAX,
            ..Self::default()
        }
    }
    pub fn upload(&mut self, id: i16, effect: Effect) -> Result<usize, UploadError> {
        let index = if id == -1 {
            self.slots
                .iter()
                .position(Option::is_none)
                .ok_or(UploadError::Full)?
        } else {
            let i = usize::try_from(id).map_err(|_| UploadError::InvalidId)?;
            if i >= 16 {
                return Err(UploadError::InvalidId);
            }
            i
        };
        self.slots[index] = Some(effect);
        Ok(index)
    }
    pub fn erase(&mut self, id: usize) -> bool {
        if id >= 16 || self.slots[id].is_none() {
            return false;
        }
        self.slots[id] = None;
        self.active[id] = None;
        true
    }
    pub fn play(&mut self, id: usize, repeats: i32, now: Instant) {
        if id >= 16 {
            return;
        }
        if repeats <= 0 {
            self.active[id] = None;
            return;
        }
        if let Some(effect) = self.slots[id] {
            let delay = Duration::from_millis(u64::from(effect.delay_ms).min(10_000));
            let length = if effect.length_ms == 0 {
                10_000
            } else {
                u64::from(effect.length_ms)
                    .saturating_mul(repeats as u64)
                    .min(10_000)
            };
            self.active[id] = Some((now + delay, now + delay + Duration::from_millis(length)));
        }
    }
    pub fn set_gain(&mut self, gain: i32) {
        self.gain = gain.clamp(0, 65535) as u16;
    }
    pub fn sample(&mut self, now: Instant) -> [u8; 2] {
        let mut sum = [0u32; 2];
        for i in 0..16 {
            if let Some((start, end)) = self.active[i] {
                if now >= end {
                    self.active[i] = None;
                } else if now >= start
                    && let Some(effect) = self.slots[i]
                {
                    for (v, level) in sum.iter_mut().zip(effect.levels) {
                        *v = v.saturating_add(u32::from(level));
                    }
                }
            }
        }
        sum.map(|v| {
            ((u64::from(v.min(65535)) * u64::from(self.gain) * 255) / (65535u64 * 65535)) as u8
        })
    }
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use evdev::{
        AbsInfo, AbsoluteAxisCode as A, AttributeSet, BusType, EventSummary, EventType,
        FFEffectCode, FFEffectKind, InputEvent, InputId, KeyCode as K, UInputCode, UinputAbsSetup,
        uinput::VirtualDevice,
    };
    use std::os::fd::AsRawFd;
    const KEYS: [K; 11] = [
        K::BTN_SOUTH,
        K::BTN_EAST,
        // xpad's legacy BTN_X/BTN_Y aliases are NORTH/WEST, in that order.
        K::BTN_NORTH,
        K::BTN_WEST,
        K::BTN_TL,
        K::BTN_TR,
        K::BTN_SELECT,
        K::BTN_START,
        K::BTN_MODE,
        K::BTN_THUMBL,
        K::BTN_THUMBR,
    ];
    const AXES: [A; 8] = [
        A::ABS_X,
        A::ABS_Y,
        A::ABS_RX,
        A::ABS_RY,
        A::ABS_Z,
        A::ABS_RZ,
        A::ABS_HAT0X,
        A::ABS_HAT0Y,
    ];
    pub struct Pad {
        device: VirtualDevice,
        rumble: RumbleBank,
        last: [u8; 2],
        last_feedback: Instant,
    }
    impl Pad {
        pub fn open() -> Result<Self, String> {
            let result = (|| -> std::io::Result<Self> {
                let keys = AttributeSet::from_iter(KEYS);
                let mut builder = VirtualDevice::builder()?
                    .name("CONTROLLER Virtual Xbox Gamepad")
                    .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0110))
                    .with_keys(&keys)?
                    .with_ff(&AttributeSet::from_iter([
                        FFEffectCode::FF_RUMBLE,
                        FFEffectCode::FF_GAIN,
                    ]))?
                    .with_ff_effects_max(16);
                for (i, code) in AXES.iter().enumerate() {
                    let (min, max) = if i < 4 {
                        (-32768, 32767)
                    } else if i < 6 {
                        (0, 255)
                    } else {
                        (-1, 1)
                    };
                    builder = builder.with_absolute_axis(&UinputAbsSetup::new(
                        *code,
                        AbsInfo::new(0, min, max, 0, 0, 0),
                    ))?;
                }
                let device = builder.build()?;
                // SAFETY: this is an owned, live uinput descriptor. Only its file status flags change.
                unsafe {
                    let flags = libc::fcntl(device.as_raw_fd(), libc::F_GETFL);
                    if flags < 0
                        || libc::fcntl(device.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                            < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(Self {
                    device,
                    rumble: RumbleBank::new(),
                    last: [0; 2],
                    last_feedback: Instant::now(),
                })
            })();
            result.map_err(|e|format!("Virtual gamepad unavailable: {e}. Check /dev/uinput access and load the uinput module. The optional CONTROLLER udev rule grants access to the active desktop user."))
        }
        fn emit(&mut self, report: Report) -> Result<(), String> {
            let mut events = Vec::with_capacity(19);
            for (key, value) in KEYS.iter().zip(report.keys) {
                events.push(InputEvent::new(EventType::KEY.0, key.0, value));
            }
            for (axis, value) in AXES.iter().zip(report.axes) {
                events.push(InputEvent::new(EventType::ABSOLUTE.0, axis.0, value));
            }
            self.device
                .emit(&events)
                .map_err(|e| format!("Virtual gamepad write failed: {e}"))
        }
        pub fn update(&mut self, input: &InputState) -> Result<(), String> {
            self.emit(Report::from_input(input))
        }
        pub fn neutralize(&mut self) -> Result<(), String> {
            self.emit(Report::default())
        }
        pub fn feedback(&mut self) -> Result<Option<[u8; 2]>, String> {
            let events: Vec<_> = match self.device.fetch_events() {
                Ok(events) => events.collect(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Vec::new(),
                Err(e) => return Err(e.to_string()),
            };
            for event in events {
                match event.destructure() {
                    EventSummary::UInput(e, UInputCode::UI_FF_UPLOAD, ..) => {
                        let mut request = self
                            .device
                            .process_ff_upload(e)
                            .map_err(|e| e.to_string())?;
                        let data = request.effect();
                        if let FFEffectKind::Rumble {
                            strong_magnitude,
                            weak_magnitude,
                        } = data.kind
                        {
                            match self.rumble.upload(
                                request.effect_id(),
                                Effect {
                                    levels: [strong_magnitude, weak_magnitude],
                                    length_ms: data.replay.length,
                                    delay_ms: data.replay.delay,
                                },
                            ) {
                                Ok(id) => {
                                    request.set_effect_id(id as i16);
                                    request.set_retval(0);
                                }
                                Err(UploadError::Full) => request.set_retval(-libc::ENOSPC),
                                Err(UploadError::InvalidId) => request.set_retval(-libc::EINVAL),
                            }
                        } else {
                            request.set_retval(-libc::EINVAL);
                        }
                    }
                    EventSummary::UInput(e, UInputCode::UI_FF_ERASE, ..) => {
                        let mut request =
                            self.device.process_ff_erase(e).map_err(|e| e.to_string())?;
                        let valid = self.rumble.erase(request.effect_id() as usize);
                        request.set_retval(if valid { 0 } else { -libc::EINVAL });
                    }
                    EventSummary::ForceFeedback(_, code, value) => {
                        if code == FFEffectCode::FF_GAIN {
                            self.rumble.set_gain(value);
                        } else {
                            self.rumble.play(usize::from(code.0), value, Instant::now());
                        }
                    }
                    _ => {}
                }
            }
            let values = self.rumble.sample(Instant::now());
            if values != self.last
                || (values != [0, 0] && self.last_feedback.elapsed() >= Duration::from_secs(1))
            {
                self.last = values;
                self.last_feedback = Instant::now();
                Ok(Some(values))
            } else {
                Ok(None)
            }
        }
    }
    impl Drop for Pad {
        fn drop(&mut self) {
            let _ = self.neutralize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn axes_keep_linux_direction_endpoints_and_invalid_values_neutral() {
        let mut raw = [0u8; 64];
        raw[0] = 1;
        raw[1..5].fill(128);
        raw[8] = 8;
        let mut input = aftertouch_protocol::parse_input(&raw).unwrap();
        input.axes = [-1., 1., f32::NAN, f32::INFINITY];
        input.buttons[6] = f32::NAN;
        input.buttons[7] = 1.;
        input.buttons[12] = 1.;
        input.buttons[13] = 1.;
        input.buttons[14] = 1.;
        let r = Report::from_input(&input);
        assert_eq!(r.axes, [-32768, 32767, 0, 0, 0, 255, -1, 0]);
    }
    #[test]
    fn rumble_slots_are_bounded_delayed_repeated_mixed_and_expire() {
        let now = Instant::now();
        let mut bank = RumbleBank::new();
        let e = Effect {
            levels: [65535, 32768],
            length_ms: 50,
            delay_ms: 10,
        };
        let id = bank.upload(-1, e).unwrap();
        bank.play(id, 3, now);
        assert_eq!(bank.sample(now), [0, 0]);
        assert_eq!(bank.sample(now + Duration::from_millis(10)), [255, 127]);
        assert_eq!(bank.sample(now + Duration::from_millis(160)), [0, 0]);
        bank.play(id, 1, now);
        bank.set_gain(0);
        assert_eq!(bank.sample(now + Duration::from_millis(20)), [0, 0]);
        bank.set_gain(65535);
        bank.play(id, 0, now);
        assert_eq!(bank.sample(now + Duration::from_millis(20)), [0, 0]);
        for _ in 1..16 {
            bank.upload(-1, e).unwrap();
        }
        assert!(bank.upload(-1, e).is_err());
        assert!(bank.upload(16, e).is_err());
        assert!(bank.erase(id));
        assert!(!bank.erase(id));
        let infinite = bank
            .upload(
                -1,
                Effect {
                    length_ms: 0,
                    delay_ms: 0,
                    ..e
                },
            )
            .unwrap();
        bank.play(infinite, 1, now);
        assert_eq!(bank.sample(now + Duration::from_secs(11)), [0, 0]);
    }
}
