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
