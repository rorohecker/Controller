//! Native desktop input for macOS and Linux. Constructed only when a pointer mode is enabled.
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
static FAILED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static INPUT:RefCell<Option<Enigo>>=const {RefCell::new(None)};
    static WHEEL:RefCell<i32>=const {RefCell::new(0)};
    // Raw virtual keys and mouse buttons (0 left, 1 right) currently held by us. They are
    // released before the backend is dropped so nothing stays down at OS level.
    static HELD_KEYS:RefCell<HashSet<u16>>=RefCell::new(HashSet::new());
    static HELD_BUTTONS:RefCell<[bool;2]>=const {RefCell::new([false;2])};
}
/// Left/right modifier variants map to one OS key, so they share a press count.
fn canonical(vk: u16) -> u16 {
    match vk {
        160 | 161 => 16,
        162 | 163 => 17,
        164 | 165 => 18,
        92 => 91,
        other => other,
    }
}
/// Release every key and button this process holds. Entries whose release failed, or that
/// cannot be released because no backend exists, stay tracked as pending and are retried by
/// the next `prepare()` (fresh backend) or `release_all()`. Errors never panic: best effort.
pub fn release_all() {
    INPUT.with(|input| {
        let mut guard = input.borrow_mut();
        let Some(e) = guard.as_mut() else { return };
        let mut canon_result: std::collections::HashMap<u16, bool> =
            std::collections::HashMap::new();
        let mut ordered: Vec<u16> = HELD_KEYS.with(|h| h.borrow().iter().copied().collect());
        // Release non-modifiers first, modifiers last.
        ordered.sort_by_key(|vk| crate::macros::is_modifier_key(*vk));
        for vk in ordered {
            let canon = canonical(vk);
            if canon_result.contains_key(&canon) {
                continue;
            }
            let released = match key_code(canon) {
                Some(key) => e.key(key, Direction::Release).is_ok(),
                None => true,
            };
            canon_result.insert(canon, released);
        }
        HELD_KEYS.with(|h| {
            h.borrow_mut()
                .retain(|vk| !canon_result.get(&canonical(*vk)).copied().unwrap_or(true));
        });
        HELD_BUTTONS.with(|b| {
            let mut b = b.borrow_mut();
            if b[0] && e.button(Button::Left, Direction::Release).is_ok() {
                b[0] = false;
            }
            if b[1] && e.button(Button::Right, Direction::Release).is_ok() {
                b[1] = false;
            }
        });
    });
}
/// True while some key or button release is still outstanding.
#[cfg_attr(test, allow(dead_code))]
fn has_pending() -> bool {
    HELD_KEYS.with(|h| !h.borrow().is_empty())
        || HELD_BUTTONS.with(|b| b.borrow().iter().any(|v| *v))
}
#[cfg_attr(test, allow(dead_code))]
pub fn prepare() -> Result<(), String> {
    if FAILED.swap(false, Ordering::AcqRel) {
        release_all();
        INPUT.with(|i| *i.borrow_mut() = None);
        WHEEL.with(|r| *r.borrow_mut() = 0);
    }
    #[cfg(target_os = "macos")]
    if crate::desktop_access::desktop_access_status()["controlAllowed"] != true {
        release_all();
        INPUT.with(|input| *input.borrow_mut() = None);
        return Err("Allow CONTROLLER in System Settings > Privacy & Security > Accessibility, then enable PC control again.".into());
    }
    INPUT.with(|input|{if input.borrow().is_none(){let enigo=Enigo::new(&Settings::default()).map_err(|e|format!("Desktop input unavailable: {e}. On macOS allow Accessibility access. On Wayland allow the desktop control prompt."))?;*input.borrow_mut()=Some(enigo);}Ok::<(),String>(())})?;
    // A fresh backend retries releases that failed or were left without a backend.
    if has_pending() {
        release_all();
    }
    Ok(())
}
pub fn recheck() -> Option<String> {
    if !INPUT.with(|i| i.borrow().is_some()) {
        return None;
    }
    #[cfg(target_os = "macos")]
    let revoked = crate::desktop_access::desktop_access_status()["controlAllowed"] != true;
    #[cfg(not(target_os = "macos"))]
    let revoked = false;
    if revoked || FAILED.swap(false, Ordering::AcqRel) {
        // Key-ups first, while the backend still exists; only then drop it.
        release_all();
        INPUT.with(|i| *i.borrow_mut() = None);
        WHEEL.with(|r| *r.borrow_mut() = 0);
        Some(
            if revoked {
                "Desktop access was revoked. Grant Accessibility access and test again."
            } else {
                "Desktop input failed. Test access again before re-enabling controls."
            }
            .into(),
        )
    } else {
        None
    }
}
pub fn mouse(dx: i32, dy: i32, wheel: i32, flags: u32) {
    INPUT.with(|input| {
        if let Some(e) = input.borrow_mut().as_mut() {
            if flags & 1 != 0 {
                report(e.move_mouse(dx, dy, Coordinate::Rel));
            }
            for (flag, index, button, press) in [
                (2, 0, Button::Left, true),
                (4, 0, Button::Left, false),
                (8, 1, Button::Right, true),
                (16, 1, Button::Right, false),
            ] {
                if flags & flag != 0 {
                    let direction = if press {
                        Direction::Press
                    } else {
                        Direction::Release
                    };
                    let ok = report(e.button(button, direction));
                    // A failed release stays tracked so release_all() retries it.
                    HELD_BUTTONS.with(|b| b.borrow_mut()[index] = if ok { press } else { !press });
                }
            }
            if flags & 0x800 != 0 {
                WHEEL.with(|remainder| {
                    // enigo only exposes whole-line scrolling, so sub-line amounts accumulate
                    // here and are emitted as soon as a full line has built up.
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
    let canon = canonical(vk);
    let Some(key) = key_code(canon) else { return };
    INPUT.with(|input| {
        let mut guard = input.borrow_mut();
        let Some(e) = guard.as_mut() else { return };
        if up {
            let (known, others_held) = HELD_KEYS.with(|h| {
                let h = h.borrow();
                (
                    h.contains(&vk),
                    h.iter().any(|k| *k != vk && canonical(*k) == canon),
                )
            });
            // Releasing one side of a modifier pair must not release the other.
            if !known || others_held {
                HELD_KEYS.with(|h| h.borrow_mut().remove(&vk));
                return;
            }
            if report(e.key(key, Direction::Release)) {
                HELD_KEYS.with(|h| h.borrow_mut().remove(&vk));
            }
        } else {
            let (already, canon_held) = HELD_KEYS.with(|h| {
                let h = h.borrow();
                (h.contains(&vk), h.iter().any(|k| canonical(*k) == canon))
            });
            if already {
                return;
            }
            if canon_held || report(e.key(key, Direction::Press)) {
                HELD_KEYS.with(|h| h.borrow_mut().insert(vk));
            }
        }
    });
}
/// Returns true when the call succeeded.
fn report(result: Result<(), enigo::InputError>) -> bool {
    if let Err(error) = result {
        FAILED.store(true, Ordering::Release);
        crate::input_map::record_input_error(format!("Desktop input failed: {error}"));
        false
    } else {
        true
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn left_and_right_modifiers_share_one_os_key() {
        assert_eq!(canonical(160), canonical(161));
        assert_eq!(canonical(162), 17);
        assert_eq!(canonical(92), 91);
        assert_eq!(canonical(65), 65);
    }
    #[test]
    fn unreleased_entries_stay_pending_without_a_backend() {
        HELD_KEYS.with(|h| h.borrow_mut().insert(65));
        HELD_BUTTONS.with(|b| b.borrow_mut()[0] = true);
        release_all();
        assert!(
            has_pending(),
            "nothing could be released, so nothing may be forgotten"
        );
        HELD_KEYS.with(|h| h.borrow_mut().clear());
        HELD_BUTTONS.with(|b| *b.borrow_mut() = [false; 2]);
        assert!(!has_pending());
    }
}
