# Platform builds

The Windows installer remains the fully tested release. macOS and Linux builds are preview artifacts from `portable-build.yml`; they require hardware testing before release. The 27 October 2026 preview workflow [built a `.dmg`, `.deb`, and `.AppImage` successfully](https://github.com/rorohecker/Controller/actions/runs/40520842226). Use the newest successful workflow for subsequent playback and trigger tuning changes.

| Capability | Windows | macOS preview | Ubuntu/Bazzite preview |
| --- | --- | --- | --- |
| DualSense HID input, lightbar, adaptive triggers, profile editing | Yes | Code path and build check | Code path and build check |
| USB four-channel grip audio and controller speaker | Tested on Windows USB | Needs endpoint test | Needs endpoint test |
| Desktop mouse, keyboard, PC macros, gyro pointer | Yes | Pending native input backend | Pending Wayland/X11 input backend |
| XInput virtual pad and HidHide | Yes, optional drivers | Not applicable | Pending `uinput`/`uhid` bridge |
| Foreground game matching and screen color | Yes | Pending native APIs | Pending compositor-specific APIs |
| System audio capture to grips | Windows WASAPI | Pending CoreAudio | Pending PipeWire |

## Install previews

- Ubuntu: install the `.deb` artifact. It includes the DualSense hidraw `uaccess` rule. Reconnect the controller after install. An AppImage is also built.
- Bazzite desktop: use the AppImage artifact. If hidraw cannot open, install `resources/70-controller-dualsense.rules` into `/etc/udev/rules.d/`, reload udev rules, and reconnect. Bazzite documents AppImage as a portable app format; the AppImage does not install a udev rule.
- macOS: use the `.dmg` artifact. This preview is unsigned and not notarized.

No installer can force its HID reports to win over Steam Input or a game's own output reports. The last writer can replace light or trigger effects. Start at sign-in helps the app connect earlier, but does not guarantee effect ownership. For a game's native DualSense features, test with Steam Input disabled for that game. Windows XInput mode uses ViGEmBus and may need HidHide to prevent double input.

## Build

The preview workflow runs Rust tests, check, Clippy, and a Tauri bundle on Ubuntu 22.04 and macOS. Linux packages use `tauri.linux.conf.json`; macOS uses `tauri.macos.conf.json`. Neither preview has an updater feed yet.
