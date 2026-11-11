# Software-completion pass: what changed

Baseline 0.22.0 (`c9221dc`). Nothing here has been verified on hardware unless stated; the [hardware test checklist](hardware-test-checklist.md) lists what to check, and the [remaining work tracker](software-completion-pass.md) has per-item status. This page is a summary written from the working tree, not an exhaustive list of fixes.

## Added

- **Trigger, light and audio output (unverified on hardware):** `setAdaptiveTrigger` (feedback, weapon, vibration, galloping, machine), `setMicrophoneLed` (off/solid/pulse), `setAudioVolumes`, `setLedBrightness`, `setLightbarAnimation` (breathe, pulse, rainbow, low-battery blink) and `setLowBatteryThresholds`.
- **Audio bridge and conversion:** bridge latency presets, a custom preset, 3-band mode, onset pulses and pan emphasis; 3-band (`bands:`) and Beats conversion kinds; `previewEnvelope`; re-attach to a restarted captured app for up to 30 s; sustained bounded Bluetooth grip PCM (experimental).
- **Profiles and integrations:** per-profile lightbar/player-LED state and audio preset reference; `recover_profiles`; import validation; `steam_input_status`; telemetry allowed-source restriction.
- **Input:** gyro activation modes, stick-deflection pause, speed-dependent smoothing and acceleration, flick stick while gyro is off; touchpad gain and acceleration; left stick as mouse or WASD in PC mode; remap layers, latch, long press, double tap, turbo, consume and configurable key timing; program-button run modes; Windows scancode key injection.
- **Platforms (earlier commits on this branch):** Linux `uinput` virtual pad; portable screen colour (ScreenCaptureKit, desktop portal); CoreAudio and PulseAudio/PipeWire-pulse capture; portable signed updater feeds and optional Apple notarization plumbing.
- **Lifecycle and diagnostics:** optional per-user background core with authenticated loopback IPC and an NSIS stop hook; Settings controls for background core and start-minimized; tray first-close notice; rotating `controller.log` and `core-service.log`; `export_diagnostics`; remembered per-controller output state re-applied on reconnect.
- **Edge:** opt-in transactional profile writer with backup, readback and recovery (never run on a physical Edge).
- **Repository:** `dist/` untracked and ignored, `.gitattributes`, CONTRIBUTING, pinned `rust-toolchain.toml`, `tests/requirements.txt`, Dependabot, pull-request CI with fmt, Clippy, tests and a version-consistency check, and a Linux browser gate.

## Fixed (selection)

- UI and clip rumble levels are scaled from 0-100 to the 0-255 motor range instead of being sent as raw bytes.
- Output writes distinguish a short HID write from hidapi's padded or unknown counts, so a truncated write is an error.
- Controller re-selection prefers the saved controller id, then the path, then USB over Bluetooth; paths are hints because they change between transports.
- Disconnects carry a structured state and reason; short Bluetooth reports report a "short-report" battery state; firmware feature version and build date are parsed from report 0x20.
- File decoding survives isolated corrupt packets and stream resets and learns the channel layout when a file omits it.
- A damaged `settings.json` is replaced with defaults and the original kept as `.bak`; a bad Edge transaction record no longer blocks every other transaction.
- Remap validation rejects sets that put competing actions on one button.

## Skipped by decision

DualShock 4 support; simultaneous multi-controller output; automatic HidHide configuration; Edge mapping decode; Bluetooth power-off command; Wayland foreground-window detector; tray profile switcher; gapless playlist backend; event-driven exclusive-mode audio; gyro-as-pointer in PC mode; per-axis stick invert; per-sector radial colour; expanded shortcut action types; drag-to-reorder lists.

## Known gaps

No starter profiles for specific games (backlog B5), no in-app hardware-test workspace (C2), no long-run fault-injection suite (C4), and Apple Developer ID signing and notarization are optional and unverified (Windows and Linux updater artifacts are signed by the release workflow). Git history still contains the old committed `dist/CONTROLLER.exe` builds; `dist/` is now untracked and ignored. Rewriting history (for example with `git filter-repo`) is optional and has not been done.
