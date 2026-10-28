# Tactile authoring (0.16.0)

## Timeline and library

On Haptics, use Tactile workshop to load or create a clip. Click a colored lane block to edit it. Start/duration fields control time; start/end zones control physical trigger travel. Load ramps force over time, Wall holds resistance, Kick uses a brief native weapon break (0x25), and Release turns resistance off. Kick is not a guarantee of finger return: its feel depends on the trigger position and remaining travel. A timeline is time-based rather than synchronized to a game's animation or an inferred weapon.

Clips contain at most 32 cues and eight seconds. Each cue lasts 10?3000 ms. Trigger lanes cannot overlap after their times are rounded upward to 10 ms boundaries. Stereo grip cues can overlap and are mixed inside the existing peak/average protection. Stereo grip playback requires USB; trigger-only clips also work over Bluetooth. HID scheduling shares the device loop, so loaded systems can deliver later than the requested time; this is not sample-accurate audio/HID synchronization.

Play clip starts a bounded native schedule. Stop clip, Reset and PS + Options cancel it. Trigger lanes return to their prior app settings at the end; expired base leases return to Off. Reset tracks timeline-owned triggers even if no static trigger profile was applied. Unowned trigger lanes are not overwritten by ordinary timeline restoration. Music and system/game audio can be mixed with clip cues; use source levels and ducking to control the blend.

Save as new or Update saved stores a clip in the library (64 max). Tags, author, license/reuse terms and an optional HTTPS source travel with profile exports and backups. The four built-in procedural clips are original CC0-1.0 data; no third-party haptic dataset or audio recording is bundled. Imported identical clips are deduplicated. Use in new action creates a profile action with a clip snapshot. Changing/deleting a library clip does not silently change its saved action copies. Action conditions, cooldown, cancellation and retrigger policy still apply; Finish current effect uses the actual timeline duration.

## Encoded preview

The workshop shows requested and rounded cue timing, native trigger packet bytes and discrete force levels. Triggers also has a Requested / encoded panel for the current L2 or R2 settings. This calls the same Rust encoder as playback, including the app's initial bow/weapon preload behavior and personal force ceiling. Live pull/release rules can subsequently change the packet. Firmware force steps are not measured force or a linear percentage of physical resistance. A preview does not send output; use Apply/Play to do that.

## Personal feel

Personal preferences are keyed to the connected controller identity. Use the four-step guide to find faintly noticeable pulses and comfortable maxima, then enable limits and Save personal feel. Nothing plays automatically. Each explicit USB Test briefly solos one grip at raw requested strength, bypassing personal/mixer levels while keeping the fixed peak/average protection. Pause music and stop other feedback before judging the pulse. These are subjective preferences, not sensor measurements or medical limits.

Noticeable thresholds lift nonzero authored cues without inventing vibration from silence. Comfortable maxima and balance attenuate all app USB grip sources and classic motor output. Speaker volume stays separate. The trigger ceiling clamps encoded force levels rather than raising weak zones. These limits stay applied after Reset and are restored on reconnect by the UI; they are not written into controller firmware. Controller acknowledgements report whether the hardware update succeeded. Settings are stored locally and are not included in shared game profiles.

## Conversion comparison

Native audio files offer Full soundtrack (the existing broad conversion), Bass (low-frequency emphasis), Impacts (onset envelope with decay), Texture (tactile carrier for higher notes) and Stereo haptic asset (unchanged stereo waveform). These modes affect grips; speaker audio remains the source track. Native decoding/resampling and bounded queues stay off the UI/audio callback threads. The compatibility/browser PCM path keeps its existing conversion; choose a native file for the new modes.

Capture a song position and use Play A/Play B to replay the same five seconds through the grips at the same mixer settings. This matches time and configured gain, not perceived loudness. Other active sources remain part of the mix. Rapid comparisons cancel superseded requests, and a comparison ending does not shorten the song's saved duration. Normal Play/Pause/Stop/seek remain available above.

Windows, Linux and universal macOS builds are checked in release CI. Actual trigger feel, comfortable settings and physical audio quality still need controller checks on each OS.
