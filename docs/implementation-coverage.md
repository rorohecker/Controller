# Implementation coverage (0.22.0)

Updated for CONTROLLER 0.22.0. The [development backlog](development-backlog.md) tracks current work. A build or software test does not establish physical controller support.

| Area | Implemented software | Remaining validation or work |
| --- | --- | --- |
| Architecture | Tauri 2 shell, restartable stdio core, pure protocol crate; one hardware writer; bounded audio/process workers | Independent daemon/service lifecycle and OS-call fault injection |
| USB/Bluetooth input | Device selection, full/basic reports, Bluetooth CRC, stalled-stream recovery, exact report lengths, per-controller identity | Long hotplug runs and more physical Bluetooth/Edge coverage |
| Ownership | Finite output leases, panic/Reset cancellation, source inspection, native-game handoff, cached conflict discovery | External-tool write ownership cannot be guaranteed |
| Trigger authoring | Ten-zone curves, Bow/weapon presets, timed input recoil, named per-game presets, custom break/return settings | Force and recoil need physical comparisons; input ranges cannot measure actuator force |
| Output calibration | Native per-controller ranges/noise, correction before game curves/XInput, raw/game plots, bypass/reload, previous-save recovery and stale neutral output | Virtual game correction is Windows/ViGEm only; physical HID and firmware remain unchanged. See [calibration](output-calibration.md) |
| Haptics/audio | Four-channel USB PCM, speaker/native file transport, bounded decode queues, stereo mixer, limiter, system/process capture, timeline authoring, personal limits and seeded A/B trials | Physical feel after scheduling changes; macOS/Linux endpoints; experimental Bluetooth PCM and native relay |
| Touch/motion/lights | Calibrated input ranges, touch pointer/right-stick scrolling, gestures, processed gyro/aim and calibration, custom RGB/player LEDs, microphone mute | Platform permissions and camera/gyro behavior on more devices; battery health and verified temperature are not reported |
| PC/game input | Per-platform chords/macros/app launching, optional Windows Xbox bridge and HidHide guidance, game setup/routing assistant | Steam/anti-cheat compatibility needs per-game testing; no universal Linux/macOS virtual-controller driver |
| Profiles | Schema-v3 archives, atomic saves, backups/import/export, controller assignments, foreground executable/title/path matching, weapon presets and action rules | Native weapon detection requires an adapter or user-supplied HUD templates |
| Telemetry | Local typed game events, recording/replay, Forza/JSON telemetry and bounded scripts; optional confidence-gated adapters | No generic live gun/ability detector or in-process injection |
| Edge/research | Read-only USB snapshots/backups, local Edge draft editing, static REA findings and reproducible learned-waveform inspection | Edge hardware writes remain disabled; no learned runtime model is bundled |
| Diagnostics | Drift/travel guides, live rate/errors, read-only raw-input trace analysis, software timing/round-trip checks and resource samples | Physical latency rig, trigger-force measurement and broad game/device matrix |
| Presentation/platforms | Geist, approved pixel backdrop/controller art, colored cards and moving sheen; responsive layouts, reduced motion and platform-specific controls | Native visual/permission/audio validation on macOS and Linux |
| Verification/distribution | Unit/browser/offline/stdin integration gates; HID-disabled core mode; Windows installer, universal macOS and Linux packages; Windows signed updates | Physical testing is distinct from release CI; portable updater feeds are absent |

The Windows installer includes the signed upstream optional ViGEmBus setup. HidHide is linked, not bundled. Direct HID features do not require either driver. No custom kernel calibration/filter driver or Edge firmware writer is shipped. See [driver sources](../resources/DRIVER-SOURCES.md), [platform support](platform-support.md), and [runtime checks](runtime-reliability.md).
