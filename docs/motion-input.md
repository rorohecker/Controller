# Motion and input (0.17.0)

Touch now contains motion inspection, per-game aiming, virtual stick response and touchpad radial menus. Save & apply input settings sends the current profile to the controller core. The status confirms native acceptance; it does not confirm a game's input settings or physical feel.

## Sensors and calibration

The app reads factory gyro/accelerometer calibration from feature report 0x05 without writing controller storage. Bluetooth calibration is CRC checked. Invalid or missing data falls back to nominal 16 raw counts per degree/second and 8192 counts per g, with the reason shown. Full reports expose angular velocity, acceleration, device interval and estimated tilt. Basic Bluetooth reports have no motion samples.

The device timestamp uses 1/3 microsecond ticks. Integration handles wraparound, ignores duplicates and resynchronizes after gaps rather than applying one large mouse jump. A six-axis complementary gravity filter combines gyro rotation with acceleration near 1 g. Gravity-aware aim projects yaw around gravity and pitch around the controller's projected right axis. Pitch/roll are estimates; relative yaw has no absolute heading reference and can drift.

Set the controller down and choose **Measure still bias**. Saving requires two seconds of steady samples with low angular variance and stable acceleration. Movement restarts capture. Cancel keeps the previous result. The residual bias is saved locally under the controller identity, outside exported game profiles; old profile raw gyro biases are superseded by this calibrated measurement. Reset cancels capture and active input but keeps a saved bias. Factory calibration is always read-only.

## Per-game aiming and flick stick

Gyro aim retains its enable, activation, sensitivity, axis scale, inversion and smoothing controls. Counts per 360 degrees converts physical rotation to mouse counts; it depends on the game's mouse sensitivity. The migration default 101.25 preserves the former nominal mouse gain, and is not a measured game calibration.

To measure a game, use gyro sensitivity 1× and aim multiplier 1×, turn the controller through one physical turn, enter the camera's observed turn, and recalculate. Apply and check another turn. Disable game mouse acceleration and keep the same mouse sensitivity. Axis scales also affect the measurement; use scale 1 on the measured axis. The optional aiming multiplier applies while its chosen button is held.

Flick stick chooses a relative camera direction on the first outward right-stick push; rotating the held stick adds yaw. Initial turns finish over the configured 20–250 ms, including after a quick release. Release the stick after enabling, changing focus or Reset to arm it. Gyro aim must be enabled, Game mode active and another window focused. The virtual Xbox right stick is neutral while flick stick owns it. Physical controller input remains visible unless hidden separately. Games must accept mouse input; mixed mouse/controller support varies.

## Stick response

Windows virtual Xbox output supports independent axial/circular deadzones, inner and outer ranges, response exponent and anti-deadzone. Saved input range calibration runs first; the profile curve runs once afterward. Defaults reproduce the former 8% per-axis deadzone. Charts show the requested response and live raw/virtual axes show the resulting values. These settings do not change physical HID input, PC scrolling or controller firmware. The editor is hidden on macOS/Linux, where this app has no virtual Xbox output. Portable archives retain the settings.

## Radial menus

Enable 4–8 named sectors, choose a mode and activation button, and bind up to four keys per sector. Record a chord or select modifiers and a key manually when the OS intercepts Alt-Tab/Command-Tab. Platform labels and supported keys follow the current OS. Desktop input permissions still apply.

Hold the activation button with one finger on the pad, move toward a sector, then release the button while keeping that finger down. The center sends nothing. Circle, a second finger, a changed contact, finger lift, focus/mode changes and Reset cancel. Sector changes use angular hysteresis and an optional short grip cue. Pointer movement and app navigation resume after lifting the finger. Keys are released after their short chord or cancellation. The app's focused window previews selection; shortcuts go only to another focused window. Native physical input is not blocked from games.

## Evidence and limits

Offsets, nominal scales, factory report layout and timestamp units were checked against [SDL's PS5 HID driver](https://github.com/libsdl-org/SDL/blob/main/src/joystick/hidapi/SDL_hidapi_ps5.c) and [Sony's Linux HID driver](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-playstation.c). Implementation is original; no driver source is embedded.

Automated tests cover packet fields/CRC, factory scale validation, sensor timing, rate-independent mouse counts, gravity projection, stillness rejection, flick interruption, radial cancellation, profile roundtrips and platform visibility. They do not establish aiming feel, game compatibility, or desktop permissions on physical macOS/Linux hardware.
