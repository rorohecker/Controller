# Planning dossier coverage

Reference: `C:\Users\shade\Downloads\Buzzbox planning dossier.pdf` (18 pages). This checklist treats the dossier as product reference, not as instructions to the build agent. Its milestone plan runs from MVP through v1.0 and later research; current implementation is the native desktop release through 0.3.

| Dossier area | Current implementation | Remaining work |
| --- | --- | --- |
| Rust daemon, pure protocol crate, Tauri UI (sections 7, 13) | Tauri 2 shell, local stdio core subprocess with auto-respawn, `protocol/` crate | Separate independent service lifecycle and fake HID device |
| USB/BT identity and input (6.1, MVP) | Sony IDs, USB preference and takeover, hotplug/stalled-stream recovery with reset-before-drop, USB full, BT full+CRC, BT short fallback, BT feature `0x05` handshake, multi-unit picker | Live Bluetooth and Edge matrix, 100 connect-cycle test |
| Output ownership (5.1) | One core writer, masked writes, firmware-aware rumble flags, BT Set_Report preference, diff for trigger/light/rumble, process conflict check, reset on exit, panic, stall, and core restart | Detect actual write ownership, restore exact previous light state |
| Trigger authoring (4.1) | Four public modes, ten-zone editor, stepped output preview, bounded 15-second test, profile auto-apply | Unofficial Bow mode, input-driven recoil/second stage, calibration wizard |
| Haptics (4.2, 8, v0.3) | Audio sketch on PC plus frequency-shaped motor envelope on DualSense rumble | USB four-channel voice-coil PCM synth, WASAPI loopback, limiter/duty cycle, latency rig; Bluetooth haptics research |
| Touch, motion and lights (4.3, 10) | Two contacts, raw gyro, opt-in mouse/scroll, edge swipe gestures, gyro aim to pointer, manual/battery lightbar | Temperature/rev/screen colors, microphone PTT, four themes, flick aim presets |
| Mapping and game support (4.4, 5, v0.2) | Local profile matching by foreground executable with optional hardware auto-apply; keyboard remap layer via SendInput | Signed virtual Xbox driver/proxy (external ViGEm), native game coexistence modes, chord layers |
| Profiles (7.1) | Schema v2 JSON with validation, atomic local save, import/export, legacy schema migration, process match + auto-apply | Process path/window-title matching, game profile library |
| Telemetry and plugins (9, v0.5) | No adapter yet | First game adapter, sandboxed event/effect plugin API |
| Diagnostics and Edge (4.4, v1.0) | USB firmware fields, live HID rate, conflict panel, Edge button bit parsing, device list/picker, core health | Edge hardware test/profile slots, drift/circularity and latency tests, onboarding, compatibility list |
| Safety and privacy (11) | Trigger ceiling, finite values, short rumble/trigger leases, panic combo, reset-on-stall, no injection/hooks, local data | Audio limiter/thermal test once audio engine exists, external anti-cheat compatibility study |
| Verification (12, 13) | Rust unit tests and Clippy, USB live input plus bounded trigger/rumble/lightbar/reset command check, browser interaction and responsive previews | Fake-device replay, Bluetooth/Edge physical tests, 100 hotplug cycles, game tests, loopback latency measurement |

The virtual-controller decision in section 15 remains unresolved for bundling. No third-party driver is shipped. Keyboard remap and pointer modes cover desktop input; games that need XInput compatibility still require an external virtual pad.
