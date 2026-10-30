# Runtime checks and haptic comparisons (0.21.1)

## Input capture and replay

Diagnostics → **Record input** captures raw input reports and connection, timeout and read-error markers in memory. Recording stops after ten seconds. A ring keeps the latest 2,048 events; a truncation marker carries the transport state and number of older events removed. Nothing is recorded until requested. Export uses compact JSON so the largest supported trace can be imported again. Files are limited to 1 MiB.

**Analyze a trace** is read-only. It validates the declared transport, timestamps and report lengths, then uses the production protocol decoder. It accepts 64-byte USB input, 78-byte full Bluetooth input with CRC validation, and 10-byte Bluetooth basic input. Results include invalid reports, sequence gaps/duplicates, reconnect markers and report intervals. A long interval is not proof of a dropped USB packet. Counts describe the retained window, not events removed before it.

Analysis never updates the live controller, invokes desktop bindings, creates a virtual input event or writes feedback. Fixtures exercise sequence wrap, reconnects, truncated/oversized packets, bad CRCs and stale intervals. This is a parser/lifecycle analysis tool, not an emulated HID device or a replacement for physical reconnect testing. Traces contain raw button, sensor and touch data; controller paths, serials and audio are not captured.

Exports and imports use bounded workers. A 1 MiB trace can exceed the previous 450 kB command envelope after JSON escaping; the core now allows a bounded 2 MiB envelope plus header space. A maximum-size trace round trip is covered by the real stdio integration check.

## Work outside the controller loop

Windows process discovery now uses an immutable cached snapshot, as the portable backend already did. One worker scans every two seconds. The core checks the cache every 250 ms; snapshots older than six seconds stop providing foreground matches. Discovery duration and snapshot age are visible in Diagnostics. Portable subprocesses have a 600 ms deadline and bounded output.

Audio endpoint opening, stream teardown and system/native loopback setup run on workers on all three platforms. Opening has a four-second deadline. There are at most two opening/output workers and two system-capture workers; Windows process capture has its own existing limit. Cancellation signals workers without joining an OS call from the HID thread. Replacement output waits for prior streams from that engine to close and gets separate shared audio state, so old callbacks cannot consume new cues or publish stale errors.

USB preview commands wait in order for the endpoint to be ready. The pending queue is capped at 64 commands; each controller-loop pass handles at most 32. Reset, shutdown, handoff and the controller panic combo cancel pending playback. Track/clip Stop commands remove their own pending playback without stopping another track. Profile checkpoints remain ordered after preceding commands. Haptic previews and action tests keep motor fallback when USB audio cannot open; a new manual audio request retries the endpoint; automatic feedback observes a five-second failure cooldown.

HID enumeration, device reads and device writes still depend on OS calls. Moving audio/process discovery does not guarantee that every controller-loop iteration has a hard real-time deadline.

## Reading the timing panel

The performance lease lasts 30 seconds and is renewed only while Diagnostics is visible or a five-second resource sample is running. Recording has a separate ten-second limit.

| Reading | What it measures |
| --- | --- |
| HID read wait | Time inside the blocking read, including waiting for the next report |
| Decode | Software parsing time |
| Input processing | Parsing, mapping, feedback updates and event dispatch for a report |
| Loop period | Time between controller-loop passes, including reads and scheduled work |
| HID write call | Software time in the normal write path; not actuator onset |
| Bridge round trip | UI send → core queue → echo → UI, using one UI monotonic clock |
| Output startup | Time from worker launch until readiness is observed |
| Core CPU/memory | This core process only; CPU uses 100% for one core |

Timing distributions cover the latest 512 samples and show median, p95 and maximum. The round-trip check sends 20 passive probes. It does not send HID output. Core resources use OS process counters; current resident memory is available on Windows/Linux and reported unavailable on macOS. These readings exclude the GUI host, webview subprocesses, driver work outside this process and GPU use.

The UI retains input, calibration samples and playback state while hidden. Live DOM updates are scoped to the visible page and unchanged widget values are skipped. Native music playback needs no hidden UI timer; legacy chunk streaming continues pumping. The card reflection moves as a separate transform layer; offscreen and hidden decoration pauses. The sidebar, controller artwork, shoulder motion, palette and pixel background remain intact.

`tests/runtime_reliability_smoke.py` records headless Chromium main-thread task time and heap samples in `target/batch9/browser-resources.json`. Its controlled CSS comparison uses the same page, forces both reflections into their moving phase, and simulates the hidden flag. It is not a native WebView2, GPU, attached-controller or long-duration battery benchmark. There is no machine-dependent CPU pass threshold; deterministic checks require zero idle/hidden app draw frames. The committed [sample](research/runtime-performance.json) records scope, browser and source hashes. Animation choices follow the [Chrome team's transform/opacity guidance](https://web.dev/articles/animations-guide); Windows resident memory uses the documented [process working set](https://learn.microsoft.com/en-us/windows/win32/psapi/process-memory-usage-information).

## Physical latency measurement

Software measurements cannot establish controller-to-screen or actuator latency. For an optional external test:

1. Record controller model/firmware, USB or Bluetooth, game/input route, display refresh, audio device/format, profile, mixer and personal limits. Keep them fixed during a comparison.
2. Use a camera with a known frame rate that can see both a defined physical input point and the resulting display change. Keep framing and exposure fixed. Record at least 30 trials, including warm/cold starts separately.
3. Count frames from that defined point to the first visible response: `milliseconds = frames × 1000 / camera FPS`. Report median, p95, range and frame quantization. At 240 FPS one frame is about 4.17 ms. Finger motion is not electrical switch closure; rolling shutter and display scanout add uncertainty.
4. For haptics, use an external contact sensor or accelerometer and a reference signal recorded on the same acquisition clock. A microphone on a resonating box measures an acoustic response, not trigger force. Capture onset and waveform separately; do not align independent software clocks and call the difference physical latency.
5. Alternate conditions and keep controller mounting, grip/contact, source level and thermal state consistent. Save raw recordings and the method with the results. No such physical benchmark has been performed for this release.

## Reproducible haptic A/B trials

Haptics → **Haptic comparison** prepares four trials from a native USB audio file. Choose two conversion modes and a nonzero 32-bit seed. The versioned xorshift/Fisher–Yates plan assigns A to X twice and B to X twice. X/Y hide the assignment during scoring; preparing a plan does not play anything.

Each explicit Play uses the same five-second excerpt and mixer/personal settings. Playback must complete before both samples can be rated. Source/controller/settings changes, interrupted playback and Reset cancel the session. Cancellation only stops playback still owned by that trial. The report records the assignment, seed, modes, excerpt, source metadata, requested/applied levels, controller/firmware, each playback order/time and the user's ratings. Partial/cancelled sessions are marked and unscored trials are absent. Export omits the local file path. Actual hearing/feel is never inferred from successful software playback.

Score strength and clarity separately from 0–5, then choose X, Y or no preference. Fixed mixer gain does not equalize waveform RMS or perceived intensity. Keep playback order/repetitions consistent; extra plays are recorded. Keep the exact source file with the report: the UI does not calculate its checksum and exports `sourceChecksum: null`. The offline `scripts/evaluate_haptics.py` tool can provide waveform/file measurements separately. These small within-user comparisons do not establish a population preference or the quality of a learned model.

## Checks

```text
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
python tests/run_ui_checks.py
cargo build --bin controller
python tests/run_core_checks.py
python tests/update_manifest_smoke.py
```

The core integration scripts explicitly use `--core --no-hid`, which disables discovery before HID initialization and ignores Scan/Select requests. They can run with a real pad connected without opening it. Shutdown also exits when the caller keeps stdin open; the output thread no longer waits indefinitely for the command-reader sender to disappear. Browser/offline checks propagate every child failure to CI. Physical USB/BT feel, macOS/Linux permissions/audio endpoints, Edge writes and driver-level calibration remain separate validation work.
