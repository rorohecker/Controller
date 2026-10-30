# Platform integrations (0.22.0)

## Game audio capture

On Windows build 20348 or newer, **Haptics → Audio capture source → Selected app** captures the selected PID and its child processes. Refresh lists running executables, not only processes currently making sound. Capture is silent if the selected process has no render stream. PID selections are temporary and are not exported in profiles.

The bridge gain/preset controls keep the active source. Changing the selector alone does not switch a running capture; choose **Use selected source**. Process exit, unavailable devices and capture failures stop that source. They never silently fall back to system capture. System audio remains an explicit choice on older Windows versions or when process capture is unavailable.

Profiles can select **Audio haptics source → Matched game process** for a managed session. The app resolves the profile's executable to one current PID when the session starts. Missing/ambiguous matches stop session setup with an explanation. It does not guess a launcher or capture Steam's entire process tree. Existing session restoration still restores the previously requested source on exit, including System audio if that was the prior setting.

The WASAPI worker uses a bounded grip queue, handles silent/null buffers, holds the original process handle, checks process termination, and stops cooperatively. A generation check rejects late callbacks/errors from replaced sources. Process-client activation runs off the HID loop and times out after roughly four seconds. Initial USB output still uses the existing startup path. At most two process-capture workers may exist while startup/stop overlaps; a stalled OS activation cannot create unlimited workers. Capture errors leave file playback intact. No microphone or audio-file recording is involved.

## Desktop access

**PC controls → Desktop access → Test desktop access** stops this app's effects/mappings, then opens the native input backend without sending pointer/key events. macOS checks Accessibility, event-posting and Input Monitoring separately. Linux reports X11/Wayland/headless environment information without treating environment variables as a permission grant.

Wayland backend/portal setup may prompt. A successful connection is not proof that every native/XWayland/elevated target accepts input. The app does not grant itself OS permissions. Runtime macOS revocation and native input errors invalidate the cached backend and disable active input mappings; test/re-enable after resolving access. Physical macOS/Linux permission behavior remains unverified here.

## Native game feedback: prototype boundary

The bundled ViGEmBus provides Xbox 360/DS4 targets; this app does not integrate a virtual DualSense HID/audio driver. Creating a native virtual controller requires both the expected HID interface and the audio endpoints used by a game. Audio-derived effects do not identify weapons or reconstruct native trigger commands.

### External vDS provider

Current upstream [vDS](https://github.com/hurryman2212/vds) documents a concrete native Bluetooth-to-USB bridge on both Windows and Linux. Review snapshot: `ec531599dfdd02e14ce68efd8c0d2dcb439894a5` (MIT), checked 2026-10-30. Its [Windows guide](https://github.com/hurryman2212/vds/blob/ec531599dfdd02e14ce68efd8c0d2dcb439894a5/README-WINDOWS.md) uses a userspace vDS service, usbip-win2, HidHide and Opus. Its [Linux guide](https://github.com/hurryman2212/vds/blob/ec531599dfdd02e14ce68efd8c0d2dcb439894a5/README-LINUX.md) requires `vds_hcd` and the userspace daemon. This updates the earlier feasibility finding: there is a documented external Windows route, not an integrated native driver in this app.

**Profiles > Native game feedback** now reports whether the provider tools are present in standard installation folders and copies the platform's upstream setup link. This is read-only file discovery: it does not establish that drivers/services work, execute the tools, attach a controller, or install drivers. Windows USB/IP setup can restart USB hubs, so dependency installation remains an explicit external setup step. No third-party driver/runtime is newly bundled.

With a separately configured provider, release this app's monitoring while vDS owns the physical pad. Native effects belong to the game/provider then; the app's Stop all cannot stop them. Resume monitoring only after handing ownership back. Native forwarding remains experimental until an installed provider, target game and physical controller pass a complete handoff/audio/trigger/recovery test. There is no upstream macOS provider integration here, and provider setup controls are hidden there.

**Profiles → Native game feedback → Release controller to game** stops app mappings/effects and closes this core's HID/audio handles. Automatic reconnect is suspended until **Resume monitoring**. Live input pauses. This lets an already-compatible USB or dongle route be owned by a native game. The mode is session-local; restarting the app/core resumes normal monitoring. Stop all affects this app's sources, not another game's output while released.

The **four-channel relay prototype** captures an existing non-controller render endpoint configured for four channels at 48 kHz. Only channels 3/4 are forwarded to the grip mix, without music conversion/preset boost; the mixer, limiter and personal caps still apply. It rejects controller-named endpoints to avoid feedback loops. It does not create an endpoint, rename one, or install a driver. A separate virtual device/provider must produce the expected source. This is not a complete native-game emulation route, and its physical output is unverified.

Diagnostics includes a read-only USB output-report inspector. It previews standard USB 0x02 to Bluetooth 0x31 framing and rejects unknown trailing extension data. No preview packet is sent. This provides a testable control-packet boundary for a future driver adapter, not a running HID relay.

## Bluetooth haptics: opt-in experiment

**Diagnostics → Wireless haptics lab** runs only after an explicit click. It resets competing app writers and sends a two-second 80 Hz, four-pulse-per-second pattern. Strength is limited to 1–25%, with personal side limits applied. USB uses the same envelope through the existing audio mixer; Bluetooth uses 3 kHz signed stereo samples in 142-byte report-0x32 frames, separate transport/audio counters and seeded CRC. Normal Bluetooth control/rumble behavior is unchanged outside the test.

The scheduler skips missed packets rather than sending catch-up bursts. Stop, Reset, disconnect and native handoff cancel the run; Bluetooth sends silence on normal completion/stop. Partial HID writes and errors end the test. Output-setting changes are rejected while the experiment owns the link. Early stops are not stored as completed comparison runs.

Reported lateness/write durations are software scheduling figures. USB reports audio callback counts/startup timing; it does not claim zero HID or physical latency. Actual end-to-end comparison requires recording a common timing reference and physical grip motion/contact-mic signal over both connections. No physical Bluetooth advanced-haptics or force equivalence has been verified in this iteration. This remains experimental and is not used for normal music/game capture.

## DS5Dongle discovery

DS5Dongle uses Sony's ordinary USB VID/PID/product strings, so those values cannot identify it. On USB connect, the app checks read-only F8 version and F7 schema-5 configuration responses. A matching bounded version/configuration is labeled **DS5Dongle-compatible interface**, not authenticated hardware. Older forks or changed schemas remain unknown; their ordinary USB route is retained. No F6 SET, pairing reset, flash write or firmware update is performed.

Input/trigger/audio capability still depends on the bridge firmware and available audio endpoint. The app has not physically tested a dongle. Avoid competing app and native-game writers; use handoff for native feedback.

## Evidence

- [Microsoft process-loopback sample](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/) — process-tree capture, endpoint independence, silence and minimum build.
- [WASAPI process capture API](https://github.com/HEnquist/wasapi-rs/blob/master/src/api.rs) — existing dependency used for client activation; the app handles raw silent buffers directly.
- [XDG RemoteDesktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html), [Enigo Linux backends](https://github.com/enigo-rs/enigo/blob/main/src/linux/mod.rs), [Apple event-posting preflight](https://developer.apple.com/documentation/coregraphics/cgpreflightposteventaccess()) — runtime access boundaries.
- [ViGEm](https://docs.nefarius.at/projects/ViGEm/) and [vDS](https://github.com/hurryman2212/vds) — current virtual-device options and the distinction between Xbox feedback and a native HID/audio bridge. vDS is researched, not bundled or installed.
- [DS5Dongle, commit 77d378e](https://github.com/awalol/DS5Dongle/tree/77d378effcda11abee87f9be1ac31d6518e87560) — MIT source, USB identity, F7/F8 discovery, 142-byte Bluetooth transport. [Bluetooth haptics experiment, commit 7ed4950](https://github.com/awalol/dualsense-bt-haptics/tree/7ed4950197d342d17f14081188f61d4fbe0faa03) — MIT source, 3 kHz stereo framing/counters. Wire facts were used in original Rust code; third-party driver/firmware code is not embedded.

Tests cover source validation, stale errors, scoped failures, bounded/native stereo DSP, packet CRC/counters, scheduler skips/deadlines, PID session resolution, no implicit fallback, handoff ownership, access states, opt-in experiments and layout. A separate silent Windows audio-service test started/stopped process loopback successfully without opening a controller output. It is excluded from service-free CI. These checks do not establish physical device compatibility or OS permission grants.
