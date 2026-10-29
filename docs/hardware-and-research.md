# Hardware library and research

Batch 8, 0.20.0. Software checks are separate from physical validation.

## Controller names, players and audio

In **Settings > Controllers**, select a connected controller, give it a name, choose a player light and pair a USB audio endpoint. Save & apply stores the assignment by the same stable local identity used for calibration. Reconnect restores the pairing. The native picker uses the exact saved endpoint ID; an unavailable endpoint fails instead of falling through to another controller. Corrupt controller-library data also blocks audio until it is repaired.

Automatic selection remains available when only one matching controller endpoint is present. With two identical audio names, compare the endpoint ID suffixes or rename the outputs in the OS sound settings before pairing. Pairing is manual; format compatibility does not prove which physical pad owns an endpoint.

One controller is active at a time. Saved names and assignments manage multiple controllers; they do not add concurrent input/output streams. Player lights 1–5 are controller indicators, not a way to force a Windows Xbox user index. A player light and a paired endpoint cannot be assigned to two saved controllers. Changing the active audio pairing stops its feedback sources. Forget removes an offline assignment; it does not remove calibration or Edge backups.

The roster lives in the normal CONTROLLER configuration directory under `hardware/controllers.json`. Writes are atomic. Identity falls back to a path-derived ID if neither a usable serial nor pairing address is available; that fallback may change with the connection path. Names do not turn a path fallback into a hardware identity.

The output layer now cancels a worker when its startup times out, suppresses errors from cancelled workers, and prevents a renamed paired output from being captured as its own loopback source.

## Edge backups and local drafts — experimental

**Profiles > DualSense Edge** reads four onboard slots from a USB Edge. It sends GET feature requests `0x70` through `0x7B`, with no feature SET requests. A separate worker verifies the requested controller identity and captures all twelve complete 64-byte reports before returning a snapshot. Only one read worker may exist. A four-second deadline is checked around feature reads; an OS request already in progress can outlive the deadline. It cannot start unlimited workers or block the main input loop. Request IDs prevent an old read result from completing a newer request.

The GUI confirms “saved” only after the original snapshot is written. Backups retain the exact original bytes, firmware label, source identity and capture time. Import/export, selection and deletion are available. A damaged backup is reported separately and does not hide valid backups or disable the controller library. Up to 128 snapshots are retained; existing snapshots are never silently rotated out.

The local editor supports the name, independent L2/R2 deadzones and grip/trigger strength fields of assigned revision-1 slots with a valid content CRC. The Fn + Triangle default remains read-only. Unknown bytes, stick curves, remaps, unique IDs and timestamps stay intact. Drafts have their own format and are not treated as original backups. **Writing a draft to the controller is not enabled.** No Edge was available to verify live reads or read-back after writing; this item remains experimental.

Set both physical stops to full travel before calibration. Short stops reduce the travel available to your finger; onboard trigger deadzones and app calibration are separate. Sony documents the default shortcut as unchangeable and explains the interaction between trigger stops and adaptive effects in its [Edge setup guide](https://www.playstation.com/en-us/support/hardware/set-up-edge-controller/).

Protocol reference: [DualSense Tester at 5bc7ece](https://github.com/daidr/dualsense-tester/tree/5bc7ecee9f9bd09c7a562b5f0f2cbc0c35c710f9), MIT, particularly `ProfileWidget.vue`, `_Profile/profile.ts` and `crc32.util.ts`. This app uses an original Rust implementation of the documented fields and content CRC; it does not copy controller artwork or the upstream writer.

## Weapon-recognition adapters

Use **Profiles > Weapon recognition** with the loopback JSON receiver and existing weapon-to-preset mappings. A packet may provide:

```json
{"detection":{"weapon":"smg","source":"telemetry","confidence":1,"ageMs":0,"adapter":"my-game"}}
```

Sources are `telemetry` or `hud`. A source label is the adapter's claim, not authentication. Configure the confidence threshold, consecutive matching observations, freshness and manual hold. HUD observations are disabled by default. Invalid, stale, low-confidence and unconfirmed observations cannot change the weapon. Freshness includes the adapter-reported frame age. When accepted detection expires, the app restores the profile's default trigger settings. Manual selection is authoritative until explicitly cleared by default; optional timed holds are supported. Profile changes clear the previous profile's manual choice. Reset cancels pending weapon applications, and a newer selection cannot inherit an older selection's input effect.

Legacy `{"weapon":"name"}` packets remain compatible. With filtering enabled they are treated as confidence-1 telemetry observations. With filtering disabled, the existing legacy behavior remains. Direct DSX trigger instructions are separate from weapon recognition.

- [`weapon_adapter.py`](../examples/weapon_adapter.py) maps documented JSON-line game/mod fields through an explicit ID dictionary. Unknown IDs produce no observation. It does not hook a process, read game memory or invent a game dataset.
- [`hud_weapon_adapter.py`](../examples/hud_weapon_adapter.py) is an optional local template-matching reference. Install Pillow and mss in your own Python environment, capture the same small HUD crop for each weapon, and provide the region and template paths described in the script. It compares normalized grayscale crops, rejects blank/ambiguous matches and sends at most ten observations per second. `--dry-run` prints observations without sending them to the app. Its match score is not a calibrated probability. Template matching and real-game accuracy need validation with the chosen game/HUD; changing resolution, scale, language or icon appearance can invalidate templates. Screen capture depends on OS permissions and backend support; some Wayland sessions cannot use mss capture.

No game artwork or recognition model is included. Crimson Desert, Modern Warfare and Rematch still have no bundled verified weapon telemetry adapter. Input-driven actions and manual selection remain available. Existing game-source findings are in [game profiles](game-profiles.md).

The optional adapter's image preprocessing was checked with Pillow 13.0.0 and MSS 10.2.0, using the documented [Pillow ImageOps](https://pillow.readthedocs.io/en/stable/reference/ImageOps.html) and [MSS region/RGB APIs](https://python-mss.readthedocs.io/stable/examples.html). This was an offline image/API check, not a successful game capture or permission test.

## REA findings: public DualSenseX 1.4.9

REA 6.8.0 inspected the public [1.4.9 package](https://github.com/Paliverse/DualSenseX/releases/tag/1.4.9) without executing its installer or application. The installer was extracted locally; all binaries, unpacked assets and full REA output remain in ignored research storage. The [reduced evidence record](research/dsx-1.4.9-evidence.json) retains the target SHA-256, assembly version, MVID and selected call-site facts. It excludes application code and artwork. The pinned executable hash is `f2e7ef9667f84a2b2f66e9116b06b6fbc3fd5af6695a50366e862692459b7a59`.

Findings are build-specific:

- `StartReading` and `StartReadingTouchPad` start separate threads. Their generated routines contain `Thread.Sleep` calls preceded by integer literals 5 and 200 respectively. These are static call-site facts, not measured polling rates or latency guarantees.
- Audio-haptics setting handlers reference NAudio loopback/capture, multiplexing and WASAPI output constructors plus output initialization. This supports investigating separate capture/output startup; it does not establish exclusive mode, successful startup or the behavior of current DSX.
- Save-file metadata exposes named settings, while application routines reference JSON serialization/deserialization. That does not establish compatibility with current Steam DSX profile exports; no importer based on guessed equivalence was added.
- Managed call-site inspection did not recover an artwork-transform implementation. BAML/resource geometry and current Steam artwork remain unexamined. The app's approved controller illustration and shoulder animation are not derived from these binaries.

Reproduce the static lane with the [REA CLI](https://github.com/morluto/rea): `inspect-desktop-application`, `inspect-managed-artifact`, `inspect-managed-members` and `inspect-managed-native-boundaries`. Use `scripts/summarize_dsx_evidence.py` on the complete managed-members JSON to reproduce the reduced report. REA's exact package version and input hash matter. A public 2021-era build is not evidence of modern Steam DSX behavior.

## Learned audio-to-haptic evaluation

[`evaluate_haptics.py`](../scripts/evaluate_haptics.py) reads bounded PCM16/float32 WAV clips and reports file hashes, durations, peak/RMS/DC, clipping, crest factor, a 20–300 Hz frequency scan and level-independent envelope alignment. Positive lag means the candidate follows the source. Suggested comparison gains target RMS 0.08 with a peak ceiling of 0.25. It performs no inference or hardware playback and is not the app's runtime DSP comparator.

The [recorded evaluation](research/learned-audio-evaluation.json) uses two source sounds and their published model-1/model-2 outputs from [Sound2Hap at b37ee97](https://github.com/Iris1215/Sound2Hap/tree/b37ee9752c1758a29f31bb29aece1e4f0eba1cf8). All four outputs have matching five-second duration. Envelope correlations range from 0.88023 to 0.96404 and the best lag is zero on the ten-millisecond grid. Peaks reach 1.0, so a matched comparison requires attenuation. These values do not rank subjective quality, measure inference time, establish zero physical latency or prove an improvement over the app's DSP. No weights were loaded or executed.

Reuse review, checked 2026-10-29:

| Artifact | Primary evidence | Finding |
| --- | --- | --- |
| Sound2Hap source | [Repository LICENSE](https://github.com/Iris1215/Sound2Hap/blob/b37ee9752c1758a29f31bb29aece1e4f0eba1cf8/LICENSE) | MIT repository code; no separate checkpoint/model card located during this review |
| EnCodec code | [Official repository](https://github.com/facebookresearch/encodec#license) | Code is MIT; that statement is specifically about code |
| EnCodec 24 kHz base model used by inference model 1 | [Official model card](https://huggingface.co/facebook/encodec_24khz) | Model API did not declare a license field in its card data; no blanket checkpoint reuse conclusion inferred from the code license |
| EnCodec 48 kHz model | [Official model card](https://huggingface.co/facebook/encodec_48khz) | Model card declares MIT; this is a separate artifact, not the base used in the inspected model-1 inference file |
| Sound2Hap dataset | [Author's dataset card](https://huggingface.co/datasets/yinanli1215/Sound2Hap) | CC BY-NC 4.0; code terms do not replace dataset terms |

Only scripts and descriptive measurements are committed. Waveforms, checkpoints and training data are not redistributed. A runtime learned converter remains a future decision requiring checkpoint/dependency clarity and controlled physical comparison.

## Verification

Native tests cover snapshot shape/revision/CRC preservation, protected default slots, controller assignment conflicts, renamed-output self-capture prevention, observation validation, confirmation, manual ownership and freshness. Browser checks cover saved-versus-applied state, failed writes, unavailable endpoints, stale read IDs, untouched original backups, recognition persistence, rapid weapon/reset races and five widths. Offline tool checks cover WAV parsing, timing-sign conventions, blank/ambiguous HUD rejection and explicit telemetry-ID mapping. These checks do not verify physical Edge behavior, actual HUD classification, multiple simultaneous routes or a learned model's perceptual quality.
