# Aftertouch

Aftertouch is a Windows tuning workbench for DualSense and DualSense Edge controllers, built in Rust with a Tauri 2 interface. It reads the controller directly, lets you shape adaptive triggers, and shows what the pad is reporting. Hardware output stays in one Rust process so a lightbar change, rumble cue, and trigger preview do not compete with one another inside the app.

## Run

Launch `dist/Aftertouch.exe` or build from source:

```powershell
cargo run --release --bin aftertouch
```

Windows 10 or 11 and WebView2 are required. Connect a controller over USB or Bluetooth. Aftertouch scans automatically, prefers USB if both transports appear, and lets you select another pad in Diagnostics. It does not install a driver or modify a game.

See [Compatibility and test record](COMPATIBILITY.md) for the controller, transport, and game combinations actually checked.

## The workbench

- **Live input:** A photograph of a DualSense lights up at the buttons being pressed. The view also shows trigger travel, stick position, touch contacts, motion readings, battery status, and a recent input trace. Edge buttons appear in the diagnostics matrix when reported.
- **Adaptive triggers:** Edit eleven points that resolve to ten hardware zones for each trigger. Resistance, weapon break, vibration, and off modes are available. An experimental Bow mode uses the authored curve to choose its start and peak; firmware support may vary. The app caps encoded force at a conservative six of eight levels. A manual preview expires after 15 seconds. A game profile can renew the lease while its executable stays in the foreground. An optional full-pull click briefly changes R2 when it crosses a profile-defined threshold, then restores the profile's trigger effect. It rearms only after R2 is released. PS + Options and Reset stop the effects.
- **Haptic bench:** Sketch a waveform, adjust its envelope and left/right balance, and play it through the USB controller's four-channel audio endpoint. The app leaves the front speaker channels silent and sends bounded PCM to the two grip channels. When that endpoint is unavailable, including over Bluetooth, the preview uses ordinary motor rumble. A quiet sketch also plays through the computer's default speakers.
- **System audio bridge:** The Haptic page can capture the Windows default output through WASAPI loopback and turn bass and sharp transients into stereo grip motion. There are five feel presets and an intensity control. The bridge is USB-only and stops on Reset, PS + Options, disconnect, or an audio stream error. It refuses to capture the controller's own speaker endpoint to avoid feedback. The audio stays local; the app does not upload or record it.
- **Haptic alerts:** The Haptic page can ping a connected pad, arm one low-battery cue, or start a timer up to 30 minutes from the interface. Low-battery alerts are opt-in, fire once at or below 20% while discharging, and rearm after charging or recovery to 30%. The core owns the timer and plays its cue even if the workbench window is in the background; Reset or disconnect clears it.
- **Touch and motion:** One finger can move the pointer, two fingers can scroll, and a touchpad press can click. An inward swipe from either edge can send an arrow key. With edge gestures enabled, entering the side or top touch zone also gives a brief, rate-limited grip tick. Optional gyro aim integrates motion by elapsed time and can be gated by a trigger, shoulder button, or touchpad. A two-second still calibration measures gyro bias and saves it with the active profile; movement during sampling is rejected.
- **Forza Data Out:** The Lightbar page can listen on a chosen UDP port for Forza Motorsport Dash or Forza Horizon 4/5 Dash packets. RPM drives a stepped rev color; gear changes, braking slip, general tire slip, and rumble-strip contact produce short, rate-limited grip cues. Speed, gear, RPM, and an estimated braking slip indicator appear in the app. Forza Motorsport 2023 can send to `127.0.0.1`; older Forza titles need **Allow LAN sender** and this PC's IPv4 address as the game's destination. LAN mode accepts packets from private or link-local addresses only. Manual lightbar and battery color controls take ownership when chosen.
- **Profiles and mapping:** Save curves, haptic settings, gyro bias, full-pull click settings, and button-to-key bindings in local JSON. A binding can hold a key, tap it once, or send up to six keys in a paced sequence. Add a layer button to make a chord; a layer binding takes priority over the base binding for that button and temporarily suppresses the layer button's own base action. The MIC button can be mapped to a push-to-talk key. Keyboard remapping pauses while the workbench has focus so controller navigation does not type into its own controls. Profiles can match the foreground executable name, with optional window-title text and an exact executable path to distinguish games that share a launcher name. The app reads those details through standard Windows process and window APIs, then applies the best matching profile automatically. Import and export make profiles portable. The optional XInput bridge presents an Xbox 360 pad through a separately installed ViGEmBus driver.
- **Diagnostics:** Inspect report rate, missing full-report sequence counters, rejected input reports, firmware, connected pads, and likely competing controller tools. The drift test measures resting stick offset; the travel test samples twelve directions. Battery history records the last 24 changes locally.
- **Four palettes:** Dusk, Cinder, Tide, and Verdigris keep the same controller-focused layout. The interface respects reduced-motion settings.

The focused desktop window also supports controller navigation. L1 and R1 change sections, the D-pad moves between controls and adjusts sliders or menus, Cross selects, and Circle returns to Input. In the trigger editor, select a curve point with D-pad left or right, then adjust its force with up or down. This navigation only reacts while the window has focus, so using the pad in another app does not change the workbench.

The battery charge state comes from the controller's status bytes. The displayed temperature uses a community interpretation of a field the Linux driver leaves reserved. “Condition” is an estimate from that field and reported charge faults; it is not a manufacturer battery-health measurement.

## Device communication

The pure `protocol/` crate parses full USB and Bluetooth input, verifies Bluetooth input CRC32, and builds USB `0x02` or Bluetooth `0x31` control reports with the appropriate flags, sequence, and CRC32. On Bluetooth, the core requests full `0x31` input and can parse the short `0x01` fallback. Output reports are sent when state changes. The core resets effects before releasing a stalled handle, reconnects after unplug, and restarts if its subprocess exits. Audio streaming uses a separate Windows audio callback, so the HID input loop does not have to synthesize PCM. The output mixer bounds sample amplitude and average duty, limits its capture queue to 20 ms, and writes silence to the controller's front speaker channels.

The protocol crate also contains an **experimental, test-only** encoder for the separate Bluetooth `0x32` advanced-haptics frame, based on [SAxense's public research](https://github.com/egormanga/SAxense). Aftertouch does not send those frames to hardware. Real Bluetooth voice-coil haptics would need a timed 3 kHz stereo PCM stream, a validated output path on a Bluetooth pad, and a driver-backed virtual audio endpoint if a Windows game must see controller haptics channels. The USB PCM path is implemented; Bluetooth voice-coil output is still unverified.

The optional XInput bridge requires a user-installed [ViGEmBus](https://github.com/nefarius/ViGEmBus), whose upstream repository is archived. Aftertouch neither bundles nor installs it. Touch mouse, gyro aim, and keyboard remapping use Windows SendInput. A game may see both the physical and virtual pad unless the physical one is hidden by the user's separate setup.

## Verify and build

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python tests/ui_smoke.py
cargo build --bin aftertouch
python tests/core_smoke.py
python tests/feature_smoke.py
cargo run --bin audio_probe -- --smoke-silent
cargo run --bin audio_probe -- --smoke-loopback
cargo build --release --bin aftertouch
```

The UI test uses Playwright and a local Chrome installation. The core smoke test needs a connected controller; it enables and disables gyro aim, sends a low-force trigger command, and checks the live report stream. The feature smoke test uses a connected USB pad to open the silent PCM and WASAPI streams, arm and clear the input click, and decode a synthetic Forza UDP packet. On the available standard DualSense over USB, recent core runs read 232–233 reports per second with no observed sequence gaps or rejected reports, and the audio device requested PCM frames. These checks establish that Windows accepted the streams and reports; they do not measure physical haptic force or end-to-end latency. Bluetooth haptic output, Edge hardware, and prolonged hotplug cycling have not been measured on this machine.

Profiles use schema v3 at `%APPDATA%\Aftertouch\profiles.json`; the app migrates older archives when it loads them. The UI can be opened in a browser for layout and keyboard-input preview, but device writes require the desktop app.

## Protocol and art credits

Control-report layouts follow the [Linux hid-playstation driver](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-playstation.c), and trigger encoding builds on [Nielk1's public trigger research](https://gist.github.com/Nielk1/6d54cc2c00d2201ccb8c2720ad7538db). The experimental Bow mode follows the public [trigger mode mapping](https://github.com/rafaelvaloto/Dualsense-Multiplatform/wiki/%F0%9F%93%84Technical-Documentation:-Trigger-Effect-Mapping). The experimental Bluetooth haptics frame credits [egormanga's SAxense research](https://github.com/egormanga/SAxense); no SAxense code is copied into the app. The controller photograph is [Alex Cochrane's Wikimedia image](https://commons.wikimedia.org/wiki/File:Playstation_DualSense_Controller.png), licensed [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).

The system audio bridge uses the [Windows WASAPI loopback API](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording) through [CPAL](https://github.com/RustAudio/cpal). Forza Dash packet offsets were checked against the [forza-telemetry decoder](https://github.com/0x20F/forza-telemetry); no code from that project is included.

Aftertouch is independent and is not affiliated with Sony Interactive Entertainment.

Figtree and Spectral are bundled for offline typography under the SIL Open Font License; their license files are in `ui/assets/fonts/`.
