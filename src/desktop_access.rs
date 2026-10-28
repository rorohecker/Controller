//! Desktop permissions and explicitly armed, short-lived shortcut recording.
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

static RECORDING: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        static kAXTrustedCheckOptionPrompt: *const c_void;
        fn CGPreflightListenEventAccess() -> bool;
        fn CGRequestListenEventAccess() -> bool;
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: *const c_void;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            count: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(value: *const c_void);
    }
    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }
    pub fn listening() -> bool {
        unsafe { CGPreflightListenEventAccess() }
    }
    pub fn request(recording: bool) {
        unsafe {
            if recording {
                CGRequestListenEventAccess();
            } else {
                let options = CFDictionaryCreate(
                    std::ptr::null(),
                    &kAXTrustedCheckOptionPrompt,
                    &kCFBooleanTrue,
                    1,
                    std::ptr::null(),
                    std::ptr::null(),
                );
                if !options.is_null() {
                    AXIsProcessTrustedWithOptions(options);
                    CFRelease(options);
                }
            }
        }
    }
    pub fn pressed() -> Vec<u16> {
        // macOS physical key codes to the app's portable binding identifiers.
        const KEYS: &[(u16, u16)] = &[
            (0, 65),
            (1, 83),
            (2, 68),
            (3, 70),
            (4, 72),
            (5, 71),
            (6, 90),
            (7, 88),
            (8, 67),
            (9, 86),
            (11, 66),
            (12, 81),
            (13, 87),
            (14, 69),
            (15, 82),
            (16, 89),
            (17, 84),
            (18, 49),
            (19, 50),
            (20, 51),
            (21, 52),
            (22, 54),
            (23, 53),
            (24, 187),
            (25, 57),
            (26, 55),
            (27, 189),
            (28, 56),
            (29, 48),
            (30, 221),
            (31, 79),
            (32, 85),
            (33, 219),
            (34, 73),
            (35, 80),
            (36, 13),
            (37, 76),
            (38, 74),
            (39, 222),
            (40, 75),
            (41, 186),
            (42, 220),
            (43, 188),
            (44, 191),
            (45, 78),
            (46, 77),
            (47, 190),
            (48, 9),
            (49, 32),
            (50, 192),
            (51, 8),
            (53, 27),
            (55, 91),
            (54, 91),
            (56, 16),
            (60, 16),
            (58, 18),
            (61, 18),
            (59, 17),
            (62, 17),
            (123, 37),
            (124, 39),
            (125, 40),
            (126, 38),
            (122, 112),
            (120, 113),
            (99, 114),
            (118, 115),
            (96, 116),
            (97, 117),
            (98, 118),
            (100, 119),
            (101, 120),
            (109, 121),
            (103, 122),
            (111, 123),
        ];
        KEYS.iter()
            .filter_map(|&(code, vk)| unsafe { CGEventSourceKeyState(1, code).then_some(vk) })
            .collect()
    }
}

#[tauri::command]
pub fn desktop_access_status() -> Value {
    #[cfg(target_os = "macos")]
    {
        json!({"os":"macos","controlAllowed":mac::trusted(),
            "recordingAllowed":mac::listening(),"nativeRecording":true})
    }
    #[cfg(not(target_os = "macos"))]
    {
        json!({"os":std::env::consts::OS,"controlAllowed":true,
            "recordingAllowed":cfg!(windows),"nativeRecording":cfg!(windows)})
    }
}

#[tauri::command]
pub fn request_desktop_access(recording: bool) -> Value {
    #[cfg(target_os = "macos")]
    mac::request(recording);
    #[cfg(not(target_os = "macos"))]
    let _ = recording;
    desktop_access_status()
}

#[tauri::command]
pub fn open_desktop_access_settings(recording: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let pane = if recording {
            "Privacy_ListenEvent"
        } else {
            "Privacy_Accessibility"
        };
        let result = std::process::Command::new("/usr/bin/open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{pane}"
            ))
            .status()
            .map_err(|e| e.to_string())?;
        if result.success() {
            Ok(())
        } else {
            Err("Could not open System Settings.".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = recording;
        Err("Permission settings are available on macOS.".into())
    }
}

#[tauri::command]
pub fn cancel_shortcut_recording() {
    RECORDING.fetch_add(1, Ordering::SeqCst);
}

#[cfg(any(windows, target_os = "macos"))]
fn pressed_keys() -> Vec<u16> {
    #[cfg(target_os = "macos")]
    {
        mac::pressed()
    }
    #[cfg(windows)]
    {
        (8..=254u16)
            .filter(|key| unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(i32::from(*key))
                    < 0
            })
            .collect()
    }
}

#[cfg(any(windows, target_os = "macos", test))]
pub(crate) fn normalize_chord(keys: &[u16]) -> Vec<u16> {
    let mut chord = Vec::new();
    for &key in keys {
        let key = match key {
            160 | 161 => 16,
            162 | 163 => 17,
            164 | 165 => 18,
            92 => 91,
            key => key,
        };
        if !chord.contains(&key) {
            chord.push(key);
        }
    }
    chord.sort_by_key(|key| match key {
        17 => (0, 0),
        18 => (1, 0),
        16 => (2, 0),
        91 => (3, 0),
        key => (4, *key),
    });
    chord
}

#[tauri::command]
pub async fn record_shortcut() -> Result<Vec<u16>, String> {
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err("Use the shortcut builder for system shortcuts on this desktop.".into())
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        #[cfg(target_os = "macos")]
        if !mac::listening() {
            return Err(
                "Allow Input Monitoring to record system shortcuts, or use the shortcut builder."
                    .into(),
            );
        }
        let ticket = RECORDING.fetch_add(1, Ordering::SeqCst) + 1;
        tauri::async_runtime::spawn_blocking(move || {
            let started = std::time::Instant::now();
            let mut previous = pressed_keys();
            while started.elapsed() < std::time::Duration::from_secs(10) {
                if RECORDING.load(Ordering::SeqCst) != ticket {
                    return Err("Recording canceled.".into());
                }
                let current = pressed_keys();
                let chord = normalize_chord(&current);
                if chord.contains(&27) {
                    return Err("Recording canceled.".into());
                }
                if current.iter().any(|key| {
                    !previous.contains(key) && !matches!(key,16..=18 | 91 | 92 | 160..=165)
                }) {
                    return Ok(chord);
                }
                previous = current;
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err("No shortcut recorded. Use the modifier buttons and key selector instead.".into())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_modifiers_are_portable_and_unique() {
        assert_eq!(normalize_chord(&[165, 18, 9]), vec![18, 9]);
        assert_eq!(normalize_chord(&[92, 9]), vec![91, 9]);
        assert_eq!(normalize_chord(&[65, 161, 162]), vec![17, 16, 65]);
    }
}
