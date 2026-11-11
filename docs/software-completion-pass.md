# Remaining software work

Legend: `[x]` implemented in software, `[~]` implemented but not verified end to end, `[ ]` not done. None of this is hardware verified.

Baseline: 0.22.0. Hardware testing is deferred until the software pass is ready. Tests and builds do not certify physical support. New experimental output paths stay opt-in; no controller firmware/profile writes are performed by automated checks.

## Batch A — platform backends and boundaries

- [x] A1. Enforce a local-content CSP; audit IPC, imported content and bounded external helpers.
- [x] A2. Add calculation/protocol fault and stress checks; fix non-finite, range, timing and cancellation bugs found.
- [x] A3. Implement Linux virtual gamepad output, calibrated axes/triggers, neutral recovery and rumble routing with permission guidance.
- [x] A4. Enable real macOS/CoreAudio and Linux PulseAudio/PipeWire-pulse system-loopback capture, with source identity and feedback-loop protection.
- [x] A5. Implement portable screen-color capture and consent/capability UI, with bounded workers and cancellation. Windows GDI sampling plus ScreenCaptureKit (macOS) and desktop-portal/PipeWire (Linux) paths with bounded queues; the macOS/Linux paths are unverified on a physical desktop.

## Batch B — controller capabilities and ownership

- [x] B1. Implement a verified Edge profile write transaction: fresh original backup, exact device/slot checks, readback and recoverable failure handling. Automated tests never write hardware.
- [x] B2. Extend Bluetooth grip PCM into sustained bounded playback, with resampling, pacing, stop/underrun/reconnect handling and visible transport limits. Opt-in; unverified on a physical Bluetooth pad.
- [x] B3. Integrate an installed external native DualSense provider: inspect, controlled attach/detach, health, handoff and restore. Do not silently install/start privileged drivers. Needs an installed vDS runtime and a physical interoperability test.
- [~] B4. Separate background-core lifecycle from the window, with authenticated local IPC, bounded clients and safe reconnect/shutdown. Implemented (`src/core_service.rs`, `src/core_io.rs`) and wired to the Settings background-core toggle, the `--core-service` / `--stop-background-core` entry points and an NSIS pre-install/pre-uninstall stop hook (`installer-hooks.nsh`). Not yet verified with a real installer, an update over a running core, or hardware reconnects; see the [hardware checklist](hardware-test-checklist.md).
- [ ] B5. Add clearly labeled starter profiles and game-integration diagnostics for the requested games; improve adapter matching/replay. No universal gun detection is claimed. Not done: no dedicated starter profiles were added in this pass; the Steam Input status check (`steam_input_status`) and telemetry allowed-source restriction are the only integration additions.

## Batch C — release readiness and UX

- [x] C1. Add macOS/Linux signed updater artifacts/feeds and conditional macOS code-signing/notarization plumbing. Actual Apple signing requires credentials that cannot be manufactured by the app. See [portable updates](portable-updates.md); no signed release has been produced and no Apple identity is configured.
- [ ] C2. Add a hardware-test workspace with per-controller/platform checks, actionable failures, test history/export and consistent availability controls. Not built as an in-app workspace; the written [hardware test checklist](hardware-test-checklist.md) and Diagnostics export (`export_diagnostics`) cover it for now.
- [ ] C3. Audit all tabs, dialogs, output states and platform variants for clipping, overlap, spacing, keyboard access, wording and reduced motion. Ongoing; not signed off. Browser layout checks exist but no native visual audit of every platform variant has been recorded.
- [ ] C4. Add long-run/fault-injection checks and run full native/browser/core gates on all build platforms. Not done: no long-run or OS fault-injection suite exists; CI now runs fmt, strict Clippy and tests on Windows, Linux and macOS, and the browser gate on Linux and Windows release.
- [~] C5. Update support/coverage/install documentation and prepare a build plus a precise final hardware checklist. Documentation and the [hardware checklist](hardware-test-checklist.md) are updated (see [what changed](software-completion-changes.md)); the final build and hardware run are pending.

## External validation still required

Physical USB/Bluetooth/Edge behavior, native-game/provider interoperability, macOS/Linux permissions and audio endpoints, real latency/force, anti-cheat/Steam compatibility and Apple signing identity/notarization remain distinct from software validation. Record their results during the final hardware pass rather than assuming success.
