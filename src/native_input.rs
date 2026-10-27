//! Native desktop input for macOS and Linux. Constructed only when a pointer mode is enabled.
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use std::cell::RefCell;
thread_local! { static INPUT:RefCell<Option<Enigo>>=const {RefCell::new(None)}; static WHEEL:RefCell<i32>=const {RefCell::new(0)}; }
#[cfg_attr(test, allow(dead_code))]
pub fn prepare() -> Result<(), String> {
    INPUT.with(|input|{if input.borrow().is_none(){let enigo=Enigo::new(&Settings::default()).map_err(|e|format!("Desktop input unavailable: {e}. On macOS allow Accessibility access. On Wayland allow the desktop control prompt."))?;*input.borrow_mut()=Some(enigo);}Ok(())})
}
pub fn mouse(dx: i32, dy: i32, wheel: i32, flags: u32) {
    INPUT.with(|input| {
        if let Some(e) = input.borrow_mut().as_mut() {
            if flags & 1 != 0 {
                report(e.move_mouse(dx, dy, Coordinate::Rel));
            }
            for (flag, button, direction) in [
                (2, Button::Left, Direction::Press),
                (4, Button::Left, Direction::Release),
                (8, Button::Right, Direction::Press),
                (16, Button::Right, Direction::Release),
            ] {
                if flags & flag != 0 {
                    report(e.button(button, direction));
                }
            }
            if flags & 0x800 != 0 {
                WHEEL.with(|remainder| {
                    let mut r = remainder.borrow_mut();
                    *r += wheel;
                    let steps = *r / 120;
                    *r -= steps * 120;
                    if steps != 0 {
                        report(e.scroll(-steps, Axis::Vertical));
                    }
                });
            }
        }
    });
}
pub fn key(vk: u16, up: bool) {
    if !supported_key(vk) {
        crate::input_map::record_input_error(format!("Key {vk} is not supported on this platform"));
        return;
    }
    if let Some(key) = key_code(vk) {
        INPUT.with(|input| {
            if let Some(e) = input.borrow_mut().as_mut() {
                report(e.key(
                    key,
                    if up {
                        Direction::Release
                    } else {
                        Direction::Press
                    },
                ));
            }
        });
    }
}
fn report(result: Result<(), enigo::InputError>) {
    if let Err(error) = result {
        crate::input_map::record_input_error(format!("Desktop input failed: {error}"));
    }
}
pub fn supported_key(vk: u16) -> bool {
    key_code(vk).is_some()
}
fn key_code(vk: u16) -> Option<Key> {
    Some(match vk {
        8 => Key::Backspace,
        9 => Key::Tab,
        13 => Key::Return,
        16 | 160 | 161 => Key::Shift,
        17 | 162 | 163 => Key::Control,
        18 | 164 | 165 => Key::Alt,
        20 => Key::CapsLock,
        27 => Key::Escape,
        32 => Key::Space,
        33 => Key::PageUp,
        34 => Key::PageDown,
        35 => Key::End,
        36 => Key::Home,
        37 => Key::LeftArrow,
        38 => Key::UpArrow,
        39 => Key::RightArrow,
        40 => Key::DownArrow,
        46 => Key::Delete,
        91 | 92 => Key::Meta,
        48..=57 => Key::Unicode(char::from_u32(u32::from(vk))?),
        65..=90 => Key::Unicode(char::from_u32(u32::from(vk + 32))?),
        112 => Key::F1,
        113 => Key::F2,
        114 => Key::F3,
        115 => Key::F4,
        116 => Key::F5,
        117 => Key::F6,
        118 => Key::F7,
        119 => Key::F8,
        120 => Key::F9,
        121 => Key::F10,
        122 => Key::F11,
        123 => Key::F12,
        124 => Key::F13,
        125 => Key::F14,
        126 => Key::F15,
        127 => Key::F16,
        128 => Key::F17,
        129 => Key::F18,
        130 => Key::F19,
        131 => Key::F20,
        #[cfg(target_os = "linux")]
        132 => Key::F21,
        #[cfg(target_os = "linux")]
        133 => Key::F22,
        #[cfg(target_os = "linux")]
        134 => Key::F23,
        #[cfg(target_os = "linux")]
        135 => Key::F24,
        96 => Key::Numpad0,
        97 => Key::Numpad1,
        98 => Key::Numpad2,
        99 => Key::Numpad3,
        100 => Key::Numpad4,
        101 => Key::Numpad5,
        102 => Key::Numpad6,
        103 => Key::Numpad7,
        104 => Key::Numpad8,
        105 => Key::Numpad9,
        106 => Key::Unicode('*'),
        107 => Key::Unicode('+'),
        109 => Key::Unicode('-'),
        110 => Key::Unicode('.'),
        111 => Key::Unicode('/'),
        173 => Key::VolumeMute,
        174 => Key::VolumeDown,
        175 => Key::VolumeUp,
        186 => Key::Unicode(';'),
        187 => Key::Unicode('='),
        188 => Key::Unicode(','),
        189 => Key::Unicode('-'),
        190 => Key::Unicode('.'),
        191 => Key::Unicode('/'),
        192 => Key::Unicode('`'),
        219 => Key::Unicode('['),
        220 => Key::Unicode('\\'),
        221 => Key::Unicode(']'),
        222 => Key::Unicode('\''),
        176 => Key::MediaNextTrack,
        177 => Key::MediaPrevTrack,
        179 => Key::MediaPlayPause,
        _ => return None,
    })
}
