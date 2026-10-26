# DualSense effects research

Checked 2026-10-26. These are references for the effect model, not official game profiles.

| Source | Public signal | Useful finding |
| --- | ---: | --- |
| [Sony's Linux driver](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-playstation.c) | upstream kernel driver | Input reports, status bits, light and audio controls |
| [Nielk1 trigger factories](https://gist.github.com/Nielk1/6d54cc2c00d2201ccb8c2720ad7538db) | 23 gist comments | Documented feedback, weapon, vibration, bow, and other firmware encodings |
| [DualSense-Windows](https://github.com/Ohjurot/DualSense-Windows) | 442 GitHub stars | Public C++ examples for HID input and adaptive trigger output |
| [DualSense Explorer](https://github.com/nondebug/dualsense) | 122 GitHub stars | Input and output report layout and audio endpoint behavior |
| [DualSensitive](https://github.com/tpetsas/dualsensitive) | 7 GitHub stars | Unverified weapon examples |
| [DualSenseBridge](https://github.com/flyingriverhorse/DualSenseBridge) | 2 GitHub stars | Experimental game and weapon presets; limited community validation |

No weapon profile library found had both strong public approval and a repeatable hardware test for sniper, pistol, burst, and SMG effects. The built-in weapon cues therefore use the documented hardware modes and editable controller-input timing. They are labeled as templates. Overwatch 2 ability, damage, and shot events are unavailable to the app unless a game integration supplies them. A trigger-pull cue will not always coincide with a game shot.

The [REA project](https://github.com/morluto/rea) is useful for local, read-only inspection of an installed app. The locally available DualSenseX build in earlier inspection was an older release. No current DSX binary or private profile collection was available here, so no current DSX behavior or assets were copied. Add a repeatable, source-labeled comparison if a current build becomes available.

## Next useful integrations

1. Game event adapters for titles with documented local telemetry, starting with the existing Forza Data Out path.
2. Per-game profiles that declare their event source and fall back to input-driven cues when no data arrives.
3. Controller firmware and model matrix for trigger modes, PCM endpoint names, Bluetooth behavior, and Edge variants.
4. A community profile import format with source URL, license, tested controller/transport, version, and user rating captured at import time.
