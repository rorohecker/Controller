# Game sessions and feedback (0.15.7)

## Getting started

1. On Profiles, select a profile and match its game executable (Use last app).
2. Enable Manage settings while this game is focused. Choose an input route and whether to use system-audio haptics. Gyro, remaps, triggers, and action rules come from the profile.
3. On Windows, Xbox routing requires ViGEmBus. Turn vibration on in the game. If the game sees two controllers, configure HidHide separately and allow CONTROLLER to read the physical pad.
4. Switch to Game mode, then focus the game or choose Start this session. The setup card shows the physical connection, external foreground app, actual input route, active action/mod receivers, and last feedback received. End session restores prior settings. Manual changes made during a session are preserved. Disabling Auto-apply restores the current session. Reset and PS + Options cancel it without restarting effects.

Native DualSense games normally work best with their own effects: use the physical route and turn custom triggers/actions off. A physical route alone does not detect or forward native game feedback. Automatic weapon recognition still requires a supported mod/telemetry source or manual selection.

## Real game rumble

Windows ViGEm Xbox notifications now carry large/small motor envelopes back to the hardware loop. USB translates these into separate 70/150 Hz grip waveforms inside the existing limiter and mixer, without replacing music or action cues. The Xbox game rumble slider controls their USB level; action cues can duck them. Bluetooth uses the controller's standard motor-rumble compatibility output; custom fallback cues take priority. Xbox rumble is two motor levels, not the game's original DualSense trigger/haptic audio.

Notifications are polled without blocking or writing HID from a callback. Zero motor commands stop game feedback. With the Xbox bridge off, completed app audio releases the exclusive controller endpoint so native games can open it; paused tracks and active audio sources retain it. Failed audio opens back off for ten seconds instead of retrying on every game packet. Disabling the bridge, disconnecting, Reset or exiting stops it. A command expires after ten seconds without another update, so a lost game cannot leave vibration running indefinitely. Games that send one nonzero command for longer than ten seconds need to update it to continue. USB audio endpoint failures appear as output-unavailable/error status; a received packet alone is not proof that the grips moved.

The notification implementation follows the installed vigem-client 0.1.4 request/poll API. Its notification feature is marked unstable upstream; ViGEm is an existing optional dependency, not a newly installed system driver. Primary reference: [ViGEm Xbox notification API](https://github.com/nefarius/ViGEmClient/blob/master/include/ViGEm/Client.h). Session orchestration was compared with [DualSenseClient auto profiles](https://dualsenseclient.github.io/DualSenseClient/guides/auto-profiles/); no implementation was copied.

## Action rules

Cooldown is independent of effect duration. Restart replaces the active action when another eligible event arrives; Finish current effect ignores retriggers until that rule's duration ends. Only while held can require any button or an analog trigger threshold. The condition is checked at firing time, including delayed single taps; losing it cancels the current action. Stop on button release cancels a button action. Cancel event names stop a rule when a matching local game event arrives. Reset/profile changes also cancel its trigger override and grip cue. The latest eligible rule owns the one-shot cue/trigger override; this is not yet a multi-track timeline.

## Sharing and maintenance

Profiles can retain author, profile version, game version, HTTPS source, license/reuse terms, maintenance date, and up to 20 dated platform/transport reports. Reports are explicitly self-reported and are stored/exported with the profile. They are not online votes or independent compatibility verification. No reports or controller data are uploaded. Existing Import/Export and backup history carry these records. Older schema 1/2/3 archives gain disabled session settings and empty metadata; no routing changes start merely because an old archive is imported.

HidHide rules are not changed automatically. Windows-only routing/audio controls do not appear on macOS or Linux; importing such a profile preserves its settings but skips unsupported operations. Physical game feedback, native permissions and feel still need hardware checks on each supported OS.
