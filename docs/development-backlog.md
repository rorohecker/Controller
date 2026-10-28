# Development backlog

Work in batches of five. Each batch includes regression checks, wording/layout review, and Rust check/Clippy where native code changes. Status: pending, in progress, complete, or experimental. Physical feel and operating-system permissions require validation on the relevant hardware; a passing build is not hardware verification.

## Batch 1 — Playback and desktop reliability

1. **Complete — Music transport:** normal Play/Pause/Stop/seek behavior; stale requests cannot restart stopped audio; clear elapsed/duration and output selection.
2. **Complete — Shortcut editor:** reliable chords, manual modifier builder, platform-specific shortcuts and cancellation; reserved OS combinations remain selectable when recording cannot receive them.
3. **Complete — macOS permissions:** live Accessibility status, explicit request/settings actions and recheck; distinguish recording permissions from output permissions.
4. **Complete — Single instance:** subsequent launches restore the existing window and never spawn a competing GUI/core.
5. **Complete — Per-controller calibration:** input and motor-zone maps keyed by controller identity, with conservative legacy migration and visible ownership.

## Batch 2 — Audio quality and performance

6. Complete — Bounded native audio streaming; move decoding/resampling out of the UI thread.
7. Complete — Remove callback lock contention; expose underruns, queue depth and stream recovery.
8. Complete — Preserve stereo grip assets and support independent left/right audition.
9. Complete — Feedback mixer with source levels, priorities, headroom and ducking.
10. Complete — Speaker/grip meters, limiter activity and clear endpoint/routing inspection.

## Batch 3 — Game compatibility

11. Pending — Receive real XInput game vibration and translate it to DualSense feedback.
12. Pending — Game setup assistant showing input route, feedback source, profile and last event.
13. Pending — Session profiles coordinate routing/audio/gyro/hiding and restore previous settings.
14. Pending — Action cooldowns, conditions, cancellation and retrigger behavior.
15. Pending — Community profile metadata, compatibility confirmations, versions and maintenance.

## Batch 4 — Trigger and haptic authoring

16. Pending — Effect timeline with trigger load/wall/kick/release and stereo grip lanes.
17. Pending — Requested-versus-encoded trigger preview, force steps and rounded timing.
18. Pending — Personal perceived-strength calibration, side balance and comfortable limits.
19. Pending — Reusable tactile clips/material library with descriptive tags and license records.
20. Pending — Bass/impact/texture/full-soundtrack conversion modes and matched A/B preview.

## Batch 5 — Motion and input

21. Pending — Factory gyro calibration and device timestamps before sensitivity changes.
22. Pending — Accelerometer fusion, gravity-aware aim, stillness calibration and motion preview.
23. Pending — Flick stick, aiming sensitivity and per-game real-world rotation calibration.
24. Pending — Per-game stick deadzones, response curves and before/after plots.
25. Pending — Touchpad radial menus and accessible selection feedback.

## Batch 6 — Daily use and presentation

26. Pending — Visible edited/saved/applied state, concise change summary and revert.
27. Pending — Active feedback ownership display and accessible Stop all action.
28. Pending — Functional color accents, event animations and approved pixel-art treatment.
29. Pending — Compact/comfortable density, numeric/reset controls, reduced motion and keyboard focus.
30. Pending — Layout checks at OS scaling levels and disconnected/loading/error/permission states.

## Batch 7 — Platform and integration work

31. Pending — Capture a selected game's audio with capability detection and fallback.
32. Pending — Runtime desktop permission/capability probes on macOS and Linux/Wayland.
33. Pending — Native DualSense feedback forwarding feasibility and driver/audio endpoint prototype.
34. Pending — Timed advanced Bluetooth haptic streaming experiment and latency comparison.
35. Pending — Recognize compatible DS5Dongle bridges and explain supported routes.

## Batch 8 — Hardware and research

36. Pending — DualSense Edge onboard profile backup/editing and physical-stop guidance.
37. Pending — Multiple named controllers, endpoint pairing and player assignments.
38. Pending — Weapon recognition adapters: telemetry first, manual override, optional HUD recognition with confidence.
39. Pending — REA investigation of DSX scheduling, audio startup, profile formats and artwork transforms; record evidence and limits.
40. Pending — Evaluate learned audio-to-haptic conversion offline; verify code/model/data licenses separately.

## Batch 9 — Reliability and maintenance

41. Pending — Raw HID record/replay for reconnect, malformed reports and timing regressions.
42. Pending — Profile and isolate process discovery/audio startup from HID input work.
43. Pending — Render visible dirty widgets and measure idle/background resource use.
44. Pending — Software latency diagnostics and optional external measurement procedure.
45. Pending — Refresh outdated planning/compatibility documents and add reproducible haptic A/B evaluation.

## Batch 10 - Calibration at the output layer

46. Pending - Apply saved drift/range correction at the driver or virtual-controller output so games receive corrected values; verify persistence across restart/reconnect, raw-versus-corrected plots and recovery. Investigate controller firmware calibration only with a verified protocol and reversible backup.

## Verification log

Batch 1 implementation is complete (0.15.3). Automated verification covers playback stop/append races, stale acknowledgements, pause/seek/resume, disconnected playback, shortcut modifier editing, native recording results, macOS permission UI, platform-specific labels, per-controller calibration isolation, and layout across ten tabs at five window widths. Rust tests, Cargo check, and strict Clippy pass locally on Windows. Windows, Linux, and Intel/Apple Silicon macOS passed release CI for 0.15.3. macOS native permission prompts and physical controller behavior remain unverified until tested on a Mac. The native single-instance plugin is registered before other plugins and before core startup. A general regression pass also found and fixed a packet-replay file-read race: Reset now cancels loads and in-flight replay continuations.

Batch 2 implementation is complete (0.15.5): native file picking/probing and worker decoding; band-limited resampling; one-second native queues; callback mutex removal; pause/underrun playhead preservation; decoder cancellation; stereo music/haptic assets; source levels, solo and ducking; output meters and routing/buffer details. Regression tests cover resampling at 22.05/44.1/48/96 kHz, exact remaining duration after seek, stereo isolation, bounded paused workers, dropped capture frames, underrun recovery, end-of-track device drain, muting, meter reset and source ducking. Browser tests exercise the native command path without webview decoding, transport state, stale events, saved mixer controls and responsive layouts. Physical speaker/grip quality still needs a USB check; the audio backend's peak limits remain bounded.

The unpublished 0.15.4 candidate exposed an atomic API deprecation on CI's newer Rust toolchain. The counter now uses a compare/exchange loop compatible with both toolchains.

Batch 3 is next. Driver/output calibration is item 46. Research claims and implementation status are tracked separately; experimental items must not be presented as supported features.
