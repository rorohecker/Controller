# Aftertouch

A Windows desktop workbench for DualSense and DualSense Edge controllers, built with Rust and Tauri 2. The planning dossier checklist lives in [docs/planning-coverage.md](docs/planning-coverage.md). This is a native HID authoring workbench; some later-roadmap items (USB voice-coil PCM, signed virtual Xbox driver) remain out of scope for the bundled binary.

## Run

Launch `dist/Aftertouch.exe`, or build the current source:

```powershell
cargo run --release --bin aftertouch
```

Windows 10/11 and WebView2 are required. Connect a controller by USB or Bluetooth. The core scans automatically, prefers USB when both transports appear, lists every pad in Diagnostics for manual selection, and shows competing controller tools. It does not install a driver or modify games.

## What works

- Live input overlay on an actual DualSense photograph: face buttons, D-pad, shoulders, analog triggers, sticks, touchpad, PS, microphone button, and Edge-specific inputs in the diagnostic matrix. Touch contacts, raw gyro, battery, firmware and HID report rate are displayed when the controller sends them.
- USB `0x02` output and Bluetooth `0x31` output with sequence nibble and seeded CRC32. Bluetooth output prefers HID Set_Report and falls back to interrupt writes when unavailable. Bluetooth startup requests full `0x31` input via feature report `0x05`; the short `0x01` input is also parsed as a fallback. Physical Bluetooth and Edge hardware have not been available for live validation.
- Per-trigger ten-zone curves with off, resistance, weapon and vibration modes. Applying both triggers requires a button press (or auto-apply) and expires after 15 seconds. Haptic Lab sketches play computer audio and a frequency-shaped motor envelope on the DualSense rumble motors. Lightbar manual/battery-color control also uses native HID writes.
- Touchpad mouse mode on Windows: one-finger relative movement, two-finger scroll, top clutch strip, and click with a pressed touchpad. Optional edge swipe gestures send ← / →. Opt-in gyro aim moves the Windows pointer from yaw/pitch with hold-to-aim activation. Keyboard remap layers bind DualSense buttons to virtual keys. All of these use SendInput; there is no game-process injection and no bundled virtual Xbox driver.
- Versioned local JSON profiles (schema v2) with import/export, atomic replacement on save, executable-name matching, and optional **auto-apply** that pushes triggers / gyro / remap to hardware when a matched process is focused. Session rail toggle controls global auto-apply.
- A Rust subprocess owns each HID handle and sends output on change. It rechecks device presence and the input stream, **resets effects before dropping a stalled handle**, reconnects after an unplug, switches from Bluetooth to USB when available, and **respawns** if the HID process dies. Reset, app exit and PS + Options return active triggers to neutral, stop rumble and release the lightbar. The safety ceiling is six of eight trigger levels. Steam, DS4Windows, DSX and reWASD process checks warn about possible output contention.
- Local waveform sketches, texture presets, keyboard input demo in a browser, animated instrument panels, and reduced-motion styling. A persistent session strip shows the current profile, input stream health, trigger lease countdown, auto-apply state, and quick reset.

## Current boundaries

USB four-channel voice-coil PCM / WASAPI loopback is not shipped; the Haptic Lab uses a motor envelope plus a quiet audio sketch. A signed virtual Xbox / XInput proxy is not bundled — keyboard remap covers desktop shortcuts; games that require a virtual pad need an external bus such as ViGEm. Telemetry adapters, plugin scripting and a community profile library are not implemented. The dossier's 100-cycle hotplug, Bluetooth, Edge and latency acceptance tests remain to be run on their respective hardware.

On the connected Windows standard DualSense, USB full input was read at about 250 reports per second. Bounded trigger, rumble, lightbar, and reset commands were accepted by the core with no reported errors. HIDAPI confirms submission to Windows, not physical actuation. Bluetooth report generation, CRC verification, short input fallback and trigger encoding are covered by protocol tests; Bluetooth delivery still needs a physical test.

## Development

```powershell
cargo test --workspace
cargo run --bin aftertouch-probe -- --smoke-output
cargo build --release --bin aftertouch
```

The Tauri shell embeds `ui/` and starts the same executable with `--core` as its local HID process. `protocol/` contains I/O-free report parsing and construction. Profiles are stored at `%APPDATA%\Aftertouch\profiles.json`. The browser version of `ui/index.html` supports layout preview and keyboard demo; device writes require the desktop app.

## Sources and art

Protocol layout and flags follow the [Linux DualSense driver](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-playstation.c); trigger blocks follow [Nielk1's public trigger research](https://gist.github.com/Nielk1/6d54cc2c00d2201ccb8c2720ad7538db). The controller photo is [Alex Cochrane's image on Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Playstation_DualSense_Controller.png), used under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/). Aftertouch is an independent project and is not affiliated with Sony Interactive Entertainment.
