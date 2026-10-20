//! Gyro aim, touch gestures, and button→keyboard remapping via SendInput.

use aftertouch_protocol::{InputState, TouchPoint};
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
    pub fn from_label(label: &str) -> Self {
        match label {
            "l2" => Self::L2,
            "r2" => Self::R2,
            "l1" => Self::L1,
            "r1" => Self::R1,
            "touchpad" => Self::Touchpad,
            _ => Self::Always,
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
}

impl GyroAimMapper {
    pub fn set_config(&mut self, config: GyroAimConfig) {
        self.config = config;
        self.remainder_x = 0.0;
        self.remainder_y = 0.0;
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.config.enabled || !self.config.activation.active(input) {
            return;
        }
        // DualSense gyro: body[15]=X, [17]=Y, [19]=Z in Linux order; yaw≈Z, pitch≈X.
        let yaw = input.gyro[2] as f32;
        let pitch = input.gyro[0] as f32;
        let sens = self.config.sensitivity.clamp(0.05, 8.0);
        let dx = yaw * self.config.yaw_scale.clamp(0.0, 4.0) * sens * 0.0022;
        let mut dy = pitch * self.config.pitch_scale.clamp(0.0, 4.0) * sens * 0.0022;
        if self.config.invert_y {
            dy = -dy;
        }
        self.remainder_x += dx;
        self.remainder_y += dy;
        let mx = self.remainder_x.trunc() as i32;
        let my = self.remainder_y.trunc() as i32;
        if mx != 0 || my != 0 {
            self.remainder_x -= mx as f32;
            self.remainder_y -= my as f32;
            send_mouse(mx.clamp(-48, 48), my.clamp(-48, 48), 0, MOUSEEVENTF_MOVE);
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemapBinding {
    pub button: u8,
    pub vk: u16,
}

#[derive(Default)]
pub struct RemapMapper {
    enabled: bool,
    bindings: Vec<RemapBinding>,
    pressed: [bool; 23],
}

impl RemapMapper {
    pub fn set(&mut self, enabled: bool, bindings: Vec<RemapBinding>) {
        if !enabled {
            for (i, was) in self.pressed.iter_mut().enumerate() {
                if *was {
                    if let Some(binding) = self.bindings.iter().find(|b| b.button as usize == i) {
                        send_key(binding.vk, true);
                    }
                    *was = false;
                }
            }
        }
        self.enabled = enabled;
        self.bindings = bindings;
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled {
            return;
        }
        for binding in &self.bindings {
            let idx = binding.button as usize;
            if idx >= 23 {
                continue;
            }
            let down = input.buttons[idx] > 0.5;
            if down != self.pressed[idx] {
                send_key(binding.vk, !down);
                self.pressed[idx] = down;
            }
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
    edge_anchor: Option<(f32, f32)>,
}

impl MouseMapper {
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
    }

    pub fn set_gestures(&mut self, gestures: bool) {
        self.gestures = gestures;
        self.edge_anchor = None;
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled {
            return;
        }
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
                if self.gestures && point.x <= 0.12 {
                    self.handle_edge(point, true);
                } else if self.gestures && point.x >= 0.88 {
                    self.handle_edge(point, false);
                } else if point.y >= 0.12 {
                    self.edge_anchor = None;
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
                    && let Some((start_x, start_y)) = self.edge_anchor.take()
                {
                    // Edge-zone release fires browser-style history keys.
                    if (start_y - 0.5).abs() < 0.45 {
                        if start_x <= 0.12 {
                            tap_key(VK_LEFT);
                        } else if start_x >= 0.88 {
                            tap_key(VK_RIGHT);
                        }
                    }
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
    }

    fn handle_edge(&mut self, point: TouchPoint, _left: bool) {
        self.previous = None;
        if self.edge_anchor.is_none() {
            self.edge_anchor = Some((point.x, point.y));
        }
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
