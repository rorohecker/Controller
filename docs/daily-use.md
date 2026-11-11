# Daily use and presentation (0.18.0)

The bar below the session controls separates profile storage from controller output. **Saved** means the current editor matches the last successful archive write. **Unsaved changes**, **Saving**, **Save failed**, and **Load failed** describe local storage; failed saves retain the edits. Expand **Changes and feedback** for the changed sections, source list and profile actions.

**Applied** requires a native FIFO checkpoint after the profile commands finish. Errors before that checkpoint prevent confirmation. Missing/stale confirmations, controller changes, manual writes during an apply, and Reset cannot mark a profile applied. Applications are queued and use an immutable snapshot. An unrelated native error during that interval conservatively rejects confirmation. Applied confirms command processing, not measured force or in-game compatibility.

The applied comparison covers triggers/input effects, gyro/control settings, keyboard remaps and game-action/link settings. Haptic preview parameters and profile notes are saved but do not play simply because a profile was applied. Separate control writes mark the full profile state as changed separately. Active leases and source reports show ongoing output independently of the saved/applied snapshot.

**Revert unsaved** restores the last successful saved profile in the editor; it is unavailable during an archive write. **Restore applied settings** restores that profile's last confirmed hardware settings in the editor while retaining notes and haptic preview parameters. Neither action writes hardware. Use Save or Apply afterward as needed.

## Feedback and Stop all

The source inspector combines native trigger/motor/lightbar ownership with USB audio mixer activity. It distinguishes running music, cue/timeline feedback, Xbox rumble, system audio and armed rules waiting for input. Reports expire after four seconds rather than remaining active forever. These are software ownership reports, not force or acoustic measurements.

**Stop all** is available on each page and floats at the lower right when its bar scrolls out of view. The shortcut is Ctrl+Shift+Backspace on Windows/Linux and Command+Shift+Backspace on macOS. It invalidates queued session work and music playback before requesting the core Reset, which stops output and input mappings. Automatic profile reapplication remains suppressed until a deliberate Apply. The controller's existing PS+Options emergency reset also remains available. Reset/stop does not erase profiles or personal comfort/calibration data.

## Layout and accessibility

Settings adds **Comfortable/Compact** spacing and a toggle for numeric slider controls. Numeric values follow the original sliders, enforce their limits/steps, and use the same handlers. Reset buttons return to the control's built-in default, within its current valid range. The song timeline retains its own transport and elapsed-time controls.

Local display preferences survive restarts and stay outside shared game profiles. Reduced decorative motion respects both the existing app setting and the system preference, including pointer-focus scrolling and new status animations. Keyboard focus has a clear outline. Functional status colors retain text labels, and applied/action/stop announcements use a status region. Approved pixel art appears in panel headers; the sidebar and controller artwork are unchanged.

Automated checks cover all ten tabs in both densities at effective 100%, 125%, 150% and 200% OS scaling, plus five window widths, loading/failed/disconnected states and denied macOS permission UI. These browser checks do not verify native permission prompts or physical controllers on macOS/Linux.

## Background core, tray, logs and notifications (post-0.22.0)

These were added after 0.22.0 and are tested in software only; real installer, update and hardware behavior is unverified (see the [hardware test checklist](hardware-test-checklist.md)).

- **Background core (Settings):** an opt-in per-user service (the same executable started with `--core-service`) can keep controller settings applied while the window is closed. The window connects over authenticated loopback JSONL with a small client limit and bounded queues; remembered output settings are replayed when a controller reconnects. `--stop-background-core` asks it to exit and waits. The Windows installer stops it before installing or uninstalling (`installer-hooks.nsh`). Turning the setting off makes the app stop the service when it quits.
- **Start minimized and tray:** the app can start hidden in the tray. The first time Close hides the app, a one-off notification and message say it is still running. The tray menu reopens or quits the app; a tray profile switcher is not implemented.
- **Logs and diagnostics:** the desktop shell writes `controller.log` and the background core `core-service.log` under `CONTROLLER/logs` in the per-user local data folder, each rotated at 1 MiB with three older copies. **Export diagnostics** (`export_diagnostics`) builds a path-redacted report (settings strings redacted, platform probe, recent log lines) that can be shown or saved to Downloads.
- **Low-battery alert:** warning and critical percentages are adjustable (`setLowBatteryThresholds`). The sidebar shows a battery chip when the controller reports one.
- **Profile recovery:** `recover_profiles` restores the newest valid backup over an unreadable profile archive (the damaged file is kept aside, and saves stay blocked until you recover or fix it); a damaged `settings.json` is replaced by defaults with the original kept as `.bak`.
- **Steam Input check:** Profiles can ask whether Steam Input is likely to interfere (`steam_input_status`). Telemetry can be limited to one LAN sender address (`setTelemetryAllowedSource`).
- **Hardware-visible controls (all unverified on hardware):** extra adaptive trigger modes (feedback, weapon, vibration, galloping, machine), microphone LED off/on/pulse, headphone/speaker/microphone volumes, lightbar and player-LED brightness, and software lightbar animations (breathe, pulse, rainbow, low-battery blink). **Per-profile lightbar and audio:** a profile can carry an optional lightbar/player-LED state and a reference to a saved audio preset; both are applied with the profile and validated on import. Output state the user set explicitly is remembered per controller id and re-applied after a reconnect or transport change. Trigger, light and audio commands are built from community report documentation, not confirmed on a controller; if one does nothing or misbehaves, use Reset and record it in the checklist.
