//! Gyro aim, touch gestures, and button→keyboard remapping via SendInput.

use aftertouch_protocol::{InputState, TouchPoint};
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
};

const VK_LEFT: u16 = 0x25;
const VK_RIGHT: u16 = 0x27;

#[derive(Clone, Debug, Default)]
pub struct GyroAimConfig {
    pub enabled: bool,
    pub sensitivity: f32,
    pub yaw_scale: f32,
    pub pitch_scale: f32,
    pub invert_y: bool,
    pub activation: GyroActivation,
    pub bias: [i16; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum GyroActivation {
    #[default]
    Always,
    L2,
    R2,
    L1,
    R1,
    Touchpad,
}

impl GyroActivation {
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "always" => Some(Self::Always),
            "l2" => Some(Self::L2),
            "r2" => Some(Self::R2),
            "l1" => Some(Self::L1),
            "r1" => Some(Self::R1),
            "touchpad" => Some(Self::Touchpad),
            _ => None,
        }
    }

    fn active(&self, input: &InputState) -> bool {
        match self {
            Self::Always => true,
            Self::L2 => input.buttons[6] > 0.15,
            Self::R2 => input.buttons[7] > 0.15,
            Self::L1 => input.buttons[4] > 0.5,
            Self::R1 => input.buttons[5] > 0.5,
            Self::Touchpad => input.buttons[17] > 0.5 || input.touch.iter().any(Option::is_some),
        }
    }
}

#[derive(Default)]
pub struct GyroAimMapper {
    pub config: GyroAimConfig,
    remainder_x: f32,
    remainder_y: f32,
    last_sample: Option<Instant>,
}

impl GyroAimMapper {
    pub fn set_config(&mut self, config: GyroAimConfig) {
        self.config = config;
        self.remainder_x = 0.0;
        self.remainder_y = 0.0;
        self.last_sample = None;
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.config.enabled || !input.full_report || !self.config.activation.active(input) {
            self.last_sample = None;
            self.remainder_x = 0.0;
            self.remainder_y = 0.0;
            return;
        }
        let now = Instant::now();
        let dt = self
            .last_sample
            .replace(now)
            .map(|previous| now.duration_since(previous).as_secs_f32().min(0.025))
            .unwrap_or(0.0);
        if dt == 0.0 {
            return;
        }
        // The Linux driver documents 1024 raw units per degree per second.
        // Integrating with elapsed time keeps pointer speed stable across USB and BT rates.
        let yaw = (input.gyro[2] as i32 - self.config.bias[2] as i32) as f32;
        let pitch = (input.gyro[0] as i32 - self.config.bias[0] as i32) as f32;
        let sens = self.config.sensitivity.clamp(0.05, 8.0);
        let to_pixels = |raw: f32| {
            if raw.abs() < 90.0 {
                0.0
            } else {
                raw / 1024.0 * dt * 18.0
            }
        };
        let dx = to_pixels(yaw) * self.config.yaw_scale.clamp(0.0, 4.0) * sens;
        let mut dy = to_pixels(pitch) * self.config.pitch_scale.clamp(0.0, 4.0) * sens;
        if self.config.invert_y {
            dy = -dy;
        }
        self.remainder_x += dx;
        self.remainder_y += dy;
        self.remainder_x = self.remainder_x.clamp(-48.0, 48.0);
        self.remainder_y = self.remainder_y.clamp(-48.0, 48.0);
        let mx = self.remainder_x.trunc() as i32;
        let my = self.remainder_y.trunc() as i32;
        if mx != 0 || my != 0 {
            self.remainder_x -= mx as f32;
            self.remainder_y -= my as f32;
            send_mouse(mx, my, 0, MOUSEEVENTF_MOVE);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemapBinding {
    pub button: u8,
    pub vk: u16,
    pub modifier: Option<u8>,
    pub behavior: RemapBehavior,
    pub sequence: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemapBehavior {
    Hold,
    Tap,
    Macro,
}

impl RemapBehavior {
    pub fn from_label(value: &str) -> Option<Self> {
        match value {
            "hold" => Some(Self::Hold),
            "tap" => Some(Self::Tap),
            "macro" => Some(Self::Macro),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct RemapMapper {
    enabled: bool,
    suspended: bool,
    bindings: Vec<RemapBinding>,
    keys_down: HashSet<u16>,
    taps_down: HashSet<(u8, Option<u8>)>,
    macro_queue: VecDeque<u16>,
    macro_held: Option<(u16, Instant)>,
    next_macro_at: Option<Instant>,
}

impl RemapMapper {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set(&mut self, enabled: bool, bindings: Vec<RemapBinding>) {
        if self.enabled == enabled && self.bindings == bindings {
            return;
        }
        self.release_all();
        self.enabled = enabled;
        self.bindings = bindings;
    }

    pub fn set_suspended(&mut self, suspended: bool) {
        if self.suspended == suspended {
            return;
        }
        if suspended {
            self.release_all();
        }
        self.suspended = suspended;
    }

    fn release_all(&mut self) {
        for vk in self.keys_down.drain() {
            send_key(vk, true);
        }
        if let Some((vk, _)) = self.macro_held.take() {
            send_key(vk, true);
        }
        self.taps_down.clear();
        self.macro_queue.clear();
        self.next_macro_at = None;
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled || self.suspended {
            return;
        }
        let selected = self.active_bindings(input);
        let active = self.active_keys(input);
        for vk in self.keys_down.difference(&active) {
            if self.macro_held.is_none_or(|(held, _)| held != *vk) {
                send_key(*vk, true);
            }
        }
        for vk in active.difference(&self.keys_down) {
            send_key(*vk, false);
        }
        self.keys_down = active;
        let mut taps_down = HashSet::new();
        for index in selected {
            let binding = &self.bindings[index];
            if binding.behavior == RemapBehavior::Hold {
                continue;
            }
            let identity = (binding.button, binding.modifier);
            taps_down.insert(identity);
            if self.taps_down.contains(&identity) {
                continue;
            }
            if binding.behavior == RemapBehavior::Tap {
                if !self.keys_down.contains(&binding.vk) {
                    tap_key(binding.vk);
                }
            } else if self.macro_queue.len() + binding.sequence.len() < 32 {
                self.macro_queue.push_back(binding.vk);
                self.macro_queue.extend(binding.sequence.iter().copied());
            }
        }
        self.taps_down = taps_down;
        self.advance_macro();
    }

    pub fn tick(&mut self) {
        if self.enabled && !self.suspended {
            self.advance_macro();
        }
    }

    fn active_bindings(&self, input: &InputState) -> Vec<usize> {
        let active_chords: Vec<&RemapBinding> = self
            .bindings
            .iter()
            .filter(|binding| {
                binding
                    .modifier
                    .is_some_and(|modifier| input.buttons[modifier as usize] > 0.5)
                    && input.buttons[binding.button as usize] > 0.5
            })
            .collect();
        let chorded: HashSet<u8> = active_chords.iter().map(|binding| binding.button).collect();
        let engaged_modifiers: HashSet<u8> = active_chords
            .iter()
            .filter_map(|binding| binding.modifier)
            .collect();
        self.bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| {
                input.buttons[binding.button as usize] > 0.5
                    && (binding.modifier.is_some() || !engaged_modifiers.contains(&binding.button))
                    && binding
                        .modifier
                        .map_or(!chorded.contains(&binding.button), |modifier| {
                            input.buttons[modifier as usize] > 0.5
                        })
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn active_keys(&self, input: &InputState) -> HashSet<u16> {
        self.active_bindings(input)
            .iter()
            .map(|&index| &self.bindings[index])
            .filter(|binding| binding.behavior == RemapBehavior::Hold)
            .map(|binding| binding.vk)
            .collect()
    }

    fn advance_macro(&mut self) {
        let now = Instant::now();
        if let Some((vk, until)) = self.macro_held {
            if now < until {
                return;
            }
            if !self.keys_down.contains(&vk) {
                send_key(vk, true);
            }
            self.macro_held = None;
            self.next_macro_at = Some(now + Duration::from_millis(20));
        }
        if self.next_macro_at.is_some_and(|at| now < at) {
            return;
        }
        if let Some(vk) = self.macro_queue.pop_front() {
            if !self.keys_down.contains(&vk) {
                send_key(vk, false);
            }
            self.macro_held = Some((vk, now + Duration::from_millis(45)));
        }
    }
}

#[derive(Default)]
pub struct MouseMapper {
    enabled: bool,
    gestures: bool,
    previous: Option<TouchPoint>,
    previous_scroll: Option<f32>,
    wheel_remainder: f32,
    left_down: bool,
    edge_anchor: Option<EdgeGesture>,
    previous_zone: Option<TouchZone>,
    last_zone_tick: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchCue {
    Left,
    Right,
    Top,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TouchZone {
    Centre,
    Left,
    Right,
    Top,
}

impl TouchZone {
    fn at(point: TouchPoint) -> Self {
        if point.y <= 0.10 {
            Self::Top
        } else if point.x <= 0.10 {
            Self::Left
        } else if point.x >= 0.90 {
            Self::Right
        } else {
            Self::Centre
        }
    }

    fn cue(self) -> Option<TouchCue> {
        match self {
            Self::Left => Some(TouchCue::Left),
            Self::Right => Some(TouchCue::Right),
            Self::Top => Some(TouchCue::Top),
            Self::Centre => None,
        }
    }
}

struct EdgeGesture {
    started: Instant,
    left: bool,
    start: TouchPoint,
    last: TouchPoint,
}

impl EdgeGesture {
    fn arrow_key(&self) -> Option<u16> {
        if self.started.elapsed() > Duration::from_millis(750)
            || (self.last.y - self.start.y).abs() > 0.12
        {
            return None;
        }
        let dx = self.last.x - self.start.x;
        if self.left && dx >= 0.18 {
            Some(VK_RIGHT)
        } else if !self.left && dx <= -0.18 {
            Some(VK_LEFT)
        } else {
            None
        }
    }
}

impl MouseMapper {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled && self.left_down {
            send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
        }
        self.enabled = enabled;
        self.previous = None;
        self.previous_scroll = None;
        self.wheel_remainder = 0.0;
        self.left_down = false;
        self.edge_anchor = None;
        self.previous_zone = None;
        self.last_zone_tick = None;
    }

    pub fn set_gestures(&mut self, gestures: bool) {
        self.gestures = gestures;
        self.edge_anchor = None;
        self.previous_zone = None;
        self.last_zone_tick = None;
    }

    fn update_zone(&mut self, touch: [Option<TouchPoint>; 2]) -> Option<TouchCue> {
        let point = match touch {
            [Some(point), None] | [None, Some(point)] => point,
            _ => {
                self.previous_zone = None;
                return None;
            }
        };
        let zone = TouchZone::at(point);
        let previous = self.previous_zone.replace(zone);
        if previous.is_some_and(|prior| prior != zone)
            && self
                .last_zone_tick
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(150))
            && let Some(cue) = zone.cue()
        {
            self.last_zone_tick = Some(Instant::now());
            return Some(cue);
        }
        None
    }

    pub fn update(&mut self, input: &InputState) -> Option<TouchCue> {
        if !self.enabled {
            return None;
        }
        let cue = if self.gestures {
            self.update_zone(input.touch)
        } else {
            None
        };
        match input.touch {
            [Some(a), Some(b)] => {
                self.previous = None;
                self.edge_anchor = None;
                let centre = (a.y + b.y) * 0.5;
                if let Some(previous) = self.previous_scroll {
                    self.wheel_remainder += (previous - centre) * 900.0;
                    let detents = (self.wheel_remainder / 120.0).trunc() as i32;
                    if detents != 0 {
                        let wheel = detents.clamp(-2, 2) * 120;
                        send_mouse(0, 0, wheel, MOUSEEVENTF_WHEEL);
                        self.wheel_remainder -= wheel as f32;
                    }
                }
                self.previous_scroll = Some(centre);
            }
            [Some(point), None] | [None, Some(point)] => {
                self.previous_scroll = None;
                if let Some(gesture) = self.edge_anchor.as_mut() {
                    gesture.last = point;
                    self.previous = None;
                } else if self.gestures
                    && self.previous.is_none()
                    && (point.x <= 0.12 || point.x >= 0.88)
                {
                    self.edge_anchor = Some(EdgeGesture {
                        started: Instant::now(),
                        left: point.x <= 0.12,
                        start: point,
                        last: point,
                    });
                    self.previous = None;
                } else if point.y >= 0.12 {
                    if let Some(previous) = self.previous {
                        let dx = ((point.x - previous.x) * 1250.0).round() as i32;
                        let dy = ((point.y - previous.y) * 780.0).round() as i32;
                        let dx = dx.clamp(-36, 36);
                        let dy = dy.clamp(-36, 36);
                        if dx != 0 || dy != 0 {
                            send_mouse(dx, dy, 0, MOUSEEVENTF_MOVE);
                        }
                    }
                    self.previous = Some(point);
                } else {
                    // Top strip clutch: leaving it starts a fresh drag.
                    self.previous = None;
                    self.edge_anchor = None;
                }
            }
            [None, None] => {
                if self.gestures
                    && let Some(gesture) = self.edge_anchor.take()
                    && let Some(vk) = gesture.arrow_key()
                {
                    tap_key(vk);
                }
                self.previous = None;
                self.previous_scroll = None;
            }
        }
        let should_press = input.buttons[17] > 0.5 && input.touch.iter().any(Option::is_some);
        if should_press != self.left_down {
            send_mouse(
                0,
                0,
                0,
                if should_press {
                    MOUSEEVENTF_LEFTDOWN
                } else {
                    MOUSEEVENTF_LEFTUP
                },
            );
            self.left_down = should_press;
        }
        cue
    }
}

fn send_mouse(dx: i32, dy: i32, wheel: i32, flags: u32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: wheel as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_key(vk: u16, up: bool) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

fn tap_key(vk: u16) {
    send_key(vk, false);
    send_key(vk, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_buttons_sharing_key_keep_it_active_until_both_release() {
        let mapper = RemapMapper {
            enabled: true,
            bindings: vec![
                RemapBinding {
                    button: 0,
                    vk: 65,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                },
                RemapBinding {
                    button: 1,
                    vk: 65,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                },
            ],
            ..Default::default()
        };
        let mut report = [0u8; 64];
        report[0] = 0x01;
        report[8] = 0x68;
        let both = aftertouch_protocol::parse_input(&report).unwrap();
        assert_eq!(mapper.active_keys(&both), HashSet::from([65]));
        report[8] = 0x48;
        let one = aftertouch_protocol::parse_input(&report).unwrap();
        assert_eq!(mapper.active_keys(&one), HashSet::from([65]));
        report[8] = 0x08;
        let none = aftertouch_protocol::parse_input(&report).unwrap();
        assert!(mapper.active_keys(&none).is_empty());
    }

    #[test]
    fn held_modifier_selects_layer_binding_over_base() {
        let mapper = RemapMapper {
            enabled: true,
            bindings: vec![
                RemapBinding {
                    button: 0,
                    vk: 65,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                },
                RemapBinding {
                    button: 0,
                    vk: 66,
                    modifier: Some(4),
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                },
                RemapBinding {
                    button: 4,
                    vk: 67,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                },
            ],
            ..Default::default()
        };
        let mut report = [0u8; 64];
        report[0] = 0x01;
        let mut input = aftertouch_protocol::parse_input(&report).unwrap();
        input.buttons[0] = 1.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([65]));
        input.buttons[4] = 1.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([66]));
        input.buttons[0] = 0.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([67]));
    }

    #[test]
    fn edge_tap_does_not_fire_but_inward_swipe_does() {
        let start = TouchPoint { x: 0.04, y: 0.5 };
        let mut gesture = EdgeGesture {
            started: Instant::now(),
            left: true,
            start,
            last: start,
        };
        assert_eq!(gesture.arrow_key(), None);
        gesture.last.x = 0.31;
        assert_eq!(gesture.arrow_key(), Some(VK_RIGHT));
        gesture.last.y = 0.8;
        assert_eq!(gesture.arrow_key(), None);
    }

    #[test]
    fn touch_zone_tick_only_fires_on_a_new_zone() {
        let mut mapper = MouseMapper::default();
        let centre = TouchPoint { x: 0.5, y: 0.5 };
        let left = TouchPoint { x: 0.05, y: 0.5 };
        assert_eq!(mapper.update_zone([Some(centre), None]), None);
        assert_eq!(mapper.update_zone([Some(left), None]), Some(TouchCue::Left));
        assert_eq!(mapper.update_zone([Some(left), None]), None);
        assert_eq!(mapper.update_zone([None, None]), None);
        assert_eq!(mapper.update_zone([Some(left), None]), None);
    }
}
