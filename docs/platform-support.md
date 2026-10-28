# Platform builds

Windows is physically tested. The release workflow also publishes macOS and Linux packages after their tests, Cargo check, strict Clippy, and bundling pass. macOS/Linux hardware behavior still needs physical testing. Preview builds remain available from `portable-build.yml`.

| Capability | Windows | macOS preview | Ubuntu/Bazzite preview |
| --- | --- | --- | --- |
| DualSense HID input, lightbar, adaptive triggers, profile editing | Yes | Code path and build check | Code path and build check |
| USB four-channel grip audio and controller speaker | Tested on Windows USB | Needs endpoint test | Needs endpoint test |
| Desktop mouse, keyboard, PC macros, gyro pointer | Yes | Native events; Accessibility permission | Enigo X11 / Wayland / desktop portal; compositor dependent |
| XInput virtual pad and HidHide | Yes, optional drivers | Not applicable | Pending `uinput`/`uhid` bridge |
| Foreground game matching | Yes | Application name via System Events | X11/XWayland via xprop; pure Wayland may need manual selection |
| Screen color | Yes | Pending native API | Pending compositor-specific API |
| System audio capture to grips | Windows WASAPI | Pending CoreAudio | Pending PipeWire |

## Install previews

- Ubuntu: install the `.deb` artifact. It includes the DualSense hidraw `uaccess` rule. Reconnect the controller after install. An AppImage is also built.
- Bazzite desktop: use the AppImage artifact. If hidraw cannot open, install `resources/70-controller-dualsense.rules` into `/etc/udev/rules.d/`, reload udev rules, and reconnect. Bazzite documents AppImage as a portable app format; the AppImage does not install a udev rule.
- macOS: use the universal `.dmg` artifact for Intel or Apple Silicon. This preview is unsigned and not notarized.

For desktop controls on macOS, allow Accessibility access when requested. On Linux, allow the desktop control portal if prompted. Some compositors do not expose the required input protocol. Linux auto matching needs `xprop` (Ubuntu package `x11-utils`) and an X11/XWayland window. For Proton games, the detector reads the foreground process command line for a game executable and translates Z: paths. Other Wine drive mappings may still need a manually chosen match.

No installer can force its HID reports to win over Steam Input or a game's own output reports. The last writer can replace light or trigger effects. Start at sign-in helps the app connect earlier, but does not guarantee effect ownership. For a game's native DualSense features, test with Steam Input disabled for that game. Windows XInput mode uses ViGEmBus and may need HidHide to prevent double input.

## Build

The preview workflow runs Rust tests, check, Clippy, and a Tauri bundle on Ubuntu 22.04 and macOS. Linux packages use `tauri.linux.conf.json`; macOS uses `tauri.macos.conf.json`. The macOS bundle includes Intel and Apple Silicon binaries; CI checks both targets and runs native tests on Apple Silicon. Neither preview has an updater feed yet. Use Diagnostics ? Check platform to inspect Linux hidraw access and desktop requirements. Native input errors are reported rather than ignored.

The interface uses each platform's shortcut names and modifier keys: Ctrl/Windows key on Windows, Command/Option on macOS, and Ctrl/Super on Linux. Unsupported driver, XInput, screen-capture, system-audio-capture, and updater controls are hidden rather than shown as inactive options.

Desktop permission controls show separate macOS Accessibility (sending desktop input) and Input Monitoring (recording system shortcuts) status. The app can request access and open the relevant settings; macOS requires the user to grant it. Native shortcut recording runs only after Record is pressed, for up to ten seconds, and Escape cancels it. Common shortcuts and manual modifier buttons work without recording. A physical Mac is still required to verify the permission flow.

Calibration is stored per controller identity, including motor-zone measurements. A controller address is used internally to derive an opaque local identifier; a connection path is the fallback when the controller cannot provide a stable identity. Legacy shared calibration is assigned once to the first controller connected after migration. Calibration correction currently runs in this app; driver/output-level persistence is tracked in the development backlog.

Native file playback uses the system file picker, Symphonia decoding and Rubato resampling on worker threads on all desktop targets. Supported formats include WAV/AIFF PCM, MP3, FLAC, Ogg/Vorbis, and supported AAC/ALAC tracks in M4A containers. Opus and multichannel surround assets are not supported by this decoder path. Native streams accept mono/stereo audio at 8-192 kHz, up to one hour / 512 MB. Four-channel controller output remains 48 kHz; speaker audio is mono and the rear grip channels preserve stereo. Hardware audio routes still require one USB pad and a compatible four-channel endpoint. Web preview uses its existing limited browser decoder; installed builds select native playback.
