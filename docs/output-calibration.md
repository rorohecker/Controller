# Saved output calibration (0.22.0)

On Windows, games using **Touch > Game compatibility > XInput bridge** receive the corrected virtual Xbox values. Games reading the physical DualSense directly keep receiving its original reports. ViGEmBus is required for virtual Xbox output; HidHide can prevent duplicate physical input. No controller firmware, registry calibration, or kernel filter is modified.

## Pipeline and persistence

The core loads a controller's native record before processing its first input report. The pipeline is **raw HID > saved center/ranges > measured rest-noise filter > per-game stick curves > XInput integer encoding**. Trigger ranges and release noise are corrected before their conversion to 0–255 XInput values. Each axis is rescaled around its own center; filtering retains full travel at the captured endpoints. The touchpad range remains available to the app's pointer controls; XInput has no touchpad interface.

Guided measurement uses temporary uncorrected readings. The game's projection uses the saved native record independently, so measurement defaults cannot replace the game's saved correction. The incoming report is never mutated by the output projection. The calibration guide rejects a moving rest sample, measures ranges without a timer, then writes validated settings before requesting core reload. A failed save remains visible and can be retried. Cancel is disabled during the save transaction.

Native records live under `CONTROLLER/calibration` in the platform's normal application configuration directory. Filenames use an opaque identity hash, and the record must match that identity. The same controller's USB and Bluetooth connections share settings when its serial/pairing address is available. A path-derived fallback can change across connection paths. Names and game profiles never transfer calibration to another controller.

The browser's existing per-controller calibration is migrated once when no native record exists. Native records take precedence, including a saved bypass choice. Profile export does not transfer hardware calibration. Writes run on the desktop host's blocking pool and use an atomic temporary-file replacement; no per-report disk writes occur. Revisions reject stale saves. Native loads occur on connection or an explicit reload.

## Inspect and recover

**Calibration > Game input correction** plots raw input in pink and the calculated game output in mint, with both trigger values. The panel distinguishes calculated values from an active virtual Xbox output. Per-game curves can introduce a larger deadzone than the measured rest filter. The **Use saved correction** switch is hidden on macOS/Linux because the app currently provides virtual Xbox output on Windows only; the stored ranges still serve the app and desktop controls on all three platforms.

- **Use saved correction** bypasses range/noise correction for virtual game output without deleting it. Per-game curves still apply.
- **Reload saved** rereads the active controller's native record.
- **Restore previous** restores the last valid previous calibration. A missing backup produces an error and leaves current data unchanged.
- **Reset ranges** writes defaults. It also works when the native record is damaged.

The last valid save is kept separately. Recovery preserves damaged/replaced bytes in a bounded extra file rather than overwriting a valid recovery copy with invalid JSON. Identity, schema, finite ranges, file type and size are checked before use. An invalid file falls back to uncalibrated output with an explicit error; it is not silently used or overwritten by migration.

After 250 ms without a valid input report, virtual game output is neutralized once and desktop/legacy key holds are released. Fresh input resumes projection. The existing reconnect logic removes a disconnected/stalled target. A panic-reset report is neutralized before normal game output, so the panic buttons are not forwarded first.

## Verification scope

Rust tests exercise the actual projection and shared XInput encoder, asymmetric ranges, full travel, rest filtering, bypass, unchanged source data, native reload, identity isolation, stale-save rejection, corruption recovery and watchdog timing. Browser tests cover plots, status, save/bypass/restore, damaged-record recovery availability, stale output, reconnect during a save, and window sizes. Core smoke checks disable HID discovery explicitly. These checks do not measure physical drift or certify a game's input path.

No permanent firmware-calibration command is exposed. Factory/Edge writes need a verified model-specific protocol, an original backup and physical recovery validation. The upstream [DualShock calibration tool](https://github.com/dualshock-tools/dualshock-tools.github.io) documents model-specific calibration limits; its [Edge notes](https://github.com/dualshock-tools/dualshock-tools.github.io/blob/main/templates/edge-modal.html) explicitly require hardware intervention for persistent stick-module changes. Software output correction is reversible independently of that firmware work.
