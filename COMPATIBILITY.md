# Compatibility and test record

Last checked: 21 October 2026 on Windows 11. This page records observed results rather than expected support. A protocol unit test verifies report construction; it cannot prove that a particular controller firmware or game accepts an effect.

| Setup | Input | Triggers and lightbar | Haptic audio | Game bridge |
| --- | --- | --- | --- | --- |
| Standard DualSense, USB (`054C:0CE6`) | Live checks: 232–233 reports/s, zero observed sequence gaps or rejected reports | Low-force resistance and experimental Bow reports were accepted; Reset sent a neutral report | Windows opened the four-channel endpoint and requested PCM frames; physical force and latency were not measured | Synthetic Forza Dash and local JSON packets decoded and produced UI telemetry |
| Standard DualSense, Bluetooth | Full and short input layouts have unit tests; no live Bluetooth pad was available for this record | `0x31` output framing and CRC have unit tests; no live write check | Motor rumble is the app fallback. The separate `0x32` advanced-haptics encoder is test-only | No live game check |
| DualSense Edge (`054C:0DF2`) | Edge button bits and identity have unit tests; no Edge hardware check | Firmware-dependent output behavior needs a physical Edge check | No Edge endpoint check | No live game check |

The optional XInput bridge needs an installed ViGEmBus driver. Its command path builds, but a three-game playability check has not been completed. Forza Motorsport 2023 and Horizon 4/5 packet layouts have synthetic decoder tests; no live racing session was used for this record.

Version 0.8.0 adds local telemetry rules, an explicit USB speaker chirp, and two-finger circle scrolling. On 21 October 2026, unit tests confirmed that a chirp writes only to the first two PCM channels at a bounded level, while regular grip output leaves them silent. The circle gesture test checks angular wrap, sudden touch-ID changes, and a rotation sample. The controller-free core smoke test confirmed that valid UDP telemetry can drive a scripted light and cue, that a runaway rule stops, and that the core stays alive afterward. The Playwright UI test covered the editor and speaker control. No pad was connected for this release's live smoke run, so the speaker chirp has not been audibly checked and the new paths have not been exercised on hardware.

Version 0.9.0 adds opt-in primary-display color sampling and a color mode based on the uncalibrated controller sensor estimate. A live desktop test read pixels through Windows GDI and received a sample from the background worker. Unit tests checked the color mix, smoothing, and sensor-color scale. Playwright covered the Lightbar controls and status. No controller was attached to measure the actual lightbar output or USB/Bluetooth update timing.

Version 0.10.0 renames the desktop app to CONTROLLER and copies valid profiles from the old Aftertouch data folder on first load. The migration test confirms the original archive remains intact. Cargo check, strict Clippy, Rust tests, the browser UI smoke test, and a controller-free core telemetry/script smoke test passed. The HID probe found no attached pad for this run, so live USB and Bluetooth writes, physical trigger force, and haptic output were not retested.

The app reads HID reports, uses Windows audio loopback, and listens for published telemetry. It does not inject code into games. Steam Input or another controller tool may write to the same controller; Diagnostics reports likely conflicts, and Reset releases effects when this app owns them.
