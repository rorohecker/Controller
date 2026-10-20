use aftertouch_protocol::{InputState, TouchPoint};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
};

#[derive(Default)]
pub struct MouseMapper {
    enabled: bool,
    previous: Option<TouchPoint>,
    previous_scroll: Option<f32>,
    wheel_remainder: f32,
    left_down: bool,
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
    // A failed SendInput is harmless here; Windows blocks injection into higher integrity apps.
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
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
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled {
            return;
        }
        match input.touch {
            [Some(a), Some(b)] => {
                self.previous = None;
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
                if point.y >= 0.12 {
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
                    // The top strip is a clutch: leaving it must start a fresh drag.
                    self.previous = None;
                }
            }
            [None, None] => {
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
}
