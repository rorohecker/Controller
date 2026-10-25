# Bundled optional XInput driver setup

- File: `ViGEmBus_1.22.0_x64_x86_arm64.exe`
- Source: <https://github.com/nefarius/ViGEmBus/releases/tag/v1.22.0>
- SHA-256: `89220A7865076B342892F98865F3499FB7C4CFD673159E89D352C360FD014C6A`
- Windows Authenticode signer: `Nefarius Software Solutions e.U.`; signature validated before bundling and in Windows release CI.
- License: BSD 3-Clause; see `ViGEmBus-LICENSE.txt`.

This setup is included in the Windows installer as an optional XInput prerequisite. CONTROLLER launches it only after the user presses **Set up XInput driver**. Direct DualSense HID features use the standard Windows HID stack. HidHide is not bundled; the app opens its official download page when requested.
