# CONTROLLER

CONTROLLER is a Windows tuning workbench for DualSense and DualSense Edge controllers, built in Rust with a Tauri 2 interface. It reads the controller directly, lets you shape adaptive triggers, and shows what the pad is reporting. Hardware output stays in one Rust process so a lightbar change, rumble cue, and trigger preview do not compete with one another inside the app.

## Run

Launch `dist/CONTROLLER.exe` or build from source:

```powershell
cargo run --release --bin controller
```

Windows 10 or 11 and WebView2 are required. Connect a controller over USB or Bluetooth. CONTROLLER scans automatically, prefers USB if both transports appear, and lets you select another pad in Diagnostics. It does not install a driver or modify a game.

You can also work without a controller: edit and export profiles, design trigger curves, preview haptic sketches through the PC speakers, and receive local telemetry to test effect rules. To try a rule without a game, select **Local JSON adapter**, start listening, arm the rule, and select **Send test lap**. Hardware output controls become available when a pad connects. The Input page accepts J, K, L, and I as a face-button preview while disconnected.

## Updates and releases

The installed Windows app checks for updates shortly after launch and every twelve hours while open. Diagnostics has **Check now** and shows download progress when an update is available. Installation uses Tauri's signature verification and the Windows NSIS installer; it closes and restarts the app. The standalone `dist/CONTROLLER.exe` remains a portable build and does not use the installer update path. The latest signed GitHub release provides the update feed.

Release builds use the private updater key generated at `%APPDATA%\CONTROLLER\updater.key`. Keep a secure backup of this key: later releases must use the same key. Its contents are configured as the repository's `TAURI_SIGNING_PRIVATE_KEY` Actions secret. The workflow in `.github/workflows/release.yml` runs on a `v<version>` tag, checks that the tag matches `tauri.conf.json`, verifies Rust and browser tests, builds a signed NSIS installer, and publishes the installer, signature, and update manifest. For a local installer build, set `TAURI_SIGNING_PRIVATE_KEY` to that key's path in the PowerShell session and run `cargo tauri build --bundles nsis`.

See [Compatibility and test record](COMPATIBILITY.md) for the controller, transport, and game combinations actually checked.

## The workbench

- **Live input and PC controls:** A front-facing DualSense vector map lights up each reported button and moves both stick caps, trigger bars, and touch contacts with live input. The activity trace includes both sticks. Game mode is the default, so controller presses only show on the map and remain available to games. PC control is opt-in: when this app is focused, the pad navigates its pages; in other windows, one finger on the touchpad moves the pointer, the right stick scrolls, and L2/R2 click. Right-stick down scrolls down by default, and touchpad sensitivity is adjustable. The PC controls page lets you assign keys, taps, and short key sequences to the other buttons and tune the click point. The controller remains visible to games in both modes. Edge buttons appear in the diagnostics matrix when reported.
- **Adaptive triggers:** L2 and R2 each have a visible ten-zone graph, stacked for comparison. Drag each zone or use the keyboard to adjust its force independently. Resistance, weapon break, vibration, and off modes are available. Experimental Bow mode and Weapon mode also have explicit resistance start and break zones; Bow has a separate snap force. Firmware support may vary. The app caps encoded force at a conservative six of eight levels. A manual preview expires after 15 seconds. A game profile can renew the lease while its executable stays in the foreground. An optional full-pull click briefly changes R2 when it crosses a profile-defined threshold, then restores the profile's trigger effect. It rearms only after R2 is released. PS + Options and Reset stop the effects.
- **Haptic bench:** Sketch a waveform, adjust its envelope and left/right balance, and play it through the USB controller's four-channel audio endpoint. The app leaves the front speaker channels silent and sends bounded PCM to the two grip channels. When that endpoint is unavailable, including over Bluetooth, the preview uses ordinary motor rumble. An audible PC sketch plays while disconnected and can be enabled separately during controller previews.
- **System audio bridge:** The Haptic page can capture the Windows default output through WASAPI loopback and turn bass and sharp transients into stereo grip motion. There are five feel presets and an intensity control. The bridge is USB-only and stops on Reset, PS + Options, disconnect, or a fatal audio stream error. Recoverable buffer glitches are counted in the bridge status while playback continues. It refuses to capture the controller's own speaker endpoint to avoid feedback. The audio stays local; the app does not upload or record it.
- **Haptic alerts:** The Haptic page can ping a connected pad, arm one low-battery cue, or start a timer up to 30 minutes from the interface. Low-battery alerts are opt-in, fire once at or below 20% while discharging, and rearm after charging or recovery to 30%. The core owns the timer and plays its cue even if the workbench window is in the background; Reset or disconnect clears it.
- **Touch and motion:** One finger can move the pointer, two fingers can scroll vertically, and a touchpad press can click. With gestures enabled, rotating two fingers also scrolls by angle; touch IDs that jump too far between reports are ignored. An inward swipe from either edge can send an arrow key. Entering the side or top touch zone gives a brief, rate-limited grip tick. Optional gyro aim integrates motion by elapsed time and can be gated by a trigger, shoulder button, or touchpad. A two-second still calibration measures gyro bias and saves it with the active profile; movement during sampling is rejected.
- **Forza Data Out:** The Lightbar page can listen on a chosen UDP port for Forza Motorsport Dash or Forza Horizon 4/5 Dash packets. RPM drives a stepped rev color; gear changes, braking slip, general tire slip, and rumble-strip contact produce short, rate-limited grip cues. Speed, gear, RPM, and an estimated braking slip indicator appear in the app. Forza Motorsport 2023 can send to `127.0.0.1`; older Forza titles need **Allow LAN sender** and this PC's IPv4 address as the game's destination. LAN mode accepts packets from private or link-local addresses only. Manual lightbar and battery color controls take ownership when chosen.
- **Local telemetry adapter:** The same listener can accept bounded JSON datagrams from another game tool or a small script. Select **Local JSON adapter** on the Lightbar page. Revs, speed, gear, braking, tyre slip, and kerb contact use the same conservative light and haptic cues as Forza. The adapter accepts a fixed schema and rejects unknown effect commands; it does not execute code from packets. It listens on loopback by default. The optional LAN setting only accepts private, link-local, or loopback senders.
- **Local effect rules:** The Lightbar page has an opt-in Rhai editor for custom telemetry responses. A rule reads validated racing signals and may return an RGB light color and one of three short grip cues. The core evaluates it at most ten times per second, bounds source and operations, and stops it after a runtime error. The editor remembers text locally; rules are never armed automatically after opening the app.
- **Screen color:** An opt-in mode samples fifteen points on the primary Windows display, favors colored areas when mixing them, and eases the controller lightbar toward the result. Sampling runs in a background worker at about 2.5 updates per second, so it does not block HID input. No image is saved or transmitted. Choosing a fixed color, battery color, or telemetry stops screen sampling. Reset and disconnect stop it too.
- **Sensor color:** Another opt-in lightbar mode maps a raw, uncalibrated byte from full input reports across cool blue, amber, and warm red. Its meaning and units are unverified; the color is a visual experiment, not a temperature warning. If a short Bluetooth report omits the byte, the mode waits for a full report.
- **Profiles and mapping:** Save ten-zone trigger forces, haptic preview settings, gyro bias, full-pull click settings, and button-to-key bindings in local JSON. A binding can hold a key, tap it once, or send up to six keys in a paced sequence. Add a layer button to make a chord; a layer binding takes priority over the base binding for that button and temporarily suppresses the layer button's own base action. The MIC button can be mapped to a push-to-talk key. Keyboard remapping pauses while the workbench has focus so controller navigation does not type into its own controls. Profiles can match the foreground executable name, with optional window-title text and an exact executable path to distinguish games that share a launcher name. The app reads those details through standard Windows process and window APIs, then applies the best matching profile automatically. Import and export make profiles portable. The Profiles page includes editable Hanzo, Genji, and Soldier: 76 weapon feel templates. They use static trigger settings and optional controller-input cues; live ability, hit, and shot telemetry would require a separate game event source. The optional XInput bridge presents an Xbox 360 pad through ViGEmBus.
- **Lightbar and battery:** Pick a color from four named palettes, enter a custom hex value, or save favorites locally. The front-view DualSense preview shows the selected RGB color diffused through the touchpad seam and nearby shell. The five lower white player indicators have separate on/off controls. Actual color depends on the controller and room light. The Input page shows battery history and an estimated charging rate after enough coarse level changes; raw sensor values and report counters stay in Diagnostics.
- **Calibration and diagnostics:** The untimed Calibration guide measures resting stick and trigger values, full stick travel, trigger pulls, and touchpad edges. It shows moving stick dots, accumulated travel, reported trigger bytes with peaks captured on release, and touchpad corner markers. The user chooses when to advance; accepted ranges are saved and applied to live input and pointer control. Each trigger occupies one byte (0–255) in the DualSense input report. Travel after the reported value reaches 255 cannot be measured by the app. Diagnostics shows report rate, missing full-report sequence counters, rejected reports, firmware, connected pads, and likely competing controller tools. **Copy diagnostics** puts a shareable summary on the clipboard without the device path. The drift test measures resting stick offset; the travel test samples twelve directions. Battery history records the last 24 changes locally.
- **Interface:** The control desk uses one theme with optional reduced motion through Windows accessibility settings.

In PC control, the focused desktop window supports controller navigation. L1 and R1 change sections, the D-pad moves between controls and adjusts sliders or menus, Cross selects, and Circle returns to Input. In the trigger editor, select a curve point with D-pad left or right, then adjust its force with up or down. This navigation only reacts while the window has focus, so using the pad in another app does not change the workbench.

### Local telemetry packet

Send one UTF-8 JSON object per UDP datagram to the configured port. `rpm` and `maxRpm` are required; the remaining fields are optional. A packet must be at most 512 bytes. `kerb` lists front left, front right, rear left, and rear right contact. `brake` uses 0–255, while `slip` and `frontSlip` use 0–10. Speed is in km/h. Values outside the documented bounds and fields that try to specify a direct effect are rejected.

```json
{"rpm":6100,"maxRpm":8000,"speedKmh":108,"gear":4,"brake":150,"slip":1.4,"frontSlip":1.3,"kerb":[true,false,false,false]}
```

Run `python examples/send_telemetry.py` for a twelve-second local preview. Game adapters should send at 10–30 Hz; the UI updates at no more than 10 Hz, and grip cues have separate rate limits.

The battery level and charge state come from the controller's status bytes. Diagnostics shows charge faults from that status field and the separate raw sensor byte. The controller does not report battery health or a verified temperature value through this input report.

### Local effect rule

Start a Forza or Local JSON telemetry listener, then open **Telemetry rules** on the Lightbar page. Edit a rule and select **Arm rule**. The editor becomes read-only while armed; disarm it to make changes. The result must be a Rhai map. Return an empty map to keep the normal telemetry effects for that sample. Omit `light` or `cue` to leave just that effect under the normal telemetry behavior.

```rhai
if rpm / max_rpm > 0.8 {
    #{ light: [220, 40, 20], cue: "pulse" }
} else {
    #{ light: [40, 145, 130] }
}
```

Available inputs are `rpm`, `max_rpm`, `speed_kmh`, `gear`, `brake`, `slip`, `front_slip`, `kerb_left`, and `kerb_right`. Missing optional numeric values appear as zero, except an unknown gear is `-1`; missing kerb flags are `false`. `light` needs three integers from 0 to 255. `cue` accepts `"pulse"`, `"grit"`, or `"shift"`; repeated scripted cues are limited to one every 500 ms. A rule is at most 4096 UTF-8 bytes and each evaluation is capped at 2000 Rhai operations. The engine exposes no file, network, keyboard, or device API. Compile errors are shown before arming; a rule that fails while running is turned off. Reset and PS + Options turn it off too. Script text is stored only in this app's local browser storage, separate from exported profiles. A rule must be armed again after reopening the app.

### Controller speaker chirp

The Haptic page has a separate **Speaker chirp** button. It plays a short, quiet rising tone through the first two channels of the DualSense USB audio endpoint, so you can find a pad within hearing distance. It requires one connected USB pad with its four-channel Windows audio device enabled. The grip haptic channels continue to work independently; ordinary haptics and the system audio bridge leave the speaker channels silent.

### Screen color

On the Lightbar page, select **Follow screen color** to sample the primary display and gently update the controller lightbar. The app reads individual pixels through the Windows desktop device context in a background worker; it does not capture a full image. This is a local, opt-in effect. It samples the visible desktop, so move or minimize the workbench if you want another window's color to dominate. Protected content may return no usable pixels; the mode turns itself off and shows an error if sampling fails. Multi-monitor setups currently use the primary display only.

**Sensor color** is a separate Lightbar control. It maps the uncalibrated raw byte in full input reports to a color and displays the last raw value beside the control. The Diagnostics reading and this color do not establish battery temperature or health.

## Device communication

The pure `protocol/` crate parses full USB and Bluetooth input, verifies Bluetooth input CRC32, and builds USB `0x02` or Bluetooth `0x31` control reports with the appropriate flags, sequence, and CRC32. On Bluetooth, the core requests full `0x31` input and can parse the short `0x01` fallback. Output reports are sent when state changes. The core resets effects before releasing a stalled handle, reconnects after unplug, and restarts if its subprocess exits. Audio streaming uses a separate Windows audio callback, so the HID input loop does not have to synthesize PCM. The output mixer bounds grip sample amplitude and average duty and limits its capture queue to 20 ms. The front speaker channels carry only an explicitly requested chirp.

The protocol crate also contains an **experimental, test-only** encoder for the separate Bluetooth `0x32` advanced-haptics frame, based on [SAxense's public research](https://github.com/egormanga/SAxense). CONTROLLER does not send those frames to hardware. Real Bluetooth voice-coil haptics would need a timed 3 kHz stereo PCM stream, a validated output path on a Bluetooth pad, and a driver-backed virtual audio endpoint if a Windows game must see controller haptics channels. The USB PCM path is implemented; Bluetooth voice-coil output is still unverified.

The optional XInput bridge requires [ViGEmBus](https://github.com/nefarius/ViGEmBus), whose upstream repository is archived. The Windows installer bundles the signed v1.22.0 setup and offers a **Set up XInput driver** button; it launches only when chosen. The portable executable opens the official release page instead. Direct HID input, adaptive triggers, haptics, lights, touch mouse, gyro aim, and keyboard remapping do not need ViGEmBus. The Windows installer embeds the WebView2 bootstrapper. A game may see both the physical and virtual pad; the app also links to optional [HidHide](https://github.com/nefarius/HidHide/releases) for users who need to hide the physical pad. Driver provenance and the pinned hash are in [resources/DRIVER-SOURCES.md](resources/DRIVER-SOURCES.md).

## Verify and build

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python tests/ui_smoke.py
python tests/update_manifest_smoke.py
cargo build --bin controller
python tests/script_smoke.py
python tests/core_smoke.py
python tests/feature_smoke.py
cargo run --bin audio_probe -- --smoke-silent
cargo run --bin audio_probe -- --smoke-loopback
cargo build --release --bin controller
```

The two connected-controller smoke scripts also accept an executable path, for example `python tests/feature_smoke.py dist/CONTROLLER.exe`, to verify a portable build.

The UI test uses Playwright and a local Chrome installation. The core smoke test needs a connected controller; it enables and disables gyro aim, sends a low-force trigger command, and checks the live report stream. The feature smoke test uses a connected USB pad to exercise lightbar modes, open the silent PCM and WASAPI streams, arm and clear the input click, and decode synthetic Forza and local JSON telemetry. On the available standard DualSense over USB, recent core runs read 231–237 reports per second with no observed sequence gaps or rejected reports, and the audio device requested PCM frames. These checks establish that Windows accepted the streams and reports; they do not measure physical haptic force or end-to-end latency. Bluetooth haptic output, Edge hardware, and prolonged hotplug cycling have not been measured on this machine.

Profiles use schema v3 at `%APPDATA%\CONTROLLER\profiles.json`. On first launch after the rename, the app reads an existing `%APPDATA%\Aftertouch\profiles.json`, copies the validated archive into the new folder, and leaves the original file in place. Saved lightbar colors, scripts, and battery history keep their existing local storage keys so those preferences survive the rename. The UI can be opened in a browser for layout and keyboard-input preview, but device writes require the desktop app.

## Protocol and art credits

Control-report layouts follow the [Linux hid-playstation driver](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-playstation.c), and trigger encoding builds on [Nielk1's public trigger research](https://gist.github.com/Nielk1/6d54cc2c00d2201ccb8c2720ad7538db). The experimental Bow mode follows the public [trigger mode mapping](https://github.com/rafaelvaloto/Dualsense-Multiplatform/wiki/%F0%9F%93%84Technical-Documentation:-Trigger-Effect-Mapping). The experimental Bluetooth haptics frame credits [egormanga's SAxense research](https://github.com/egormanga/SAxense); no SAxense code is copied into the app. The controller base, stick cap, and control highlight images come from [Al. Lopez's Gamepad Asset Pack](https://github.com/AL2009man/Gamepad-Asset-Pack) under the MIT license; the license is in `resources/DualSense-artwork-LICENSE.txt`. Live motion and light overlays are drawn by this app.

The system audio bridge uses the [Windows WASAPI loopback API](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording) through [CPAL](https://github.com/RustAudio/cpal). Forza Dash packet offsets were checked against the [forza-telemetry decoder](https://github.com/0x20F/forza-telemetry); no code from that project is included.

CONTROLLER is independent and is not affiliated with Sony Interactive Entertainment.

Geist Sans, Mono, and Pixel are bundled for offline typography under the SIL Open Font License. The license is in `ui/assets/fonts/Geist-OFL.txt`. The workbench's observatory backdrop was generated for this app; it is separate from the DualSense control artwork.
