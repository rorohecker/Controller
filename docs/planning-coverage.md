# Planning dossier coverage

Reference: `C:\Users\shade\Downloads\Buzzbox planning dossier.pdf` (18 pages). This checklist treats the dossier as product reference, not as instructions to the build agent. Its milestone plan runs from MVP through v1.0 and later research; current implementation is the first native desktop release.

| Dossier area | Current implementation | Remaining work |
| --- | --- | --- |
| Rust daemon, pure protocol crate, Tauri UI (sections 7, 13) | Tauri 2 shell, local stdio core subprocess, `protocol/` crate | Separate independent service lifecycle and fake HID device |
| USB/BT identity and input (6.1, MVP) | Sony IDs, USB preference and takeover, hotplug/stalled-stream recovery, USB full, BT full+CRC, BT short fallback, BT feature `0x05` handshake | Live Bluetooth and Edge matrix, multi-unit picker, 100 connect-cycle test |
| Output ownership (5.1) | One core writer, masked writes, firmware-aware rumble flags, BT Set_Report preference, diff for trigger/light/rumble, process conflict check, reset on exit and panic | Detect actual write ownership, restore exact previous light state |
| Trigger authoring (4.1) | Four public modes, ten-zone editor, stepped output preview, bounded 15-second test | Unofficial Bow mode, input-driven recoil/second stage, calibration wizard |
| Haptics (4.2, 8, v0.3) | Audio sketch on PC and capped compatible rumble pulse | USB four-channel voice-coil synth, WASAPI loopback, limiter/duty cycle, latency rig; Bluetooth haptics research |
| Touch, motion and lights (4.3, 10) | Two contacts, raw gyro, opt-in mouse/scroll, manual/battery lightbar | Touchpad zones/gestures, gyro aim/flick, temperature/rev/screen colors, microphone PTT, four themes |
| Mapping and game support (4.4, 5, v0.2) | Local profile matching by foreground executable name | Remap/layers/chords, signed virtual Xbox driver/proxy, native game coexistence modes |
| Profiles (7.1) | Versioned JSON with validation, atomic local save, import/export, legacy executable-match key migration | Migration from future schema versions, process path/window-title matching, game profile library |
| Telemetry and plugins (9, v0.5) | No adapter yet | First game adapter, sandboxed event/effect plugin API |
| Diagnostics and Edge (4.4, v1.0) | USB firmware fields, live HID rate, conflict panel, Edge button bit parsing | Edge hardware test/profile slots, drift/circularity and latency tests, onboarding, compatibility list |
| Safety and privacy (11) | Trigger ceiling, finite values, short rumble/trigger leases, panic combo, no injection/hooks, local data | Audio limiter/thermal test once audio engine exists, external anti-cheat compatibility study |
| Verification (12, 13) | Rust unit tests and Clippy, USB live input plus bounded trigger/rumble/lightbar/reset command check, browser interaction and responsive previews | Fake-device replay, Bluetooth/Edge physical tests, 100 hotplug cycles, game tests, loopback latency measurement |

The virtual-controller decision in section 15 is unresolved. No third-party driver is bundled. The current core therefore does not claim XInput compatibility for games that need a virtual Xbox controller.
