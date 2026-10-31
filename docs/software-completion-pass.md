# Remaining software work

Baseline: 0.22.0. Hardware testing is deferred until the software pass is ready. Tests and builds do not certify physical support. New experimental output paths stay opt-in; no controller firmware/profile writes are performed by automated checks.

## Batch A — platform backends and boundaries

- [ ] A1. Enforce a local-content CSP; audit IPC, imported content and bounded external helpers.
- [ ] A2. Add calculation/protocol fault and stress checks; fix non-finite, range, timing and cancellation bugs found.
- [ ] A3. Implement Linux virtual gamepad output, calibrated axes/triggers, neutral recovery and rumble routing with permission guidance.
- [ ] A4. Enable real macOS/CoreAudio and Linux/PipeWire system-loopback capture, with source identity and feedback-loop protection.
- [ ] A5. Implement portable screen-color capture and consent/capability UI, with bounded workers and cancellation.

## Batch B — controller capabilities and ownership

- [ ] B1. Implement a verified Edge profile write transaction: fresh original backup, exact device/slot checks, readback and recoverable failure handling. Automated tests never write hardware.
- [ ] B2. Extend Bluetooth grip PCM into sustained bounded playback, with resampling, pacing, stop/underrun/reconnect handling and visible transport limits.
- [ ] B3. Integrate an installed external native DualSense provider: inspect, controlled attach/detach, health, handoff and restore. Do not silently install/start privileged drivers.
- [ ] B4. Separate background-core lifecycle from the window, with authenticated local IPC, bounded clients and safe reconnect/shutdown.
- [ ] B5. Add clearly labeled starter profiles and game-integration diagnostics for the requested games; improve adapter matching/replay. No universal gun detection is claimed.

## Batch C — release readiness and UX

- [ ] C1. Add macOS/Linux signed updater artifacts/feeds and conditional macOS code-signing/notarization plumbing. Actual Apple signing requires credentials that cannot be manufactured by the app.
- [ ] C2. Add a hardware-test workspace with per-controller/platform checks, actionable failures, test history/export and consistent availability controls.
- [ ] C3. Audit all tabs, dialogs, output states and platform variants for clipping, overlap, spacing, keyboard access, wording and reduced motion.
- [ ] C4. Add long-run/fault-injection checks and run full native/browser/core gates on all build platforms.
- [ ] C5. Update support/coverage/install documentation and prepare a build plus a precise final hardware checklist.

## External validation still required

Physical USB/Bluetooth/Edge behavior, native-game/provider interoperability, macOS/Linux permissions and audio endpoints, real latency/force, anti-cheat/Steam compatibility and Apple signing identity/notarization remain distinct from software validation. Record their results during the final hardware pass rather than assuming success.
