# Hardware test checklist

Everything in this list is implemented in software and covered by unit, browser or controller-free core tests, but has **not** been verified on a physical controller or desktop. Record outcomes in the tables. A passing build, CI run or `--no-hid` core check does not substitute for any row.

How to use it: pick one controller and one platform per session, fill the Result columns (`pass`, `fail`, `partial`, `n/a`), and put what you saw in Notes (firmware version from Diagnostics, what you felt or heard, error text). Use **Diagnostics > Export diagnostics** for failures and attach the file. Press **Reset** (or PS + Options) between rows. None of these tests should need firmware changes; stop and use Reset if the controller behaves unexpectedly.

## Session record

| Field | Value |
| --- | --- |
| Date / tester | |
| App version and core build (Diagnostics) | |
| OS and version | |
| Controller model and firmware | DualSense / DualSense Edge, |
| Transport | USB / Bluetooth |
| Other tools running (Steam Input, DSX, DS4Windows) | |

## 1. Core output (run on USB first, then Bluetooth)

| # | Check | USB | BT | Notes |
| --- | --- | --- | --- | --- |
| 1.1 | Connect; input at about 230+ reports/s on USB; no sequence gaps in Diagnostics | | | |
| 1.2 | Bluetooth full report (0x31) parsed; battery level shown | n/a | | |
| 1.3 | Short Bluetooth report (0x01) only, before the controller sends full reports: app shows a short-report / battery-unavailable state and recovers when full reports arrive | n/a | | |
| 1.4 | Rumble scale: set 25%, 50%, 100% motor rumble from the UI; strength rises steadily and 100% is full strength | | | |
| 1.5 | Reconnect by controller id: after unplug/replug (or with two pads) the same pad is re-selected; switching USB to Bluetooth keeps the saved name, lights and remembered output | | | |
| 1.6 | Unplug during a lightbar animation and during playback: app reports the disconnect, no hang, no stuck output after reconnect | | | |
| 1.7 | Remembered output: set lightbar colour and player LEDs, reconnect the pad; they are re-applied, and Reset clears them | | | |

## 2. Unverified output features (DualSense)

| # | Check | USB | BT | Notes |
| --- | --- | --- | --- | --- |
| 2.1 | Adaptive trigger modes via `setAdaptiveTrigger`: feedback, weapon, vibration, galloping and machine on L2 and R2 each produce a distinct felt effect; Off clears it | | | |
| 2.2 | Microphone LED: off, solid and pulse. Confirm the pulse mode actually pulses (report layout comes from community documentation) | | | |
| 2.3 | Volumes: headphone (0-127), speaker (0-100), microphone (0-127) change the level of a headset, speaker and mic as expected | | | |
| 2.4 | Lightbar brightness: bright, medium, dim changes brightness; the lightbar-setup byte does not blank the light permanently (Reset recovers) | | | |
| 2.5 | Lightbar animations: breathe, pulse, rainbow and low-battery blink run smoothly (about 20 Hz), stop on Reset/disconnect, and do not flicker alongside screen colour or telemetry | | | |
| 2.6 | Low-battery thresholds: set warn/critical; the alert fires once on a discharging pad and re-arms after charging | | | |
| 2.7 | Mic button toggles hardware mute and the orange LED; audio playback keeps the mute state | | | |
| 2.8 | Bluetooth grip PCM (experimental): Enable Bluetooth grip audio, choose Grips only, play a file. Note whether it is felt, steady for 5+ minutes, dropped/underrun counts, and that it stops on Reset/disconnect | n/a | | |
| 2.9 | 3-band and Beats conversion kinds feel distinct from Full soundtrack; Preview envelope resembles what plays | | n/a | |
| 2.10 | System-audio bridge latency presets (low/normal/safe) and the custom preset: no crackle at "safe"; note whether "low" glitches | | n/a | |

## 3. DualSense Edge

| # | Check | Result | Notes |
| --- | --- | --- | --- |
| 3.1 | USB Edge read-only snapshot succeeds (12 feature reports `0x70`-`0x7B`); backups are written | | |
| 3.2 | Edge on Bluetooth is detected and a snapshot attempt does not corrupt anything | | |
| 3.3 | Opt-in Edge write transaction on a spare slot: fresh backup, write, readback matches, recovery works if you unplug mid-write. Do not use your only profile | | |
| 3.4 | Fn buttons, back paddles and stick-module state appear in the Diagnostics matrix | | |
| 3.5 | Edge mapping decode is **not** implemented; record which extra buttons do and do not appear | | |

## 4. Windows background core and installer

| # | Check | Result | Notes |
| --- | --- | --- | --- |
| 4.1 | Settings: enable background core, close the window; settings and lightbar stay applied; reopen the window and it reattaches | | |
| 4.2 | Unplug and replug the controller with no window open; remembered output is restored | | |
| 4.3 | Start minimized + start at sign-in: the app appears only in the tray; the first-close notification appears once | | |
| 4.4 | Install over a running app with the core active (NSIS pre-install hook `--stop-background-core`): no files-in-use error; the old core is gone afterwards | | |
| 4.5 | Uninstall with the core active: the hook stops it; no process or `CONTROLLER` files left in use | | |
| 4.6 | In-app update from one signed release to the next with the core running | | |
| 4.7 | `controller.log` and `core-service.log` exist under `%LOCALAPPDATA%\CONTROLLER\logs`, rotate at 1 MiB, and Export diagnostics redacts the user profile path | | |

## 5. Input, remapping and virtual pad

| # | Check | Result | Notes |
| --- | --- | --- | --- |
| 5.1 | Scancode key injection (Windows): a remapped key is accepted by a game that ignored virtual-key events; extended keys (arrows, right Ctrl/Alt) behave | | |
| 5.2 | Long-press, double-tap, turbo, layer latch and consume bindings fire once and release their keys; a conflicting set is rejected | | |
| 5.3 | Gyro modes hold / disable (ratchet) / toggle, stick-deflection pause, acceleration; flick stick with gyro off | | |
| 5.4 | Left stick as mouse and as WASD in PC mode; touchpad gain/acceleration | | |
| 5.5 | Windows ViGEm pad: calibrated axes; game rumble returns to the DualSense | | |
| 5.6 | Linux `/dev/uinput` pad appears to a game or `evtest`; axes/triggers calibrated; rumble reaches the pad; neutral on disconnect | | |

## 6. macOS and Linux platform checks

| # | Check | macOS | Linux | Notes |
| --- | --- | --- | --- | --- |
| 6.1 | HID input and output (hidraw rules on Linux) | | | |
| 6.2 | Desktop control permission flow: Accessibility / Input Monitoring on macOS; portal or X11 on Linux | | | |
| 6.3 | Screen colour: macOS Screen Recording prompt (ScreenCaptureKit); Linux desktop-portal consent and PipeWire; cancel and denial leave no stuck worker | | | |
| 6.4 | System audio capture: CoreAudio loopback (macOS 14.6+); PulseAudio/PipeWire-pulse; feedback-loop protection; four-channel USB endpoint if present | | | |
| 6.5 | Foreground game matching (macOS System Events; Linux X11/XWayland via `xprop`; pure Wayland needs manual selection) | | | |
| 6.6 | Package installs: `.deb`, AppImage, universal `.dmg`; Gatekeeper behavior for the unsigned dmg; udev rules | | | |
| 6.7 | Updater on AppImage and macOS (only after a signed release exists) | | | |

## 7. Native provider (vDS) and games

| # | Check | Result | Notes |
| --- | --- | --- | --- |
| 7.1 | With vDS installed: inspect, attach, detach, recover; the app releases HID/audio on attach and resumes after detach | | |
| 7.2 | A native-haptics game receives adaptive triggers/haptics through the provider over Bluetooth | | |
| 7.3 | Steam Input status check reports correctly with Steam Input on and off | | |
| 7.4 | Telemetry allowed-source restriction accepts the named sender and rejects another LAN address | | |

## Results summary

| Area | Verified on | Date | Open issues |
| --- | --- | --- | --- |
| USB output features | | | |
| Bluetooth output features | | | |
| Edge | | | |
| Background core and installer | | | |
| macOS | | | |
| Linux | | | |
| Native provider | | | |
