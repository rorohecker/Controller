# Aftertouch

Aftertouch is a Windows tuning workbench for DualSense and DualSense Edge controllers, built in Rust with a Tauri 2 interface. It reads the controller directly, lets you shape adaptive triggers, and shows what the pad is reporting. Hardware output stays in one Rust process so a lightbar change, rumble cue, and trigger preview do not compete with one another inside the app.

## Run

Launch `dist/Aftertouch.exe` or build from source:

```powershell
cargo run --release --bin aftertouch
```

Windows 10 or 11 and WebView2 are required. Connect a controller over USB or Bluetooth. Aftertouch scans automatically, prefers USB if both transports appear, and lets you select another pad in Diagnostics. It does not install a driver or modify a game.

## The workbench

- **Live input:** A photograph of a DualSense lights up at the buttons being pressed. The view also shows trigger travel, stick position, touch contacts, motion readings, battery status, and a recent input trace. Edge buttons appear in the diagnostics matrix when reported.
- **Adaptive triggers:** Edit eleven points that resolve to ten hardware zones for each trigger. Resistance, weapon break, vibration, and off modes are available. The app caps encoded force at a conservative six of eight levels. A manual preview expires after 15 seconds. A game profile can renew the lease while its executable stays in the foreground; PS + Options and Reset stop the effects.
- **Haptic Lab:** Sketch a waveform and play a timed, frequency-shaped envelope on the ordinary rumble motors. The computer plays a quiet audio preview. This is a motor pattern, not USB voice-coil PCM or the controller's advanced haptics audio path.
- **Touch and motion:** One finger can move the pointer, two fingers can scroll, and a touchpad press can click. An inward swipe from either edge can send an arrow key. Optional gyro aim integrates motion by elapsed time and can be gated by a trigger, shoulder button, or touchpad.
- **Profiles and mapping:** Save curves, haptic settings, gyro options, and button-to-key bindings in local JSON. Profiles can match the foreground executable name and apply automatically. Import and export make profiles portable. The optional XInput bridge presents an Xbox 360 pad through a separately installed ViGEmBus driver.
- **Diagnostics:** Inspect report rate, missing full-report sequence counters, rejected input reports, firmware, connected pads, and likely competing controller tools. The drift test measures resting stick offset; the travel test samples twelve directions. Battery history records the last 24 changes locally.
- **Four palettes:** Dusk, Cinder, Tide, and Verdigris keep the same controller-focused layout. The interface respects reduced-motion settings.

The battery charge state comes from the controller's status bytes. The displayed temperature uses a community interpretation of a field the Linux driver leaves reserved. “Condition” is an estimate from that field and reported charge faults; it is not a manufacturer battery-health measurement.

## Device communication

The pure `protocol/` crate parses full USB and Bluetooth input, verifies Bluetooth input CRC32, and builds USB `0x02` or Bluetooth `0x31` control reports with the appropriate flags, sequence, and CRC32. On Bluetooth, the core requests full `0x31` input and can parse the short `0x01` fallback. Output reports are sent when state changes. The core resets effects before releasing a stalled handle, reconnects after unplug, and restarts if its subprocess exits.

The protocol crate also contains an **experimental, test-only** encoder for the separate Bluetooth `0x32` advanced-haptics frame, based on [SAxense's public research](https://github.com/egormanga/SAxense). Aftertouch does not send those frames to hardware. Real Bluetooth voice-coil haptics would need a timed 3 kHz stereo PCM stream, a validated output path on a Bluetooth pad, and a driver-backed virtual audio endpoint if a Windows game must see controller haptics channels. Sony's documented PC game-haptics route remains USB.

The optional XInput bridge requires a user-installed [ViGEmBus](https://github.com/nefarius/ViGEmBus), whose upstream repository is archived. Aftertouch neither bundles nor installs it. Touch mouse, gyro aim, and keyboard remapping use Windows SendInput. A game may see both the physical and virtual pad unless the physical one is hidden by the user's separate setup.

## Verify and build

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python tests/ui_smoke.py
cargo build --bin aftertouch
python tests/core_smoke.py
cargo build --release --bin aftertouch
```

The UI test uses Playwright and a local Chrome installation. The core smoke test needs a connected controller; it enables gyro aim while L2 is held, then disables it, and checks the live report stream. On the available standard DualSense over USB, the latest run read about 236 reports per second with no observed sequence gaps or rejected reports. HIDAPI submission confirms that Windows accepted the report; it is not a physical force or latency measurement. Bluetooth output, Edge hardware, and prolonged hotplug cycling have not been measured on this machine.

Profiles live at `%APPDATA%\Aftertouch\profiles.json`. The UI can be opened in a browser for layout and keyboard-input preview, but device writes require the desktop app.

## Protocol and art credits

Control-report layouts follow the [Linux hid-playstation driver](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-playstation.c), and trigger encoding builds on [Nielk1's public trigger research](https://gist.github.com/Nielk1/6d54cc2c00d2201ccb8c2720ad7538db). The experimental Bluetooth haptics frame credits [egormanga's SAxense research](https://github.com/egormanga/SAxense); no SAxense code is copied into the app. The controller photograph is [Alex Cochrane's Wikimedia image](https://commons.wikimedia.org/wiki/File:Playstation_DualSense_Controller.png), licensed [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).

Aftertouch is independent and is not affiliated with Sony Interactive Entertainment.

Figtree and Spectral are bundled for offline typography under the SIL Open Font License; their license files are in `ui/assets/fonts/`.
