# Game profiles

## Save a feel for each game

1. On **Triggers**, tune L2, R2, and recoil. Enter a name under **My trigger presets**, then **Save new**. **Update selected** replaces that saved preset.
2. On **Profiles**, create or select a game profile. Focus the game, return here, then choose **Use last app**. Add a window title or executable path only if several games use the same executable name.
3. Load a trigger preset on Triggers. Save the game profile. Enable auto-apply if it should load when that game is focused.

The trigger library is shared across profiles. Loading a library entry copies its settings into the current profile; later edits do not change the library entry until **Update selected**. Export/import includes the library, game actions, and weapon mappings. Imported preset IDs are remapped so existing entries are preserved.

## Actions

Under **Profiles → Game actions**, add a press, double-tap, hold, or named game event. Choose the button, tap/hold window, cooldown, duration, grip texture, and optional trigger feel. A trigger feel stores a snapshot of both triggers. Enable the rules and **Save & apply**.

Rules follow controller input. A Circle press does not prove a roll happened: a menu, cooldown, or different game binding can consume it. A single press waits for the double-tap window when both rules share a button. A hold fires once until release. PC control suspends button rules. A profile with an executable match suspends its rules and receiver outside that game. Reset, the panic combo, and disconnect clear controller effects.

## Weapon recognition

The app can switch presets when a game integration supplies a weapon name. It cannot universally identify guns from raw controller input. It does not inspect game memory or inject into games.

Save trigger presets, then add weapon names under **Weapons & game events**. Without an integration, the **Manual weapon** picker loads the same mappings. An unrecognized name restores the profile default.

For a local JSON integration, enable **Weapon / action JSON**, choose a port (default 6969), and Apply. Send UTF-8 UDP to `127.0.0.1:6969`:

```json
{"weapon":"smg","events":["dash"]}
```

Either field can be omitted. Names are case-insensitive, at most 80 bytes. At most 16 events and 8 KiB per packet are accepted. Each event packet fires once; only send it when the action happens. Repeated weapon announcements do not restart the same preset. The receiver binds only to loopback. Existing Forza/JSON racing telemetry remains separate.

### DSX mods

Choose **DSX mod UDP** and point a compatible mod to the same local port. Close DSX if it already owns that port. This implementation uses the published [DSX protocol and enum definitions](https://github.com/Paliverse/DSX/tree/main/Mod%20System%20%28DSX%20v3%29), not DSX application code or assets.

Supported controller index: 0. Supported trigger modes: OFF/Normal, Feedback/Resistance, Weapon/SemiAutomaticGun, Vibration/AutomaticGun, Slope Feedback, Multiple Position Feedback, and Multiple Position Vibration. RGB and reset are supported. Status responses report connection state; their battery field is a placeholder, not a live battery reading. Player/mic LEDs, trigger thresholds, legacy named/custom effects, and other instructions are rejected with a status message. This is partial compatibility; an arbitrary DSX mod is not guaranteed to work. Effects return to the profile after 15 seconds without a trigger update.

### Game research, 2026-10-27

- **Crimson Desert:** [CrimsonDesertTelemetry](https://github.com/ZappendusterFX/CrimsonDesertTelemetry) exposes light, player, camera, and ambient data. Its public README does not document weapon/fire/dodge events. No weapon adapter is bundled.
- **Modern Warfare and Rematch:** no verified, public weapon/action feed was found in this pass. Use input rules and manual weapon presets.
- [Paliverse's integration list](https://github.com/Paliverse/DualSenseX#game-mods-with-dualsensex) shows previous game-specific work for Forza, Cyberpunk, Skyrim, and others. These are integrations, not a universal dataset of controller-to-game actions.
- No source found combined a reusable weapon/action dataset, clear redistribution terms, and strong community validation for the three requested games. Existing editable presets remain templates; they do not claim to reproduce official game telemetry.

## PC shortcuts and ports

In **PC controls**, select **Open app** and paste its full executable path. On macOS an absolute `.app` bundle path also works. Programs are launched directly, without a shell command. **Record shortcut** captures combinations such as Ctrl+Shift+K. Hold keeps the combination down; Tap presses and releases it. Key sequences accept one to five following key names or numeric virtual-key codes.

Windows uses SendInput. macOS uses native events and needs Accessibility access. Linux uses X11, supported Wayland protocols, or the desktop portal through Enigo. The Wayland desktop may ask for permission. Exact support depends on the compositor. Auto profile detection uses macOS application names or X11 foreground window information; a pure Wayland session may require manual profile selection. XInput, screen sampling, and system audio capture remain Windows features. Hardware behavior on macOS/Linux has not been physically checked on this Windows PC.
