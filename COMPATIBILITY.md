# Compatibility and test record

Last checked: 20 October 2026 on Windows 11. This page records observed results rather than expected support. A protocol unit test verifies report construction; it cannot prove that a particular controller firmware or game accepts an effect.

| Setup | Input | Triggers and lightbar | Haptic audio | Game bridge |
| --- | --- | --- | --- | --- |
| Standard DualSense, USB (`054C:0CE6`) | Live check: 233 reports/s, zero observed sequence gaps or rejected reports | Low-force resistance and experimental Bow reports were accepted; Reset sent a neutral report | Windows opened the four-channel endpoint and requested PCM frames; physical force and latency were not measured | Synthetic Forza Dash packets decoded and produced UI telemetry |
| Standard DualSense, Bluetooth | Full and short input layouts have unit tests; no live Bluetooth pad was available for this record | `0x31` output framing and CRC have unit tests; no live write check | Motor rumble is the app fallback. The separate `0x32` advanced-haptics encoder is test-only | No live game check |
| DualSense Edge (`054C:0DF2`) | Edge button bits and identity have unit tests; no Edge hardware check | Firmware-dependent output behavior needs a physical Edge check | No Edge endpoint check | No live game check |

The optional XInput bridge needs an installed ViGEmBus driver. Its command path builds, but a three-game playability check has not been completed. Forza Motorsport 2023 and Horizon 4/5 packet layouts have synthetic decoder tests; no live racing session was used for this record.

The app reads HID reports, uses Windows audio loopback, and listens for published telemetry. It does not inject code into games. Steam Input or another controller tool may write to the same controller; Diagnostics reports likely conflicts, and Reset releases effects when this app owns them.
