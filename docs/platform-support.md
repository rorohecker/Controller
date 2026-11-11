# Platform builds

Windows USB is the only physically tested configuration, and several newer Windows features are still unverified (see the [hardware test checklist](hardware-test-checklist.md)). The release workflow also publishes macOS and Linux packages after their tests, Cargo check, strict Clippy, and bundling pass. macOS/Linux hardware behavior, permissions and audio endpoints still need physical testing; "implemented" below means the code path exists and builds, not that it has been run on a desktop with a controller. Preview builds are produced by `portable-build.yml` (also run on pull requests).

| Capability | Windows | macOS preview | Ubuntu/Bazzite preview |
| --- | --- | --- | --- |
| DualSense HID input, lightbar, adaptive triggers, profile editing | Yes | Code path and build check | Code path and build check |
| USB four-channel grip audio and controller speaker | Tested on Windows USB | Needs endpoint test | Needs endpoint test |
| Desktop mouse, keyboard, PC macros, gyro pointer | Yes | Native events; Accessibility permission | Enigo X11 / Wayland / desktop portal; compositor dependent |
| Virtual gamepad | Xbox 360 via ViGEmBus; HidHide is manual and optional | Not applicable | Implemented via `/dev/uinput` with calibrated axes/triggers, neutral recovery and rumble routing; needs the `uinput` module and the bundled udev rule (`71-controller-uinput.rules`); unverified on hardware |
| Foreground game matching | Yes | Application name via System Events | X11/XWayland via xprop; pure Wayland may need manual selection |
| Screen color | Yes (GDI, multi-monitor) | Implemented with ScreenCaptureKit; needs Screen Recording permission; unverified | Implemented through the desktop portal/PipeWire with a consent prompt; compositor dependent; unverified |
| System audio capture to grips | Windows WASAPI (plus per-game capture on build 20348+) | Implemented with CoreAudio loopback on macOS 14.6+; unverified | Implemented through PulseAudio/PipeWire-pulse; unverified |
| Background core service | Implemented (Settings); NSIS installer stops it before install/uninstall; not verified with a real installer | Implemented, no installer hook | Implemented, no installer hook |
| Notifications and logging | Yes; rotating logs in the per-user local data folder | Same | Same |

## Install previews

- Ubuntu: install the `.deb` artifact. It includes the DualSense hidraw `uaccess` rule. Reconnect the controller after install. An AppImage is also built.
- Bazzite desktop: use the AppImage artifact. If hidraw cannot open, install `resources/70-controller-dualsense.rules` into `/etc/udev/rules.d/`, reload udev rules, and reconnect. Bazzite documents AppImage as a portable app format; the AppImage does not install a udev rule.
- macOS: use the universal `.dmg` artifact for Intel or Apple Silicon. This preview is unsigned and not notarized.

For desktop controls on macOS, allow Accessibility access when requested. On Linux, allow the desktop control portal if prompted. Some compositors do not expose the required input protocol. Linux auto matching needs `xprop` (Ubuntu package `x11-utils`) and an X11/XWayland window. For Proton games, the detector reads the foreground process command line for a game executable and translates Z: paths. Other Wine drive mappings may still need a manually chosen match.

No installer can force its HID reports to win over Steam Input or a game's own output reports. The last writer can replace light or trigger effects. Start at sign-in helps the app connect earlier, but does not guarantee effect ownership. For a game's native DualSense features, test with Steam Input disabled for that game. Windows XInput mode uses ViGEmBus and may need HidHide to prevent double input.

## Build

The preview workflow runs Rust tests, check, Clippy, and a Tauri bundle on Ubuntu 22.04 and macOS. Linux packages use `tauri.linux.conf.json`; macOS uses `tauri.macos.conf.json`. The macOS bundle includes Intel and Apple Silicon binaries; CI checks both targets and runs native tests on Apple Silicon. The release workflow builds signed updater artifacts and one combined feed for AppImage and macOS universal builds when the signing secret is configured; no such release has been published or installed end to end. Debian packages do not self-update. Use Diagnostics ? Check platform to inspect Linux hidraw access and desktop requirements. Native input errors are reported rather than ignored.

The interface uses each platform's shortcut names and modifier keys: Ctrl/Windows key on Windows, Command/Option on macOS, and Ctrl/Super on Linux. Unsupported driver, XInput, screen-capture, system-audio-capture, per-game audio capture and updater controls are hidden rather than shown as inactive options.

Desktop permission controls show separate macOS Accessibility (sending desktop input) and Input Monitoring (recording system shortcuts) status. The app can request access and open the relevant settings; macOS requires the user to grant it. Native shortcut recording runs only after Record is pressed, for up to ten seconds, and Escape cancels it. Common shortcuts and manual modifier buttons work without recording. A physical Mac is still required to verify the permission flow.

Calibration is stored per controller identity, including motor-zone measurements. A controller address is used internally to derive an opaque local identifier; a connection path is the fallback when the controller cannot provide a stable identity. Legacy shared calibration is assigned once to the first controller connected after migration. Calibration correction currently runs in this app; driver/output-level persistence is tracked in the development backlog.

Native file playback uses the system file picker, Symphonia decoding and Rubato resampling on worker threads on all desktop targets. Supported formats include WAV/AIFF PCM, MP3, FLAC, Ogg/Vorbis, and supported AAC/ALAC tracks in M4A containers. Opus and multichannel surround assets are not supported by this decoder path. Native streams accept mono/stereo audio at 8-192 kHz, up to one hour / 512 MB. Four-channel controller output remains 48 kHz; speaker audio is mono and the rear grip channels preserve stereo. Hardware audio routes still require one USB pad and a compatible four-channel endpoint. Web preview uses its existing limited browser decoder; installed builds select native playback.


## Runtime diagnostics (0.21)

All platforms compile the bounded audio-opening/capture workers, input trace analyzer and software timing checks. Core CPU counters are available on Windows/macOS/Linux; resident memory is currently reported on Windows/Linux. The portable CI also runs the real stdio integration checks in `--no-hid` mode. Physical endpoints and output feel after the scheduling change remain unverified. See [runtime reliability](runtime-reliability.md).

## Linux build dependencies

Ubuntu 22.04 packages needed to build from source (the same list CI uses): `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libasound2-dev libudev-dev libxkbcommon-dev libpipewire-0.3-dev libdbus-1-dev libclang-dev`. Install the Tauri CLI with `cargo install tauri-cli --version 2.11.1 --locked` to produce bundles. For the virtual gamepad, load the `uinput` module and install `resources/71-controller-uinput.rules`; for hidraw access install `resources/70-controller-dualsense.rules` (the `.deb` installs the latter).
