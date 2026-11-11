#[path = "controls.rs"]
pub mod controls;
#[path = "motion.rs"]
pub mod motion;
// Gyro aim, touch gestures, and keyboard remapping.

use aftertouch_protocol::{InputState, TouchPoint};
use serde::Deserialize;
thread_local! { static INPUT_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) }; }
pub fn record_input_error(message: String) {
    INPUT_ERROR.with(|e| {
        if e.borrow().is_none() {
            *e.borrow_mut() = Some(message);
        }
    });
}
pub fn take_input_error() -> Option<String> {
    INPUT_ERROR.with(|e| e.borrow_mut().take())
}
pub fn supported_key(vk: u16) -> bool {
    #[cfg(windows)]
    {
        (1..=254).contains(&vk)
    }
    #[cfg(not(windows))]
    {
        crate::native_input::supported_key(vk)
    }
}
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    MapVirtualKeyW, SendInput,
};
#[cfg(not(windows))]
const MOUSEEVENTF_MOVE: u32 = 0x0001;
#[cfg(not(windows))]
const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
#[cfg(not(windows))]
const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
#[cfg(not(windows))]
const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
#[cfg(not(windows))]
const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
#[cfg(not(windows))]
const MOUSEEVENTF_WHEEL: u32 = 0x0800;

const VK_LEFT: u16 = 0x25;
const VK_RIGHT: u16 = 0x27;

#[derive(Clone, Debug, Deserialize, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputCalibration {
    pub axis_centers: [f32; 4],
    pub axis_mins: [f32; 4],
    pub axis_maxs: [f32; 4],
    pub trigger_rests: [f32; 2],
    pub trigger_fulls: [f32; 2],
    pub touch_mins: [f32; 2],
    pub touch_maxs: [f32; 2],
}

impl Default for InputCalibration {
    fn default() -> Self {
        Self {
            axis_centers: [0.0; 4],
            axis_mins: [-1.0; 4],
            axis_maxs: [1.0; 4],
            trigger_rests: [0.0; 2],
            trigger_fulls: [1.0; 2],
            touch_mins: [0.0; 2],
            touch_maxs: [1.0; 2],
        }
    }
}

impl InputCalibration {
    pub fn valid(&self) -> bool {
        self.axis_centers
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 0.3)
            && self
                .axis_mins
                .iter()
                .zip(self.axis_centers)
                .all(|(min, centre)| min.is_finite() && *min <= centre - 0.5 && *min >= -1.0)
            && self
                .axis_maxs
                .iter()
                .zip(self.axis_centers)
                .all(|(max, centre)| max.is_finite() && *max >= centre + 0.5 && *max <= 1.0)
            && self
                .trigger_rests
                .iter()
                .zip(self.trigger_fulls)
                .all(|(rest, full)| {
                    rest.is_finite()
                        && full.is_finite()
                        && *rest >= 0.0
                        && full <= 1.0
                        && full - rest >= 0.5
                })
            && self
                .touch_mins
                .iter()
                .zip(self.touch_maxs)
                .all(|(min, max)| {
                    min.is_finite()
                        && max.is_finite()
                        && *min >= 0.0
                        && max <= 1.0
                        && max - min >= 0.5
                })
    }

    pub fn apply(&self, input: &mut InputState) {
        for i in 0..4 {
            let value = input.axes[i];
            let extent = if value >= self.axis_centers[i] {
                self.axis_maxs[i] - self.axis_centers[i]
            } else {
                self.axis_centers[i] - self.axis_mins[i]
            };
            input.axes[i] = ((value - self.axis_centers[i]) / extent).clamp(-1.0, 1.0);
        }
        for i in 0..2 {
            input.buttons[6 + i] = ((input.buttons[6 + i] - self.trigger_rests[i])
                / (self.trigger_fulls[i] - self.trigger_rests[i]))
                .clamp(0.0, 1.0);
        }
        for point in input.touch.iter_mut().flatten() {
            point.x = ((point.x - self.touch_mins[0]) / (self.touch_maxs[0] - self.touch_mins[0]))
                .clamp(0.0, 1.0);
            point.y = ((point.y - self.touch_mins[1]) / (self.touch_maxs[1] - self.touch_mins[1]))
                .clamp(0.0, 1.0);
        }
    }
}

/// Touchpad pointer tuning shared by the PC-mode and touchpad-mouse mappers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerSettings {
    /// Multiplier on the base gain (1.0 = 1250 x 780 counts across the pad).
    pub sensitivity: f32,
    /// 0 disables acceleration; 1 reaches up to 3x gain on fast swipes.
    pub acceleration: f32,
}
impl Default for PointerSettings {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            acceleration: 0.0,
        }
    }
}
impl PointerSettings {
    pub fn sanitized(self) -> Self {
        Self {
            sensitivity: if self.sensitivity.is_finite() {
                self.sensitivity.clamp(0.2, 3.0)
            } else {
                1.0
            },
            acceleration: if self.acceleration.is_finite() {
                self.acceleration.clamp(0.0, 1.0)
            } else {
                0.0
            },
        }
    }
}

/// One-finger relative pointer: converts consecutive touch points into whole-pixel
/// moves, rejects contact jumps, and carries fractional pixels between reports.
#[derive(Default)]
pub struct PointerTracker {
    previous: Option<TouchPoint>,
    previous_at: Option<Instant>,
    carry: [f32; 2],
}
/// Report interval the acceleration curve is tuned for (USB, about 250 Hz). Slower links
/// (Bluetooth, about 100 Hz) report larger per-report deltas for the same finger speed.
const POINTER_REFERENCE_DT: Duration = Duration::from_millis(4);
/// Per-report speed scale so acceleration depends on finger speed, not on report rate.
/// Intervals at or below the reference leave the speed untouched.
fn pointer_rate_scale(dt: Option<Duration>) -> f32 {
    dt.map_or(1.0, |dt| {
        (POINTER_REFERENCE_DT.as_secs_f32() / dt.as_secs_f32().max(f32::EPSILON)).clamp(0.1, 1.0)
    })
}
impl PointerTracker {
    pub fn reset(&mut self) {
        self.previous = None;
        self.previous_at = None;
        self.carry = [0.0; 2];
    }
    pub fn is_tracking(&self) -> bool {
        self.previous.is_some()
    }
    /// Resume from `point` without moving the pointer this report.
    pub fn prime(&mut self, point: TouchPoint) {
        self.previous = Some(point);
        self.previous_at = Some(Instant::now());
        self.carry = [0.0; 2];
    }
    pub fn step(&mut self, point: TouchPoint, settings: &PointerSettings) -> Option<[i32; 2]> {
        let now = Instant::now();
        let dt = self
            .previous_at
            .replace(now)
            .map(|at| now.saturating_duration_since(at));
        let previous = self.previous.replace(point)?;
        let (delta_x, delta_y) = (point.x - previous.x, point.y - previous.y);
        if delta_x.abs() > 0.2 || delta_y.abs() > 0.2 {
            // The contact jumped (finger id swap or a lift between reports).
            self.carry = [0.0; 2];
            return None;
        }
        let mut x = delta_x * 1250.0 * settings.sensitivity;
        let mut y = delta_y * 780.0 * settings.sensitivity;
        if settings.acceleration > 0.0 {
            let speed = x.hypot(y) * pointer_rate_scale(dt);
            let gain = 1.0 + settings.acceleration * (speed / 16.0).min(2.0);
            x *= gain;
            y *= gain;
        }
        self.carry[0] += x;
        self.carry[1] += y;
        let out = [
            self.carry[0].trunc().clamp(-36.0, 36.0),
            self.carry[1].trunc().clamp(-36.0, 36.0),
        ];
        self.carry[0] = (self.carry[0] - out[0]).clamp(-1.0, 1.0);
        self.carry[1] = (self.carry[1] - out[1]).clamp(-1.0, 1.0);
        (out != [0.0; 2]).then_some([out[0] as i32, out[1] as i32])
    }
}

/// How the left stick drives the desktop in PC mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LeftStickMode {
    #[default]
    Off,
    Mouse,
    Wasd,
}

/// W, A, S, D with hysteresis: `[up, left, down, right]`.
fn wasd_state(previous: [bool; 4], axes: [f32; 2]) -> [bool; 4] {
    let values = [-axes[1], -axes[0], axes[1], axes[0]];
    std::array::from_fn(|i| values[i] > if previous[i] { 0.35 } else { 0.5 })
}
const WASD_KEYS: [u16; 4] = [87, 65, 83, 68];

/// What pressing a program's button does when the program is already running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgramMode {
    /// Run once; pressing again restarts it.
    #[default]
    Once,
    /// Run once; pressing again while it runs cancels it.
    CancelOnRepress,
    /// Loop until the button is pressed again.
    Toggle,
    /// Loop while the button stays held, stopping cleanly on release.
    WhileHeld,
}

/// Explicit desktop controls. The controller remains visible to games; this mapper
/// only adds mouse and keyboard input while the workbench is out of focus.
pub struct DesktopMapper {
    enabled: bool,
    ui_focused: bool,
    wait_release: bool,
    keyboard: RemapMapper,
    left_down: bool,
    right_down: bool,
    click_threshold: f32,
    pointer_settings: PointerSettings,
    reverse_scroll: bool,
    pointer: PointerTracker,
    scroll_last: Option<Instant>,
    wheel_remainder: f32,
    left_stick: LeftStickMode,
    wasd_down: [bool; 4],
    stick_carry: [f32; 2],
    launches: Vec<PcLaunchBinding>,
    launch_held: HashSet<u8>,
    last_launch: HashMap<u8, Instant>,
    launch_error: Option<String>,
    programs: Vec<(u8, crate::macros::Program)>,
    program_modes: HashMap<u8, ProgramMode>,
    program_held: HashSet<u8>,
    running: Option<(u8, crate::macros::Program, ProgramMode)>,
    trigger_down: bool,
    player: crate::macros::Player,
}

#[derive(Clone, Debug)]
pub struct PcLaunchBinding {
    pub button: u8,
    pub path: PathBuf,
    pub args: Vec<String>,
}

impl Default for DesktopMapper {
    fn default() -> Self {
        Self {
            enabled: false,
            ui_focused: true,
            wait_release: true,
            keyboard: RemapMapper::default(),
            left_down: false,
            right_down: false,
            click_threshold: 0.55,
            pointer_settings: PointerSettings {
                sensitivity: 0.7,
                acceleration: 0.0,
            },
            reverse_scroll: true,
            pointer: PointerTracker::default(),
            scroll_last: None,
            wheel_remainder: 0.0,
            left_stick: LeftStickMode::Off,
            wasd_down: [false; 4],
            stick_carry: [0.0; 2],
            launches: Vec::new(),
            launch_held: HashSet::new(),
            last_launch: HashMap::new(),
            launch_error: None,
            programs: Vec::new(),
            program_modes: HashMap::new(),
            program_held: HashSet::new(),
            running: None,
            trigger_down: false,
            player: crate::macros::Player::default(),
        }
    }
}

impl DesktopMapper {
    pub fn set_programs(&mut self, programs: Vec<(u8, crate::macros::Program)>) {
        self.stop_macros();
        self.programs = programs;
        self.program_held.clear();
    }
    /// How each program button behaves when pressed again; unlisted buttons use `Once`.
    pub fn set_program_modes(&mut self, modes: Vec<(u8, ProgramMode)>) {
        self.program_modes = modes.into_iter().collect();
    }
    pub fn set_pointer_acceleration(&mut self, acceleration: f32) {
        self.pointer_settings.acceleration = acceleration;
        self.pointer_settings = self.pointer_settings.sanitized();
    }
    pub fn set_left_stick_mode(&mut self, mode: LeftStickMode) {
        if self.left_stick != mode {
            self.release_stick_outputs();
            self.left_stick = mode;
        }
    }
    /// Remap key hold and inter-key gap, in milliseconds (clamped to 15..=500 / 5..=500).
    pub fn set_remap_timing(&mut self, hold_ms: u16, gap_ms: u16) {
        self.keyboard.set_timing(hold_ms, gap_ms);
    }
    /// True once if a macro binding was dropped because the 32-key queue was full.
    pub fn take_queue_overflow(&mut self) -> bool {
        self.keyboard.take_queue_overflow()
    }
    /// Buttons that a consuming binding is handling right now. The caller should hide
    /// them from the virtual gamepad.
    pub fn consumed_buttons(&self) -> [bool; 23] {
        if self.enabled && !self.ui_focused && !self.wait_release {
            self.keyboard.consumed()
        } else {
            [false; 23]
        }
    }
    /// Why PC controls are not acting right now, for a "suspended because..." hint.
    pub fn suspension_reason(&self) -> Option<&'static str> {
        if !self.enabled {
            None
        } else if self.ui_focused {
            Some("The CONTROLLER window is focused")
        } else if self.wait_release {
            Some("Waiting for all buttons to be released")
        } else {
            None
        }
    }
    pub fn stop_macros(&mut self) {
        let output = self.player.stop();
        self.perform(output);
        self.running = None;
        self.keyboard.set_suspended(true);
        self.wait_release = true;
    }
    fn release_stick_outputs(&mut self) {
        for (down, vk) in self.wasd_down.iter_mut().zip(WASD_KEYS) {
            if *down {
                send_key(vk, true);
                *down = false;
            }
        }
        self.stick_carry = [0.0; 2];
    }
    fn perform(&mut self, output: Vec<crate::macros::Output>) {
        use crate::macros::Output;
        for op in output {
            match op {
                Output::Key(vk, up) => send_key(vk, up),
                Output::Mouse(button, up) => send_mouse(
                    0,
                    0,
                    0,
                    match (button.as_str(), up) {
                        ("left", false) => MOUSEEVENTF_LEFTDOWN,
                        ("left", true) => MOUSEEVENTF_LEFTUP,
                        (_, false) => MOUSEEVENTF_RIGHTDOWN,
                        (_, true) => MOUSEEVENTF_RIGHTUP,
                    },
                ),
                Output::Scroll(amount) => {
                    send_mouse(0, 0, i32::from(amount) * 120, MOUSEEVENTF_WHEEL)
                }
                Output::Launch(path, args) => {
                    if let Err(e) = launch_app(&PathBuf::from(&path), &args) {
                        self.launch_error = Some(format!("Could not open {path}: {e}"));
                    }
                }
            }
        }
    }
    pub fn take_launch_error(&mut self) -> Option<String> {
        self.launch_error.take()
    }
    fn scroll_delta(&mut self, vertical: f32, elapsed: f32) -> i32 {
        if vertical.abs() <= 0.18 {
            self.wheel_remainder = 0.0;
            return 0;
        }
        let direction = vertical.signum() * if self.reverse_scroll { -1.0 } else { 1.0 };
        if self.wheel_remainder.signum() != direction {
            self.wheel_remainder = 0.0;
        }
        let travel = ((vertical.abs() - 0.18) / 0.82).clamp(0.0, 1.0);
        self.wheel_remainder += direction * travel.powf(0.85) * 960.0 * elapsed;
        // Send small wheel increments regularly; apps that need full detents accumulate them.
        let delta = ((self.wheel_remainder / 15.0).trunc() as i32).clamp(-8, 8) * 15;
        self.wheel_remainder -= delta as f32;
        delta
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn ui_focused(&self) -> bool {
        self.ui_focused
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.release_all();
        self.enabled = enabled;
        self.wait_release = true;
        self.keyboard.set_suspended(!enabled || self.ui_focused);
    }

    pub fn set_ui_focused(&mut self, focused: bool) {
        if self.ui_focused == focused {
            return;
        }
        self.release_all();
        self.ui_focused = focused;
        self.wait_release = true;
        self.keyboard.set_suspended(!self.enabled || focused);
    }

    pub fn set_bindings(
        &mut self,
        bindings: Vec<RemapBinding>,
        click_threshold: f32,
        pointer_sensitivity: f32,
        reverse_scroll: bool,
        launches: Vec<PcLaunchBinding>,
    ) {
        self.release_all();
        self.keyboard.set(true, bindings);
        self.keyboard
            .set_suspended(!self.enabled || self.ui_focused);
        self.click_threshold = click_threshold.clamp(0.05, 0.95);
        self.pointer_settings.sensitivity = pointer_sensitivity;
        self.pointer_settings = self.pointer_settings.sanitized();
        self.reverse_scroll = reverse_scroll;
        self.launches = launches;
        self.wait_release = true;
    }

    pub fn tick(&mut self) {
        if self.enabled && !self.ui_focused && self.player.active() {
            let output = self.player.tick(Instant::now());
            self.perform(output);
            if !self.player.active() {
                self.program_finished(Instant::now());
            }
        } else {
            self.keyboard.tick();
        }
        retry_pending_keyups();
    }

    /// A program ran to completion: loop it again when its mode asks for that.
    fn program_finished(&mut self, now: Instant) {
        if let Some((button, program, mode)) = self.running.take() {
            let again = match mode {
                ProgramMode::Toggle => true,
                ProgramMode::WhileHeld => self.trigger_down,
                _ => false,
            };
            if again && let Ok(output) = self.player.start(program.clone(), now) {
                self.perform(output);
                self.running = Some((button, program, mode));
                return;
            }
        }
        self.wait_release = true;
    }

    pub fn cancel_for_modal(&mut self) {
        self.release_all();
        self.wait_release = true;
    }
    pub fn release_all(&mut self) {
        self.stop_macros();
        if self.left_down {
            send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
        }
        if self.right_down {
            send_mouse(0, 0, 0, MOUSEEVENTF_RIGHTUP);
        }
        self.left_down = false;
        self.right_down = false;
        self.keyboard.set_suspended(true);
        self.pointer.reset();
        self.scroll_last = None;
        self.wheel_remainder = 0.0;
        self.release_stick_outputs();
        self.launch_held.clear();
        flush_pending_keyups();
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled || self.ui_focused {
            return;
        }
        let down = input.buttons.map(|value| value > 0.5);
        if let Some((button, _, mode)) = &self.running {
            self.trigger_down = down[usize::from(*button)];
            if *mode == ProgramMode::WhileHeld && !self.trigger_down {
                // Let go of the button: stop the loop and release anything it holds.
                let output = self.player.stop();
                self.perform(output);
                self.running = None;
            }
        }
        if self.wait_release {
            self.wait_release = down
                .iter()
                .enumerate()
                .any(|(index, pressed)| index != 6 && index != 7 && *pressed);
            if self.wait_release {
                return;
            }
            self.keyboard.set_suspended(false);
        }
        let now = Instant::now();
        let triggered = self
            .programs
            .iter()
            .find(|(button, _)| down[usize::from(*button)] && !self.program_held.contains(button))
            .map(|(button, program)| (*button, program.clone()));
        self.program_held = self
            .programs
            .iter()
            .filter(|(button, _)| down[usize::from(*button)])
            .map(|(button, _)| *button)
            .collect();
        if let Some((button, program)) = triggered {
            let mode = self.program_modes.get(&button).copied().unwrap_or_default();
            let running_same = self.player.active()
                && self
                    .running
                    .as_ref()
                    .is_some_and(|(running, _, _)| *running == button);
            self.release_all();
            if running_same && matches!(mode, ProgramMode::CancelOnRepress | ProgramMode::Toggle) {
                // Pressed again: the release_all() above is the whole action.
            } else {
                match self.player.start(program.clone(), now) {
                    Ok(output) => {
                        self.perform(output);
                        self.running = Some((button, program, mode));
                        self.trigger_down = true;
                    }
                    Err(e) => self.launch_error = Some(e.into()),
                }
            }
        }
        if self.player.active() {
            self.tick();
            return;
        }
        self.keyboard.update(input);
        for launch in &self.launches {
            if down[usize::from(launch.button)] {
                if !self.launch_held.contains(&launch.button)
                    && self
                        .last_launch
                        .get(&launch.button)
                        .is_none_or(|last| now.duration_since(*last) >= Duration::from_secs(1))
                {
                    if let Err(error) = launch_app(&launch.path, &launch.args) {
                        self.launch_error =
                            Some(format!("Could not open {}: {error}", launch.path.display()));
                    }
                    self.last_launch.insert(launch.button, now);
                }
                self.launch_held.insert(launch.button);
            } else {
                self.launch_held.remove(&launch.button);
            }
        }
        for (button, previous, down_flag, up_flag) in [
            (
                6,
                &mut self.left_down,
                MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP,
            ),
            (
                7,
                &mut self.right_down,
                MOUSEEVENTF_RIGHTDOWN,
                MOUSEEVENTF_RIGHTUP,
            ),
        ] {
            let pressed = input.buttons[button] >= self.click_threshold;
            if pressed != *previous {
                send_mouse(0, 0, 0, if pressed { down_flag } else { up_flag });
                *previous = pressed;
            }
        }
        match input.touch {
            [Some(point), None] | [None, Some(point)] => {
                if let Some([dx, dy]) = self.pointer.step(point, &self.pointer_settings) {
                    send_mouse(dx, dy, 0, MOUSEEVENTF_MOVE);
                }
            }
            _ => self.pointer.reset(),
        }
        let now = Instant::now();
        let elapsed = self.scroll_last.replace(now).map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f32().min(0.05)
        });
        let wheel = self.scroll_delta(input.axes[3], elapsed);
        if wheel != 0 {
            send_mouse(0, 0, wheel, MOUSEEVENTF_WHEEL);
        }
        self.drive_left_stick([input.axes[0], input.axes[1]], elapsed);
    }

    fn drive_left_stick(&mut self, axes: [f32; 2], elapsed: f32) {
        // A non-finite axis (corrupt report) would poison the carry and the pointer forever.
        let axes = axes.map(|a| if a.is_finite() { a } else { 0.0 });
        let elapsed = if elapsed.is_finite() {
            elapsed.max(0.0)
        } else {
            0.0
        };
        match self.left_stick {
            LeftStickMode::Off => {}
            LeftStickMode::Wasd => {
                let next = wasd_state(self.wasd_down, axes);
                for i in 0..4 {
                    if next[i] != self.wasd_down[i] {
                        send_key(WASD_KEYS[i], !next[i]);
                    }
                }
                self.wasd_down = next;
            }
            LeftStickMode::Mouse => {
                let radius = axes[0].hypot(axes[1]);
                if radius <= 0.15 {
                    self.stick_carry = [0.0; 2];
                    return;
                }
                let amount = ((radius - 0.15) / 0.85).clamp(0.0, 1.0);
                let speed = amount * amount * 1100.0 * self.pointer_settings.sensitivity;
                for (carry, axis) in self.stick_carry.iter_mut().zip(axes) {
                    *carry += axis / radius * speed * elapsed;
                }
                let out = self.stick_carry.map(|v| v.trunc().clamp(-40.0, 40.0));
                self.stick_carry = [
                    (self.stick_carry[0] - out[0]).clamp(-1.0, 1.0),
                    (self.stick_carry[1] - out[1]).clamp(-1.0, 1.0),
                ];
                if out != [0.0; 2] {
                    send_mouse(out[0] as i32, out[1] as i32, 0, MOUSEEVENTF_MOVE);
                }
            }
        }
    }
}

impl Drop for DesktopMapper {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[derive(Clone, Debug, Default)]
pub struct GyroAimConfig {
    pub enabled: bool,
    pub sensitivity: f32,
    pub yaw_scale: f32,
    pub pitch_scale: f32,
    pub invert_y: bool,
    pub activation: GyroActivation,
    pub deadzone: u16,
    pub smoothing: f32,
    /// Manual gyro offset in raw sensor counts (x, y, z), subtracted before aiming.
    pub bias: [i16; 3],
    /// What the activation button does (default: gyro on while held).
    pub gate: GyroGate,
    /// Gyro pauses while the right stick is deflected past this radius (0 = never).
    pub stick_off_threshold: f32,
    /// Smoothing applies only below this speed in deg/s and fades out above it (0 = always).
    pub smooth_threshold: f32,
    /// Extra gain on fast turns, 0..=2 (0 = linear).
    pub acceleration: f32,
    /// Speed in deg/s where acceleration starts (0 = 75).
    pub accel_threshold: f32,
    /// Allow flick stick to run while gyro aim itself is off.
    pub flick_without_gyro: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GyroActivation {
    #[default]
    Always,
    L2,
    R2,
    L1,
    R1,
    Touchpad,
}

/// What the activation input does to the gyro.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GyroGate {
    /// Gyro is live while the input is held.
    #[default]
    Hold,
    /// Gyro is live except while the input is held (ratchet: lift and reposition the pad).
    Disable,
    /// Each press switches the gyro on or off.
    Toggle,
}

impl GyroGate {
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "hold" => Some(Self::Hold),
            "disable" | "ratchet" => Some(Self::Disable),
            "toggle" => Some(Self::Toggle),
            _ => None,
        }
    }
}

impl GyroActivation {
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "always" => Some(Self::Always),
            "l2" => Some(Self::L2),
            "r2" => Some(Self::R2),
            "l1" => Some(Self::L1),
            "r1" => Some(Self::R1),
            "touchpad" => Some(Self::Touchpad),
            _ => None,
        }
    }

    fn pressed(&self, input: &InputState) -> bool {
        match self {
            Self::Always => true,
            Self::L2 => input.buttons[6] > 0.15,
            Self::R2 => input.buttons[7] > 0.15,
            Self::L1 => input.buttons[4] > 0.5,
            Self::R1 => input.buttons[5] > 0.5,
            Self::Touchpad => input.buttons[17] > 0.5 || input.touch.iter().any(Option::is_some),
        }
    }
}

/// Add gyro rates (deg/s, positive = stick positive) onto a stick pair. `full_scale_dps`
/// is the turn rate that equals full deflection; `strength` scales the contribution.
#[allow(dead_code)] // reserved for gyro-to-virtual-stick mixing
pub fn mix_gyro_into_stick(
    stick: [f32; 2],
    rates_dps: [f32; 2],
    full_scale_dps: f32,
    strength: f32,
) -> [f32; 2] {
    if !full_scale_dps.is_finite() || full_scale_dps <= 0.0 {
        return stick;
    }
    std::array::from_fn(|i| {
        let add = if rates_dps[i].is_finite() {
            rates_dps[i] / full_scale_dps * strength
        } else {
            0.0
        };
        (stick[i] + add).clamp(-1.0, 1.0)
    })
}

#[derive(Default)]
pub struct GyroAimMapper {
    pub config: GyroAimConfig,
    remainder_x: f32,
    remainder_y: f32,
    flick: controls::FlickState,
    filtered: [f32; 2],
    toggled: bool,
    prev_pressed: bool,
    reason: Option<&'static str>,
}

impl GyroAimMapper {
    pub fn set_config(&mut self, config: GyroAimConfig) {
        self.config = config;
        self.remainder_x = 0.0;
        self.remainder_y = 0.0;
        self.flick.reset();
        self.filtered = [0.0; 2];
        self.toggled = false;
        self.prev_pressed = false;
    }

    /// Smoothed `[yaw, pitch]` rates in deg/s from the last report, for mixing into a
    /// virtual stick with [`mix_gyro_into_stick`]. Zero while gated off.
    #[allow(dead_code)] // reserved for gyro-to-virtual-stick mixing
    pub fn aim_rates(&self) -> [f32; 2] {
        self.filtered
    }

    /// Why gyro output is currently zero, for a "suspended because..." hint.
    pub fn inactive_reason(&self) -> Option<&'static str> {
        self.reason
    }

    fn gate_open(&mut self, input: &InputState) -> bool {
        let activation = self.config.activation;
        let mut open = if activation == GyroActivation::Always {
            true
        } else {
            let pressed = activation.pressed(input);
            match self.config.gate {
                GyroGate::Hold => pressed,
                GyroGate::Disable => !pressed,
                GyroGate::Toggle => {
                    if pressed && !self.prev_pressed {
                        self.toggled = !self.toggled;
                    }
                    self.prev_pressed = pressed;
                    self.toggled
                }
            }
        };
        if !open {
            self.reason = Some("Gyro activation input is not engaged");
        } else if self.config.stick_off_threshold > 0.0
            && input.axes[2].hypot(input.axes[3]) > self.config.stick_off_threshold
        {
            open = false;
            self.reason = Some("Gyro pauses while the right stick is moved");
        }
        open
    }

    pub fn compute(
        &mut self,
        input: &InputState,
        motion: &motion::MotionReading,
        settings: &controls::MotionSettings,
        suspended: bool,
    ) -> [i32; 2] {
        let aim_on = self.config.enabled;
        let flick_on = settings.flick_enabled && (aim_on || self.config.flick_without_gyro);
        if !(aim_on || flick_on) || !motion.valid || suspended || input.panic_combo {
            self.remainder_x = 0.0;
            self.remainder_y = 0.0;
            self.filtered = [0.0; 2];
            self.flick.reset();
            self.reason = Some(if !(aim_on || flick_on) {
                "Gyro aim is off"
            } else if suspended {
                "Gyro is suspended"
            } else if input.panic_combo {
                "Panic combo is held"
            } else {
                "No motion data in this report"
            });
            return [0; 2];
        }
        // Duplicate and resynchronised reports carry no time: skip them without
        // discarding the flick or smoothing state.
        if motion.clock == motion::Clock::Duplicate || motion.dt <= 0.0 {
            return [0; 2];
        }
        self.reason = None;
        let dt = motion.dt;
        let open = aim_on && self.gate_open(input);
        let adjusted;
        let motion = if self.config.bias == [0; 3] {
            motion
        } else {
            let mut copy = motion.clone();
            for (rate, bias) in copy.gyro_dps.iter_mut().zip(self.config.bias) {
                // Raw counts to deg/s at the nominal 16 counts per deg/s.
                *rate -= f32::from(bias) / 16.0;
            }
            adjusted = copy;
            &adjusted
        };
        let axes = motion::aim_axes_in(motion, settings.space);
        let deadzone = f32::from(self.config.deadzone) / 1024.0;
        let target = if open {
            axes.map(|v| v.signum() * (v.abs() - deadzone).max(0.0))
        } else {
            [0.0; 2]
        };
        let tau = 0.002 + self.config.smoothing.clamp(0.0, 0.85) * 0.065;
        let mut blend = 1.0 - (-dt / tau).exp();
        let threshold = self.config.smooth_threshold;
        if threshold > 0.0 {
            // Tiered smoothing: steady below half the threshold, raw above it.
            let fast = ((target[0].hypot(target[1]) - threshold * 0.5) / (threshold * 0.5))
                .clamp(0.0, 1.0);
            blend += (1.0 - blend) * fast;
        }
        if open {
            for (filtered, target) in self.filtered.iter_mut().zip(target) {
                *filtered += (target - *filtered) * blend;
            }
        } else {
            self.filtered = [0.0; 2];
        }
        let aim = if input.buttons[usize::from(settings.aim_button)] > 0.15 {
            settings.aim_multiplier
        } else {
            1.0
        };
        let accel = if self.config.acceleration > 0.0 {
            let start = if self.config.accel_threshold > 0.0 {
                self.config.accel_threshold
            } else {
                75.0
            };
            let speed = self.filtered[0].hypot(self.filtered[1]);
            1.0 + self.config.acceleration.clamp(0.0, 2.0)
                * ((speed - start) / start).clamp(0.0, 2.0)
        } else {
            1.0
        };
        let scale = settings.counts_per_turn / 360.0;
        let flick = if flick_on {
            self.flick
                .update([input.axes[2], input.axes[3]], dt, settings)
        } else {
            self.flick.reset();
            0.0
        };
        let dx =
            (self.filtered[0] * dt * self.config.yaw_scale * self.config.sensitivity * aim * accel
                + flick)
                * scale;
        let dy = self.filtered[1]
            * dt
            * self.config.pitch_scale
            * self.config.sensitivity
            * aim
            * accel
            * scale
            * if self.config.invert_y { -1.0 } else { 1.0 };
        self.remainder_x = (self.remainder_x + dx).clamp(-100_000.0, 100_000.0);
        self.remainder_y = (self.remainder_y + dy).clamp(-100_000.0, 100_000.0);
        let output = [
            self.remainder_x.trunc().clamp(-2000.0, 2000.0) as i32,
            self.remainder_y.trunc().clamp(-2000.0, 2000.0) as i32,
        ];
        self.remainder_x -= output[0] as f32;
        self.remainder_y -= output[1] as f32;
        output
    }
    pub fn update(
        &mut self,
        input: &InputState,
        motion: &motion::MotionReading,
        settings: &controls::MotionSettings,
        suspended: bool,
    ) {
        let [x, y] = self.compute(input, motion, settings, suspended);
        if x != 0 || y != 0 {
            send_mouse(x, y, 0, MOUSEEVENTF_MOVE);
        }
    }
}

/// When a binding fires relative to how its button is pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    /// As soon as the button is down.
    #[default]
    Press,
    /// After the button has been held this many milliseconds.
    Long(u16),
    /// On a press that follows the previous release within this many milliseconds.
    Double(u16),
}

impl Trigger {
    fn met(self, pressed: bool, held: Option<Duration>, gap: Option<Duration>) -> bool {
        pressed
            && match self {
                Self::Press => true,
                Self::Long(ms) => held.is_some_and(|h| h >= Duration::from_millis(u64::from(ms))),
                Self::Double(ms) => gap.is_some_and(|g| g <= Duration::from_millis(u64::from(ms))),
            }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemapBinding {
    pub button: u8,
    pub vk: u16,
    /// First layer modifier (kept for older saved bindings).
    pub modifier: Option<u8>,
    pub behavior: RemapBehavior,
    pub sequence: Vec<u16>,
    pub chord: Vec<u16>,
    /// Further modifiers that must all be held along with `modifier`.
    pub extra_modifiers: Vec<u8>,
    /// While a modifier is held the modifier button's own base bindings stay silent.
    pub layer_only: bool,
    /// Pressing the modifier button latches the layer on/off instead of holding it.
    pub toggle_layer: bool,
    pub trigger: Trigger,
    /// Rapid fire for Hold bindings: key is down this many ms, then up this many (0 = off).
    pub turbo_ms: u16,
    /// Hide the button from the virtual gamepad while this binding is active.
    pub consume: bool,
}

/// Optional per-binding extras as sent by the UI. Flatten into the command DTO so old
/// saved bindings (without these keys) keep loading.
#[derive(Clone, Debug, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct RemapExtras {
    pub modifiers: Vec<u8>,
    pub layer_only: bool,
    pub toggle_layer: bool,
    pub long_press_ms: u16,
    pub double_tap_ms: u16,
    pub turbo_ms: u16,
    pub consume: bool,
}

impl RemapExtras {
    pub fn apply(self, mut binding: RemapBinding) -> RemapBinding {
        binding.extra_modifiers = self.modifiers;
        binding.layer_only = self.layer_only;
        binding.toggle_layer = self.toggle_layer;
        binding.trigger = if self.long_press_ms > 0 {
            Trigger::Long(self.long_press_ms)
        } else if self.double_tap_ms > 0 {
            Trigger::Double(self.double_tap_ms)
        } else {
            Trigger::Press
        };
        binding.turbo_ms = self.turbo_ms;
        binding.consume = self.consume;
        binding
    }
}

fn binding_modifiers(binding: &RemapBinding) -> impl Iterator<Item = u8> + '_ {
    binding
        .modifier
        .iter()
        .copied()
        .chain(binding.extra_modifiers.iter().copied())
}

fn binding_keys(binding: &RemapBinding) -> Vec<u16> {
    if binding.chord.is_empty() {
        vec![binding.vk]
    } else {
        binding.chord.clone()
    }
}

impl RemapBinding {
    /// Pure range and consistency check. The caller still checks `supported_key` for the
    /// current platform.
    pub fn validate(&self) -> Result<(), &'static str> {
        let key_ok = |k: &u16| (1..=254).contains(k);
        if self.button >= 23 {
            return Err("Unknown controller button");
        }
        if !key_ok(&self.vk)
            || self.chord.len() > 8
            || !self.chord.iter().all(key_ok)
            || self.chord.iter().collect::<HashSet<_>>().len() != self.chord.len()
            || self.sequence.len() > 31
            || !self.sequence.iter().all(key_ok)
        {
            return Err("Binding keys must be valid keys (at most eight in a shortcut)");
        }
        let modifiers: Vec<u8> = binding_modifiers(self).collect();
        if modifiers.len() > 3
            || modifiers.iter().any(|m| *m >= 23 || *m == self.button)
            || modifiers.iter().collect::<HashSet<_>>().len() != modifiers.len()
        {
            return Err("Use up to three different modifier buttons, not the bound button itself");
        }
        if (self.layer_only || self.toggle_layer) && modifiers.is_empty() {
            return Err("Layer-only and toggle layers need a modifier button");
        }
        match self.trigger {
            Trigger::Press => {}
            Trigger::Long(ms) if (150..=3000).contains(&ms) => {}
            Trigger::Double(ms) if (100..=1000).contains(&ms) => {}
            _ => return Err("Long-press must be 150-3000 ms and double-tap 100-1000 ms"),
        }
        if self.turbo_ms != 0
            && (!(30..=1000).contains(&self.turbo_ms) || self.behavior != RemapBehavior::Hold)
        {
            return Err("Rapid fire needs a held key and 30-1000 ms per half-cycle");
        }
        Ok(())
    }

    /// Validate every binding and reject sets that would fire two competing actions for
    /// one button (a plain press together with a long-press or double-tap on it).
    pub fn validate_set(bindings: &[RemapBinding]) -> Result<(), &'static str> {
        for binding in bindings {
            binding.validate()?;
        }
        for (i, a) in bindings.iter().enumerate() {
            for b in &bindings[i + 1..] {
                let mut ma: Vec<u8> = binding_modifiers(a).collect();
                let mut mb: Vec<u8> = binding_modifiers(b).collect();
                ma.sort_unstable();
                mb.sort_unstable();
                if a.button == b.button
                    && ma == mb
                    && (a.trigger == Trigger::Press) != (b.trigger == Trigger::Press)
                {
                    return Err(
                        "A button cannot have both a plain press and a long-press or double-tap in the same layer",
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemapBehavior {
    #[default]
    Hold,
    Tap,
    Macro,
}

impl RemapBehavior {
    pub fn from_label(value: &str) -> Option<Self> {
        match value {
            "hold" => Some(Self::Hold),
            "tap" => Some(Self::Tap),
            "macro" => Some(Self::Macro),
            _ => None,
        }
    }
}

/// Per-button press timing for long-press, double-tap and turbo.
#[derive(Default)]
struct ButtonClock {
    down_since: Option<Instant>,
    released_at: Option<Instant>,
    gap: Option<Duration>,
}
#[derive(Default)]
struct GestureTracker {
    buttons: [ButtonClock; 23],
}
impl GestureTracker {
    fn update(&mut self, input: &InputState, now: Instant) {
        self.update_down(&input.buttons.map(|v| v > 0.5), now);
    }
    fn update_down(&mut self, down: &[bool; 23], now: Instant) {
        for (clock, &pressed) in self.buttons.iter_mut().zip(down) {
            match (pressed, clock.down_since) {
                (true, None) => {
                    clock.gap = clock
                        .released_at
                        .map(|released| now.saturating_duration_since(released));
                    clock.down_since = Some(now);
                }
                (false, Some(_)) => {
                    clock.down_since = None;
                    clock.released_at = Some(now);
                }
                _ => {}
            }
        }
    }
    fn held_for(&self, button: u8) -> Option<Duration> {
        self.buttons
            .get(usize::from(button))?
            .down_since
            .map(|since| since.elapsed())
    }
    fn held_at(&self, button: u8, now: Instant) -> Option<Duration> {
        self.buttons
            .get(usize::from(button))?
            .down_since
            .map(|since| now.saturating_duration_since(since))
    }
    fn gap(&self, button: u8) -> Option<Duration> {
        self.buttons.get(usize::from(button))?.gap
    }
}

/// Taps hold their keys for a fixed time through the mapper's clock, so apps see a
/// real key press rather than a same-instant down/up.
const TAP_HOLD: Duration = Duration::from_millis(35);
#[derive(Default)]
struct TapScheduler {
    held: Vec<(Vec<u16>, Instant)>,
}
impl TapScheduler {
    fn press(&mut self, keys: Vec<u16>, now: Instant) {
        for key in &keys {
            send_key(*key, false);
        }
        self.held.push((keys, now + TAP_HOLD));
    }
    fn tick(&mut self, now: Instant) {
        self.held.retain(|(keys, due)| {
            if now >= *due {
                for key in keys.iter().rev() {
                    send_key(*key, true);
                }
                false
            } else {
                true
            }
        });
    }
    fn release_all(&mut self) {
        for (keys, _) in self.held.drain(..) {
            for key in keys.iter().rev() {
                send_key(*key, true);
            }
        }
    }
}
impl Drop for TapScheduler {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RemapTiming {
    hold_ms: u16,
    gap_ms: u16,
}
impl Default for RemapTiming {
    fn default() -> Self {
        Self {
            hold_ms: 45,
            gap_ms: 20,
        }
    }
}

const MACRO_QUEUE_LIMIT: usize = 32;

#[derive(Default)]
pub struct RemapMapper {
    enabled: bool,
    suspended: bool,
    bindings: Vec<RemapBinding>,
    keys_down: HashSet<u16>,
    taps_down: HashSet<usize>,
    macro_queue: VecDeque<u16>,
    macro_held: Option<(u16, Instant)>,
    next_macro_at: Option<Instant>,
    timing: RemapTiming,
    queue_overflow: bool,
    tapper: TapScheduler,
    gestures: GestureTracker,
    latched: HashSet<u8>,
    toggle_prev: HashSet<u8>,
    consumed: [bool; 23],
}

impl RemapMapper {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set(&mut self, enabled: bool, bindings: Vec<RemapBinding>) {
        if self.enabled == enabled && self.bindings == bindings {
            return;
        }
        self.release_all();
        self.enabled = enabled;
        self.bindings = bindings;
    }

    pub fn set_timing(&mut self, hold_ms: u16, gap_ms: u16) {
        self.timing = RemapTiming {
            hold_ms: hold_ms.clamp(15, 500),
            gap_ms: gap_ms.clamp(5, 500),
        };
    }

    /// True once if a macro was dropped because the key queue was full.
    pub fn take_queue_overflow(&mut self) -> bool {
        std::mem::take(&mut self.queue_overflow)
    }

    /// Buttons whose active binding asked to be hidden from the virtual gamepad.
    pub fn consumed(&self) -> [bool; 23] {
        self.consumed
    }

    pub fn set_suspended(&mut self, suspended: bool) {
        if self.suspended == suspended {
            return;
        }
        if suspended {
            self.release_all();
        }
        self.suspended = suspended;
    }

    fn release_all(&mut self) {
        for vk in self.keys_down.drain() {
            send_key(vk, true);
        }
        if let Some((vk, _)) = self.macro_held.take() {
            send_key(vk, true);
        }
        self.tapper.release_all();
        self.taps_down.clear();
        self.macro_queue.clear();
        self.next_macro_at = None;
        self.latched.clear();
        self.toggle_prev.clear();
        self.consumed = [false; 23];
        self.gestures = GestureTracker::default();
        flush_pending_keyups();
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled || self.suspended {
            self.consumed = [false; 23];
            return;
        }
        let now = Instant::now();
        self.gestures.update(input, now);
        self.update_latches(input);
        // Resolve the active bindings once for this report.
        let selected = self.active_bindings(input);
        self.consumed = [false; 23];
        // Consuming bindings hide the button while selected or still pending (a long-press
        // that has not matured); a tap shorter than the threshold is therefore never seen by
        // the virtual gamepad either.
        let pending = self.pending_bindings(input);
        for &index in selected.iter().chain(pending.iter()) {
            let binding = &self.bindings[index];
            if binding.consume {
                self.consumed[usize::from(binding.button)] = true;
            }
        }
        let active = self.active_keys_from(&selected, now);
        let mut releasing: Vec<_> = self.keys_down.difference(&active).copied().collect();
        releasing.sort_by_key(|vk| crate::macros::is_modifier_key(*vk));
        for vk in &releasing {
            if self.macro_held.is_none_or(|(held, _)| held != *vk) {
                send_key(*vk, true);
            }
        }
        let mut pressing: Vec<_> = active.difference(&self.keys_down).copied().collect();
        pressing.sort_by_key(|vk| !crate::macros::is_modifier_key(*vk));
        for vk in pressing {
            send_key(vk, false);
        }
        self.keys_down = active;
        let mut taps_down = HashSet::new();
        for index in selected {
            let binding = &self.bindings[index];
            if binding.behavior == RemapBehavior::Hold {
                continue;
            }
            taps_down.insert(index);
            if self.taps_down.contains(&index) {
                continue;
            }
            if binding.behavior == RemapBehavior::Tap {
                let mut keys = binding_keys(binding);
                keys.retain(|key| !self.keys_down.contains(key));
                keys.sort_by_key(|vk| !crate::macros::is_modifier_key(*vk));
                self.tapper.press(keys, now);
            } else if self.macro_queue.len() + 1 + binding.sequence.len() <= MACRO_QUEUE_LIMIT {
                self.macro_queue.push_back(binding.vk);
                self.macro_queue.extend(binding.sequence.iter().copied());
            } else {
                self.queue_overflow = true;
            }
        }
        self.taps_down = taps_down;
        self.tapper.tick(now);
        self.advance_macro();
    }

    pub fn tick(&mut self) {
        if self.enabled && !self.suspended {
            self.tapper.tick(Instant::now());
            self.advance_macro();
        }
        retry_pending_keyups();
    }

    /// Latch/unlatch toggle layers on the rising edge of their modifier button.
    fn update_latches(&mut self, input: &InputState) {
        let toggles: HashSet<u8> = self
            .bindings
            .iter()
            .filter(|binding| binding.toggle_layer)
            .flat_map(binding_modifiers)
            .collect();
        self.latched.retain(|button| toggles.contains(button));
        for button in toggles {
            let pressed = input
                .buttons
                .get(usize::from(button))
                .is_some_and(|v| *v > 0.5);
            if pressed && !self.toggle_prev.contains(&button) && !self.latched.remove(&button) {
                self.latched.insert(button);
            }
            if pressed {
                self.toggle_prev.insert(button);
            } else {
                self.toggle_prev.remove(&button);
            }
        }
    }

    /// Bindings that fire for this report. The deepest satisfied layer wins per button;
    /// a modifier button's own base bindings go quiet while it is engaged (or always,
    /// for layer-only and toggle layers).
    fn active_bindings(&self, input: &InputState) -> Vec<usize> {
        let pressed = |button: u8| {
            input
                .buttons
                .get(usize::from(button))
                .is_some_and(|v| *v > 0.5)
        };
        let layer_on = |button: u8| pressed(button) || self.latched.contains(&button);
        // A deeper layer's long-press that is still timing must hold back shallower bindings
        // on the same button, otherwise the base binding fires first and is then replaced.
        let mut pending_depth: HashMap<u8, usize> = HashMap::new();
        for &index in &self.pending_bindings(input) {
            let binding = &self.bindings[index];
            let entry = pending_depth.entry(binding.button).or_insert(0);
            *entry = (*entry).max(binding_modifiers(binding).count());
        }
        let satisfied: Vec<usize> = self
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| {
                binding.trigger.met(
                    pressed(binding.button),
                    self.gestures.held_for(binding.button),
                    self.gestures.gap(binding.button),
                ) && binding_modifiers(binding).all(&layer_on)
            })
            .map(|(index, _)| index)
            .collect();
        let mut deepest: HashMap<u8, usize> = HashMap::new();
        for &index in &satisfied {
            let binding = &self.bindings[index];
            let entry = deepest.entry(binding.button).or_insert(0);
            *entry = (*entry).max(binding_modifiers(binding).count());
        }
        let engaged: HashSet<u8> = satisfied
            .iter()
            .map(|&index| &self.bindings[index])
            .filter(|binding| binding_modifiers(binding).count() > 0)
            .flat_map(binding_modifiers)
            .collect();
        let layer_buttons: HashSet<u8> = self
            .bindings
            .iter()
            .filter(|binding| binding.layer_only || binding.toggle_layer)
            .flat_map(binding_modifiers)
            .collect();
        satisfied
            .into_iter()
            .filter(|&index| {
                let binding = &self.bindings[index];
                let depth = binding_modifiers(binding).count();
                depth == deepest[&binding.button]
                    && pending_depth
                        .get(&binding.button)
                        .is_none_or(|&pending| depth >= pending)
                    && (depth > 0
                        || (!engaged.contains(&binding.button)
                            && !layer_buttons.contains(&binding.button)))
            })
            .collect()
    }

    /// Long-press bindings whose button and layer modifiers are held but whose hold time has
    /// not elapsed yet. They may still fire, so they block shallower bindings and (when they
    /// consume) already hide the button from the virtual gamepad. Double-tap bindings are
    /// decided at the moment of the press, so they are never pending.
    fn pending_bindings(&self, input: &InputState) -> Vec<usize> {
        let pressed = |button: u8| {
            input
                .buttons
                .get(usize::from(button))
                .is_some_and(|v| *v > 0.5)
        };
        let layer_on = |button: u8| pressed(button) || self.latched.contains(&button);
        self.bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| {
                matches!(binding.trigger, Trigger::Long(_))
                    && pressed(binding.button)
                    && self.gestures.held_for(binding.button).is_some()
                    && !binding.trigger.met(
                        true,
                        self.gestures.held_for(binding.button),
                        self.gestures.gap(binding.button),
                    )
                    && binding_modifiers(binding).all(&layer_on)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether a turbo binding's key is in its "down" half-cycle.
    fn turbo_on(&self, binding: &RemapBinding, now: Instant) -> bool {
        binding.turbo_ms == 0
            || self
                .gestures
                .held_at(binding.button, now)
                .is_none_or(|held| {
                    (held.as_millis() as u64 / u64::from(binding.turbo_ms)).is_multiple_of(2)
                })
    }

    fn active_keys_from(&self, selected: &[usize], now: Instant) -> HashSet<u16> {
        selected
            .iter()
            .map(|&index| &self.bindings[index])
            .filter(|binding| binding.behavior == RemapBehavior::Hold)
            .filter(|binding| self.turbo_on(binding, now))
            .flat_map(binding_keys)
            .collect()
    }

    #[cfg(test)]
    fn active_keys(&self, input: &InputState) -> HashSet<u16> {
        self.active_keys_from(&self.active_bindings(input), Instant::now())
    }

    fn advance_macro(&mut self) {
        let now = Instant::now();
        if let Some((vk, until)) = self.macro_held {
            if now < until {
                return;
            }
            if !self.keys_down.contains(&vk) {
                send_key(vk, true);
            }
            self.macro_held = None;
            self.next_macro_at = Some(now + Duration::from_millis(u64::from(self.timing.gap_ms)));
        }
        if self.next_macro_at.is_some_and(|at| now < at) {
            return;
        }
        if let Some(vk) = self.macro_queue.pop_front() {
            if !self.keys_down.contains(&vk) {
                send_key(vk, false);
            }
            self.macro_held = Some((
                vk,
                now + Duration::from_millis(u64::from(self.timing.hold_ms)),
            ));
        }
    }
}

impl Drop for RemapMapper {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[derive(Default)]
pub struct MouseMapper {
    enabled: bool,
    gestures: bool,
    pointer: PointerTracker,
    settings: PointerSettings,
    previous_scroll: Option<f32>,
    previous_angle: Option<f32>,
    wheel_remainder: f32,
    left_down: bool,
    left_grace: Option<Instant>,
    edge_anchor: Option<EdgeGesture>,
    previous_zone: Option<TouchZone>,
    last_zone_tick: Option<Instant>,
    tapper: TapScheduler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchCue {
    Left,
    Right,
    Top,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TouchZone {
    Centre,
    Left,
    Right,
    Top,
}

impl TouchZone {
    fn at(point: TouchPoint) -> Self {
        if point.y <= 0.10 {
            Self::Top
        } else if point.x <= 0.10 {
            Self::Left
        } else if point.x >= 0.90 {
            Self::Right
        } else {
            Self::Centre
        }
    }

    fn cue(self) -> Option<TouchCue> {
        match self {
            Self::Left => Some(TouchCue::Left),
            Self::Right => Some(TouchCue::Right),
            Self::Top => Some(TouchCue::Top),
            Self::Centre => None,
        }
    }
}

fn circle_angle_delta(previous: f32, current: f32) -> Option<f32> {
    let delta = (current - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    (delta.abs() <= 0.6).then_some(delta)
}

/// A touch that began on a side edge, watched for an inward swipe.
struct EdgeGesture {
    started: Instant,
    left: bool,
    start: TouchPoint,
    last: TouchPoint,
}

const EDGE_WINDOW: Duration = Duration::from_millis(750);

impl EdgeGesture {
    fn arrow_key(&self) -> Option<u16> {
        if self.started.elapsed() > EDGE_WINDOW || (self.last.y - self.start.y).abs() > 0.12 {
            return None;
        }
        let dx = self.last.x - self.start.x;
        if self.left && dx >= 0.18 {
            Some(VK_RIGHT)
        } else if !self.left && dx <= -0.18 {
            Some(VK_LEFT)
        } else {
            None
        }
    }

    /// The gesture can no longer become an arrow key and the contact should go back to
    /// moving the pointer: its window passed, or it travelled far without qualifying.
    fn abandoned(&self) -> bool {
        self.arrow_key().is_none()
            && (self.started.elapsed() > EDGE_WINDOW
                || (self.last.x - self.start.x).hypot(self.last.y - self.start.y) > 0.25)
    }
}

impl Drop for MouseMapper {
    fn drop(&mut self) {
        self.cancel_pointer();
    }
}

impl MouseMapper {
    pub fn cancel_pointer(&mut self) {
        if self.left_down {
            send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
        }
        self.left_down = false;
        self.left_grace = None;
        self.pointer.reset();
        self.previous_scroll = None;
        self.previous_angle = None;
        self.edge_anchor = None;
        self.previous_zone = None;
        self.last_zone_tick = None;
        self.wheel_remainder = 0.0;
        self.tapper.release_all();
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Current `(sensitivity, acceleration)`.
    pub fn pointer(&self) -> (f32, f32) {
        (self.settings.sensitivity, self.settings.acceleration)
    }

    /// Touchpad pointer gain (0.2..=3.0) and acceleration (0..=1).
    pub fn set_pointer(&mut self, sensitivity: f32, acceleration: f32) {
        self.settings = PointerSettings {
            sensitivity,
            acceleration,
        }
        .sanitized();
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        // Profile apply re-sends the same state; that must not cancel a held drag.
        if self.enabled == enabled {
            return;
        }
        self.cancel_pointer();
        self.enabled = enabled;
    }

    pub fn set_gestures(&mut self, gestures: bool) {
        self.gestures = gestures;
        self.previous_angle = None;
        self.edge_anchor = None;
        self.previous_zone = None;
        self.last_zone_tick = None;
    }

    fn update_zone(&mut self, touch: [Option<TouchPoint>; 2]) -> Option<TouchCue> {
        let point = match touch {
            [Some(point), None] | [None, Some(point)] => point,
            _ => {
                self.previous_zone = None;
                return None;
            }
        };
        let zone = TouchZone::at(point);
        let previous = self.previous_zone.replace(zone);
        if previous.is_some_and(|prior| prior != zone)
            && self
                .last_zone_tick
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(150))
            && let Some(cue) = zone.cue()
        {
            self.last_zone_tick = Some(Instant::now());
            return Some(cue);
        }
        None
    }

    pub fn update(&mut self, input: &InputState) -> Option<TouchCue> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        self.tapper.tick(now);
        let cue = if self.gestures {
            self.update_zone(input.touch)
        } else {
            None
        };
        match input.touch {
            [Some(a), Some(b)] => {
                self.pointer.reset();
                self.edge_anchor = None;
                let centre = (a.y + b.y) * 0.5;
                if let Some(previous) = self.previous_scroll {
                    self.wheel_remainder += (previous - centre) * 900.0;
                }
                self.previous_scroll = Some(centre);
                let dx = (b.x - a.x) * 1.9;
                let dy = b.y - a.y;
                let angle = dy.atan2(dx);
                if self.gestures && dx.hypot(dy) >= 0.10 {
                    if let Some(previous) = self.previous_angle {
                        // Ignore jumps caused by touch IDs changing or a finger lifting.
                        if let Some(delta) = circle_angle_delta(previous, angle) {
                            self.wheel_remainder -= delta * 70.0;
                        }
                    }
                    self.previous_angle = Some(angle);
                } else {
                    self.previous_angle = None;
                }
                let detents = (self.wheel_remainder / 120.0).trunc() as i32;
                if detents != 0 {
                    let wheel = detents.clamp(-2, 2) * 120;
                    send_mouse(0, 0, wheel, MOUSEEVENTF_WHEEL);
                    self.wheel_remainder -= wheel as f32;
                }
            }
            [Some(point), None] | [None, Some(point)] => {
                self.previous_scroll = None;
                self.previous_angle = None;
                if let Some(gesture) = self.edge_anchor.as_mut() {
                    gesture.last = point;
                    if gesture.abandoned() {
                        // Not a swipe after all: resume pointer motion from here.
                        self.edge_anchor = None;
                        self.pointer.prime(point);
                    } else {
                        self.pointer.reset();
                    }
                } else if self.gestures
                    && !self.pointer.is_tracking()
                    && (point.x <= 0.12 || point.x >= 0.88)
                {
                    self.edge_anchor = Some(EdgeGesture {
                        started: now,
                        left: point.x <= 0.12,
                        start: point,
                        last: point,
                    });
                } else if point.y >= 0.12 {
                    if let Some([dx, dy]) = self.pointer.step(point, &self.settings) {
                        send_mouse(dx, dy, 0, MOUSEEVENTF_MOVE);
                    }
                } else {
                    // Top strip clutch: leaving it starts a fresh drag.
                    self.pointer.reset();
                    self.edge_anchor = None;
                }
            }
            [None, None] => {
                if self.gestures
                    && let Some(gesture) = self.edge_anchor.take()
                    && let Some(vk) = gesture.arrow_key()
                {
                    self.tapper.press(vec![vk], now);
                }
                self.edge_anchor = None;
                self.pointer.reset();
                self.previous_scroll = None;
                self.previous_angle = None;
            }
        }
        let button = input.buttons[17] > 0.5;
        let touching = input.touch.iter().any(Option::is_some);
        if button && touching {
            self.left_grace = None;
            if !self.left_down {
                send_mouse(0, 0, 0, MOUSEEVENTF_LEFTDOWN);
                self.left_down = true;
            }
        } else if self.left_down {
            // A brief touch dropout under a hard press must not split it into two clicks.
            let release = if button {
                let since = *self.left_grace.get_or_insert(now);
                now.saturating_duration_since(since) >= TOUCH_DROPOUT_GRACE
            } else {
                true
            };
            if release {
                send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
                self.left_down = false;
                self.left_grace = None;
            }
        }
        cue
    }
}

const TOUCH_DROPOUT_GRACE: Duration = Duration::from_millis(70);

/// Spawn a program and reap it from a detached waiter so no zombie remains.
fn launch_app(path: &std::path::Path, args: &[String]) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = if path.extension().is_some_and(|e| e == "app") {
        let mut c = Command::new("open");
        c.arg("-a").arg(path).arg("--args");
        c
    } else {
        Command::new(path)
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let child = command.args(args).spawn()?;
    reap_later(child);
    Ok(())
}

/// Hand a launched child to one shared reaper thread (instead of a thread per launch) that
/// polls for exit so finished programs never linger as zombies/handles.
fn reap_later(child: std::process::Child) {
    use std::sync::{Mutex, OnceLock, mpsc};
    static REAPER: OnceLock<Mutex<mpsc::Sender<std::process::Child>>> = OnceLock::new();
    let sender = REAPER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<std::process::Child>();
        let _ = std::thread::Builder::new()
            .name("launch-reaper".into())
            .spawn(move || {
                let mut children: Vec<std::process::Child> = Vec::new();
                loop {
                    if children.is_empty() {
                        match rx.recv() {
                            Ok(child) => children.push(child),
                            Err(_) => return,
                        }
                    } else {
                        match rx.recv_timeout(Duration::from_millis(500)) {
                            Ok(child) => children.push(child),
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    while let Ok(child) = rx.try_recv() {
                        children.push(child);
                    }
                    children.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
                }
            });
        Mutex::new(tx)
    });
    if let Ok(tx) = sender.lock() {
        let _ = tx.send(child);
    }
}

/// Windows "Enhance pointer precision" modifies relative pointer motion and so makes
/// touchpad and gyro aim feel inconsistent. `Some(true)` means it is on; `None` means
/// unknown or not applicable on this OS.
pub fn pointer_precision_enabled() -> Option<bool> {
    #[cfg(windows)]
    {
        let mut mouse = [0i32; 3];
        // SPI_GETMOUSE: [threshold1, threshold2, acceleration]
        let ok = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                0x0003,
                0,
                mouse.as_mut_ptr().cast::<std::ffi::c_void>(),
                0,
            )
        };
        (ok != 0).then_some(mouse[2] != 0)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn send_mouse(dx: i32, dy: i32, wheel: i32, flags: u32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: wheel as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        if SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) != 1 {
            record_input_error("Windows blocked desktop input. Focus a normal window; an elevated app requires matching permissions.".into());
        }
    }
}

/// Keys whose scan code has an 0xE0 prefix and so need KEYEVENTF_EXTENDEDKEY.
#[cfg_attr(not(windows), allow(dead_code))]
fn key_is_extended(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 // PgUp, PgDn, End, Home, arrows
            | 0x2C // Print Screen
            | 0x2D // Insert
            | 0x2E // Delete
            | 0x5B..=0x5D // Win keys, Apps
            | 0x6F // numpad divide
            | 0x90 // NumLock
            | 0xA3 // right Ctrl
            | 0xA5 // right Alt
    )
}

/// Keys sent by scan code (what games and DirectInput read). Numpad, media and a few
/// special keys stay on virtual-key injection because their scan codes are ambiguous.
#[cfg_attr(not(windows), allow(dead_code))]
fn key_uses_scancode(vk: u16, scan: u16) -> bool {
    scan != 0 && !matches!(vk, 0x13 | 0x2C | 0x60..=0x6F | 0x90 | 0x91 | 0xA6..=0xB7)
}

#[cfg(windows)]
/// Scan code and extended flag for a virtual key. MAPVK_VK_TO_VSC_EX (4) reports the 0xE0/0xE1
/// prefix in the high byte, so right-hand modifiers get the right scan code (VK 0xA1 -> 0x36).
fn vk_scan(vk: u16) -> (u16, bool) {
    let mapped = unsafe { MapVirtualKeyW(u32::from(vk), 4) } as u16;
    let extended = matches!(mapped >> 8, 0xE0 | 0xE1) || key_is_extended(vk);
    (mapped & 0xFF, extended)
}

#[cfg(windows)]
fn send_key_raw(vk: u16, up: bool) -> bool {
    let (scan, extended) = vk_scan(vk);
    let mut flags = 0;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if key_uses_scancode(vk, scan) {
        flags |= KEYEVENTF_SCANCODE;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) == 1 }
}

// Key-ups that failed (for example UIPI blocking an elevated window) and are retried
// on following ticks, so a key is never left down.
#[cfg(windows)]
thread_local! { static PENDING_UP: std::cell::RefCell<HashMap<u16, KeyUpRetry>> = std::cell::RefCell::new(HashMap::new()); }
/// A blocked key-up is retried with backoff for this long before giving up.
#[cfg(any(windows, test))]
const KEYUP_RETRY_WINDOW: Duration = Duration::from_secs(30);
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug)]
struct KeyUpRetry {
    first_failed: Instant,
    next_try: Instant,
    delay: Duration,
}
#[cfg(any(windows, test))]
impl KeyUpRetry {
    fn new(now: Instant) -> Self {
        let delay = Duration::from_millis(50);
        Self {
            first_failed: now,
            next_try: now + delay,
            delay,
        }
    }
    /// After a failed attempt: `false` once the window is exhausted, otherwise schedules the next.
    fn failed(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.first_failed) >= KEYUP_RETRY_WINDOW {
            return false;
        }
        self.delay = (self.delay * 2).min(Duration::from_secs(2));
        self.next_try = now + self.delay;
        true
    }
}

#[cfg(windows)]
pub(super) fn send_key(vk: u16, up: bool) {
    let ok = send_key_raw(vk, up);
    PENDING_UP.with(|pending| {
        let mut pending = pending.borrow_mut();
        if up && !ok {
            pending
                .entry(vk)
                .or_insert_with(|| KeyUpRetry::new(Instant::now()));
        } else {
            // Any fresh press supersedes an old failed release; a good release clears it.
            pending.remove(&vk);
        }
    });
    if !ok {
        record_input_error("Windows blocked desktop input. Focus a normal window; an elevated app requires matching permissions.".into());
    }
}

/// Retry key-ups that were blocked, a bounded number of times.
pub fn retry_pending_keyups() {
    #[cfg(windows)]
    {
        PENDING_UP.with(|pending| {
            let mut pending = pending.borrow_mut();
            if pending.is_empty() {
                return;
            }
            let now = Instant::now();
            pending.retain(|vk, retry| {
                if now < retry.next_try {
                    return true;
                }
                if send_key_raw(*vk, true) {
                    return false;
                }
                retry.failed(now)
            });
        });
    }
}

/// One immediate attempt at every blocked key-up, ignoring the backoff. Used when a mapper
/// is torn down, because no later tick may arrive to retry them. Still-blocked key-ups stay
/// queued for `retry_pending_keyups`.
pub fn flush_pending_keyups() {
    #[cfg(windows)]
    {
        PENDING_UP.with(|pending| {
            pending
                .borrow_mut()
                .retain(|vk, _| !send_key_raw(*vk, true));
        });
    }
}

/// Number of key-ups still waiting to be retried (always 0 off Windows).
#[allow(dead_code)] // diagnostics hook
pub fn pending_keyups() -> usize {
    #[cfg(windows)]
    {
        PENDING_UP.with(|pending| pending.borrow().len())
    }
    #[cfg(not(windows))]
    {
        0
    }
}

#[cfg(not(windows))]
fn send_mouse(dx: i32, dy: i32, wheel: i32, flags: u32) {
    crate::native_input::mouse(dx, dy, wheel, flags);
}
#[cfg(not(windows))]
pub(super) fn send_key(vk: u16, up: bool) {
    crate::native_input::key(vk, up);
}

pub fn prepare_desktop() -> Result<(), String> {
    #[cfg(all(not(windows), not(test)))]
    {
        crate::native_input::prepare()
    }
    #[cfg(any(windows, test))]
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn blocked_keyups_back_off_and_expire_by_wall_clock() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut retry = super::KeyUpRetry::new(t0);
        assert!(retry.failed(t0 + Duration::from_secs(1)));
        assert!(retry.next_try > t0 + Duration::from_secs(1));
        assert!(retry.failed(t0 + Duration::from_secs(29)));
        assert!(!retry.failed(t0 + Duration::from_secs(31)));
    }
    #[cfg(windows)]
    #[test]
    fn right_shift_maps_to_its_own_scan_code() {
        assert_eq!(super::vk_scan(0xA1), (0x36, false));
        assert_eq!(super::vk_scan(0xA0).0, 0x2A);
    }
    use super::*;
    fn mapper_with(bindings: Vec<RemapBinding>) -> RemapMapper {
        let mut mapper = RemapMapper::default();
        mapper.enabled = true;
        mapper.bindings = bindings;
        mapper
    }
    fn binding(button: u8, vk: u16) -> RemapBinding {
        RemapBinding {
            button,
            vk,
            ..Default::default()
        }
    }
    fn blank_input() -> InputState {
        let mut input = aftertouch_protocol::parse_input(&[0x01; 64]).unwrap();
        input.buttons = [0.0; 23];
        input.axes = [0.0; 4];
        input.touch = [None, None];
        input
    }
    #[test]
    fn calibrated_gyro_counts_match_across_sample_rates_and_zero_axis_scales() {
        let run = |ticks, axis_scale| {
            let mut input = aftertouch_protocol::parse_input(&[1; 64]).unwrap();
            input.buttons = [0.0; 23];
            input.axes = [0.0; 4];
            input.gyro = [0, 0, 1440];
            input.accel = [0, 0, 8192];
            let mut processor = motion::MotionProcessor::default();
            let mut mapper = GyroAimMapper::default();
            mapper.set_config(GyroAimConfig {
                enabled: true,
                sensitivity: 1.0,
                yaw_scale: axis_scale,
                pitch_scale: 1.0,
                ..Default::default()
            });
            let settings = controls::MotionSettings {
                counts_per_turn: 3600.0,
                ..Default::default()
            };
            let mut counts = 0;
            for tick in (0..=3_000_000u32).step_by(ticks) {
                input.sensor_timestamp = Some(tick);
                let reading = processor.update(&input, Instant::now());
                counts += mapper.compute(&input, &reading, &settings, false)[0];
            }
            counts
        };
        assert!((895..=901).contains(&run(3000, 1.0)));
        assert!((895..=901).contains(&run(12000, 1.0)));
        assert_eq!(run(3000, 0.0), 0);
    }
    #[test]
    fn duplicate_sensor_reports_do_not_cancel_a_flick_but_focus_changes_do() {
        let mut input = aftertouch_protocol::parse_input(&[1; 64]).unwrap();
        input.buttons = [0.0; 23];
        input.axes = [0.0; 4];
        input.gyro = [0; 3];
        input.accel = [0, 0, 8192];
        let mut processor = motion::MotionProcessor::default();
        let mut mapper = GyroAimMapper::default();
        mapper.set_config(GyroAimConfig {
            enabled: true,
            sensitivity: 1.0,
            yaw_scale: 1.0,
            pitch_scale: 1.0,
            ..Default::default()
        });
        let settings = controls::MotionSettings {
            counts_per_turn: 3600.0,
            flick_enabled: true,
            ..Default::default()
        };
        input.sensor_timestamp = Some(0);
        let r = processor.update(&input, Instant::now());
        mapper.compute(&input, &r, &settings, false);
        input.sensor_timestamp = Some(30000);
        let r = processor.update(&input, Instant::now());
        mapper.compute(&input, &r, &settings, false);
        input.axes[2] = 1.0;
        input.sensor_timestamp = Some(60000);
        let r = processor.update(&input, Instant::now());
        let mut total = mapper.compute(&input, &r, &settings, false)[0];
        let duplicate = processor.update(&input, Instant::now());
        assert_eq!(mapper.compute(&input, &duplicate, &settings, false), [0; 2]);
        input.axes[2] = 0.0;
        for i in 3..=15 {
            input.sensor_timestamp = Some(i * 30000);
            let r = processor.update(&input, Instant::now());
            total += mapper.compute(&input, &r, &settings, false)[0];
        }
        assert!((899..=901).contains(&total));
        input.axes[2] = 1.0;
        input.sensor_timestamp = Some(480000);
        let r = processor.update(&input, Instant::now());
        assert_eq!(mapper.compute(&input, &r, &settings, true), [0; 2]);
        assert_eq!(mapper.compute(&input, &r, &settings, false), [0; 2]);
    }

    #[test]
    fn measured_ranges_adjust_trigger_stick_and_touch_readouts() {
        let mut input = aftertouch_protocol::parse_input(&[0x01; 64]).unwrap();
        input.axes = [0.1, -0.5, 0.7, 0.0];
        input.buttons[6] = 0.65;
        input.buttons[7] = 0.55;
        input.touch[0] = Some(TouchPoint {
            id: 0,
            x: 0.8,
            y: 0.2,
        });
        let calibration = InputCalibration {
            axis_centers: [0.1, 0.0, 0.0, 0.0],
            axis_mins: [-0.8; 4],
            axis_maxs: [0.8; 4],
            trigger_rests: [0.1, 0.05],
            trigger_fulls: [0.65, 0.9],
            touch_mins: [0.1, 0.1],
            touch_maxs: [0.8, 0.9],
        };
        assert!(calibration.valid());
        calibration.apply(&mut input);
        assert_eq!(input.axes[0], 0.0);
        assert!((input.axes[1] + 0.625).abs() < 0.001);
        assert!((input.axes[2] - 0.875).abs() < 0.001);
        assert_eq!(input.buttons[6], 1.0);
        assert!((input.buttons[7] - 0.588).abs() < 0.002);
        assert_eq!(input.touch[0].unwrap().x, 1.0);
        assert!((input.touch[0].unwrap().y - 0.125).abs() < 0.001);
    }

    #[test]
    fn two_buttons_sharing_key_keep_it_active_until_both_release() {
        let mapper = mapper_with(vec![
            RemapBinding {
                button: 0,
                vk: 65,
                modifier: None,
                behavior: RemapBehavior::Hold,
                sequence: vec![],
                chord: vec![],
                ..Default::default()
            },
            RemapBinding {
                button: 1,
                vk: 65,
                modifier: None,
                behavior: RemapBehavior::Hold,
                sequence: vec![],
                chord: vec![],
                ..Default::default()
            },
        ]);
        let mut report = [0u8; 64];
        report[0] = 0x01;
        report[8] = 0x68;
        let both = aftertouch_protocol::parse_input(&report).unwrap();
        assert_eq!(mapper.active_keys(&both), HashSet::from([65]));
        report[8] = 0x48;
        let one = aftertouch_protocol::parse_input(&report).unwrap();
        assert_eq!(mapper.active_keys(&one), HashSet::from([65]));
        report[8] = 0x08;
        let none = aftertouch_protocol::parse_input(&report).unwrap();
        assert!(mapper.active_keys(&none).is_empty());
    }

    #[test]
    fn shortcut_keeps_all_keys_and_shared_modifier_until_the_last_binding_releases() {
        let mapper = mapper_with(vec![
            RemapBinding {
                button: 0,
                vk: 75,
                modifier: None,
                behavior: RemapBehavior::Hold,
                sequence: vec![],
                chord: vec![17, 16, 75],
                ..Default::default()
            },
            RemapBinding {
                button: 1,
                vk: 17,
                modifier: None,
                behavior: RemapBehavior::Hold,
                sequence: vec![],
                chord: vec![],
                ..Default::default()
            },
        ]);
        let mut input = aftertouch_protocol::parse_input(&[0x01; 64]).unwrap();
        input.buttons = [0.; 23];
        input.buttons[0] = 1.;
        input.buttons[1] = 1.;
        assert_eq!(mapper.active_keys(&input), HashSet::from([17, 16, 75]));
        input.buttons[0] = 0.;
        assert_eq!(mapper.active_keys(&input), HashSet::from([17]));
        input.buttons[1] = 0.;
        assert!(mapper.active_keys(&input).is_empty());
    }

    #[test]
    fn held_modifier_selects_layer_binding_over_base() {
        let mapper = mapper_with(vec![
            RemapBinding {
                button: 0,
                vk: 65,
                modifier: None,
                behavior: RemapBehavior::Hold,
                sequence: vec![],
                chord: vec![],
                ..Default::default()
            },
            RemapBinding {
                button: 0,
                vk: 66,
                modifier: Some(4),
                behavior: RemapBehavior::Hold,
                sequence: vec![],
                chord: vec![],
                ..Default::default()
            },
            RemapBinding {
                button: 4,
                vk: 67,
                modifier: None,
                behavior: RemapBehavior::Hold,
                sequence: vec![],
                chord: vec![],
                ..Default::default()
            },
        ]);
        let mut report = [0u8; 64];
        report[0] = 0x01;
        let mut input = aftertouch_protocol::parse_input(&report).unwrap();
        input.buttons[0] = 1.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([65]));
        input.buttons[4] = 1.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([66]));
        input.buttons[0] = 0.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([67]));
    }

    #[test]
    fn edge_tap_does_not_fire_but_inward_swipe_does() {
        let start = TouchPoint {
            id: 0,
            x: 0.04,
            y: 0.5,
        };
        let mut gesture = EdgeGesture {
            started: Instant::now(),
            left: true,
            start,
            last: start,
        };
        assert_eq!(gesture.arrow_key(), None);
        gesture.last.x = 0.31;
        assert_eq!(gesture.arrow_key(), Some(VK_RIGHT));
        gesture.last.y = 0.8;
        assert_eq!(gesture.arrow_key(), None);
    }

    #[test]
    fn right_stick_scroll_emits_small_steps_and_reverses_immediately() {
        let mut mapper = DesktopMapper::default();
        assert_eq!(mapper.scroll_delta(0.0, 0.016), 0);
        let first = (0..6).map(|_| mapper.scroll_delta(0.7, 0.016)).sum::<i32>();
        assert!(first < 0 && first % 15 == 0);
        let reverse = (0..6)
            .map(|_| mapper.scroll_delta(-0.7, 0.016))
            .sum::<i32>();
        assert!(reverse > 0 && reverse % 15 == 0);
        assert_eq!(mapper.scroll_delta(0.0, 0.016), 0);
        assert_eq!(mapper.wheel_remainder, 0.0);
    }

    #[test]
    fn touch_zone_tick_only_fires_on_a_new_zone() {
        let mut mapper = MouseMapper::default();
        let centre = TouchPoint {
            id: 0,
            x: 0.5,
            y: 0.5,
        };
        let left = TouchPoint {
            id: 0,
            x: 0.05,
            y: 0.5,
        };
        assert_eq!(mapper.update_zone([Some(centre), None]), None);
        assert_eq!(mapper.update_zone([Some(left), None]), Some(TouchCue::Left));
        assert_eq!(mapper.update_zone([Some(left), None]), None);
        assert_eq!(mapper.update_zone([None, None]), None);
        assert_eq!(mapper.update_zone([Some(left), None]), None);
    }

    #[test]
    fn circle_scroll_wraps_angles_and_ignores_touch_id_jumps() {
        let near = std::f32::consts::PI - 0.15;
        let across = -std::f32::consts::PI + 0.15;
        assert!((circle_angle_delta(near, across).unwrap() - 0.3).abs() < 0.001);
        assert_eq!(circle_angle_delta(0.0, 2.0), None);

        let mut mapper = MouseMapper::default();
        mapper.set_enabled(true);
        mapper.set_gestures(true);
        let mut input = aftertouch_protocol::parse_input(&[0x01; 64]).unwrap();
        for index in 0..4 {
            let angle = index as f32 * 0.25;
            let x = angle.cos() * 0.15 / 1.9;
            let y = angle.sin() * 0.15;
            input.touch = [
                Some(TouchPoint {
                    id: 0,
                    x: 0.5 - x,
                    y: 0.5 - y,
                }),
                Some(TouchPoint {
                    id: 0,
                    x: 0.5 + x,
                    y: 0.5 + y,
                }),
            ];
            mapper.update(&input);
        }
        assert!(mapper.wheel_remainder < -40.0);
        input.touch = [None, None];
        mapper.update(&input);
        assert_eq!(mapper.previous_angle, None);
    }

    #[test]
    fn layer_only_modifier_stays_silent_and_deepest_layer_wins() {
        let mut layered = binding(0, 66);
        layered.modifier = Some(4);
        layered.layer_only = true;
        let mut deeper = binding(0, 68);
        deeper.modifier = Some(4);
        deeper.extra_modifiers = vec![5];
        let mapper = mapper_with(vec![binding(0, 65), layered, deeper, binding(4, 67)]);
        let mut input = blank_input();
        input.buttons[4] = 1.0;
        // The modifier's own base key never flashes, even before the layer button is pressed.
        assert!(mapper.active_keys(&input).is_empty());
        input.buttons[0] = 1.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([66]));
        input.buttons[5] = 1.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([68]));
    }

    #[test]
    fn toggle_layer_latches_on_press_edges() {
        let mut layered = binding(0, 66);
        layered.modifier = Some(4);
        layered.toggle_layer = true;
        let mut mapper = mapper_with(vec![binding(0, 65), layered]);
        let mut input = blank_input();
        input.buttons[4] = 1.0;
        mapper.update_latches(&input);
        input.buttons[4] = 0.0;
        mapper.update_latches(&input);
        input.buttons[0] = 1.0;
        assert_eq!(mapper.active_keys(&input), HashSet::from([66]));
        input.buttons[4] = 1.0;
        mapper.update_latches(&input);
        input.buttons[4] = 0.0;
        mapper.update_latches(&input);
        assert_eq!(mapper.active_keys(&input), HashSet::from([65]));
    }

    #[test]
    fn long_press_and_double_tap_triggers_use_hold_time_and_release_gap() {
        let ms = Duration::from_millis;
        assert!(Trigger::Press.met(true, None, None));
        assert!(!Trigger::Long(300).met(true, Some(ms(299)), None));
        assert!(Trigger::Long(300).met(true, Some(ms(300)), None));
        assert!(!Trigger::Long(300).met(false, Some(ms(500)), None));
        assert!(!Trigger::Double(250).met(true, Some(ms(10)), None));
        assert!(Trigger::Double(250).met(true, Some(ms(10)), Some(ms(200))));
        assert!(!Trigger::Double(250).met(true, Some(ms(10)), Some(ms(400))));

        let mut tracker = GestureTracker::default();
        let t0 = Instant::now();
        let mut down = [false; 23];
        down[3] = true;
        tracker.update_down(&down, t0);
        down[3] = false;
        tracker.update_down(&down, t0 + ms(50));
        down[3] = true;
        tracker.update_down(&down, t0 + ms(150));
        assert_eq!(tracker.gap(3), Some(ms(100)));
        assert_eq!(tracker.held_at(3, t0 + ms(200)), Some(ms(50)));
    }

    #[test]
    fn turbo_alternates_key_state_while_held() {
        let mut fire = binding(0, 65);
        fire.turbo_ms = 50;
        let mut mapper = mapper_with(vec![fire]);
        let t0 = Instant::now();
        let mut down = [false; 23];
        down[0] = true;
        mapper.gestures.update_down(&down, t0);
        let input = {
            let mut i = blank_input();
            i.buttons[0] = 1.0;
            i
        };
        let selected = mapper.active_bindings(&input);
        assert_eq!(
            mapper
                .active_keys_from(&selected, t0 + Duration::from_millis(10))
                .len(),
            1
        );
        assert!(
            mapper
                .active_keys_from(&selected, t0 + Duration::from_millis(60))
                .is_empty()
        );
        assert_eq!(
            mapper
                .active_keys_from(&selected, t0 + Duration::from_millis(110))
                .len(),
            1
        );
    }

    #[test]
    fn pointer_acceleration_is_normalised_by_report_interval() {
        let ms = Duration::from_millis;
        assert_eq!(pointer_rate_scale(None), 1.0);
        assert_eq!(pointer_rate_scale(Some(ms(0))), 1.0);
        assert_eq!(pointer_rate_scale(Some(ms(4))), 1.0);
        assert!((pointer_rate_scale(Some(ms(10))) - 0.4).abs() < 1e-6);
        assert_eq!(pointer_rate_scale(Some(ms(500))), 0.1);
    }

    #[test]
    fn pending_deeper_long_press_holds_back_shallower_binding() {
        let mut layered = binding(0, 66);
        layered.modifier = Some(4);
        layered.trigger = Trigger::Long(400);
        layered.consume = true;
        let mapper = mapper_with(vec![binding(0, 65), layered]);
        let mut input = blank_input();
        input.buttons[4] = 1.0;
        input.buttons[0] = 1.0;
        let mut down = [false; 23];
        down[0] = true;
        down[4] = true;
        let mut mapper = mapper;
        // Just pressed: the layered long-press is pending, so the base key must not fire.
        mapper.gestures.update_down(&down, Instant::now());
        assert!(mapper.active_keys(&input).is_empty());
        assert_eq!(mapper.pending_bindings(&input), vec![1]);
        // Held long enough: the layered binding fires alone.
        mapper.gestures = GestureTracker::default();
        mapper
            .gestures
            .update_down(&down, Instant::now() - Duration::from_millis(500));
        assert_eq!(mapper.active_keys(&input), HashSet::from([66]));
        assert!(mapper.pending_bindings(&input).is_empty());
        // Without the layer modifier nothing is pending and the base key fires normally.
        input.buttons[4] = 0.0;
        down[4] = false;
        mapper.gestures = GestureTracker::default();
        mapper.gestures.update_down(&down, Instant::now());
        assert_eq!(mapper.active_keys(&input), HashSet::from([65]));
    }

    #[test]
    fn binding_validation_rejects_bad_layers_triggers_and_conflicts() {
        assert!(binding(0, 65).validate().is_ok());
        assert!(binding(30, 65).validate().is_err());
        assert!(binding(0, 0).validate().is_err());
        let mut b = binding(0, 65);
        b.modifier = Some(0);
        assert!(b.validate().is_err(), "modifier equals bound button");
        let mut b = binding(0, 65);
        b.layer_only = true;
        assert!(b.validate().is_err(), "layer-only needs a modifier");
        let mut b = binding(0, 65);
        b.trigger = Trigger::Long(10);
        assert!(b.validate().is_err());
        let mut b = binding(0, 65);
        b.turbo_ms = 100;
        assert!(b.validate().is_ok());
        b.behavior = RemapBehavior::Tap;
        assert!(b.validate().is_err(), "turbo needs a held key");
        let mut long = binding(0, 66);
        long.trigger = Trigger::Long(400);
        assert!(RemapBinding::validate_set(&[binding(0, 65), long.clone()]).is_err());
        assert!(RemapBinding::validate_set(&[binding(1, 65), long]).is_ok());
    }

    #[test]
    fn extras_build_a_trigger_and_old_json_still_loads() {
        let old: RemapExtras = serde_json::from_str("{}").unwrap();
        assert_eq!(old, RemapExtras::default());
        let extras: RemapExtras =
            serde_json::from_str(r#"{"modifiers":[5],"longPressMs":400,"consume":true}"#).unwrap();
        let b = extras.apply(binding(0, 65));
        assert_eq!(b.trigger, Trigger::Long(400));
        assert_eq!(b.extra_modifiers, vec![5]);
        assert!(b.consume);
    }

    #[test]
    fn pointer_tracker_filters_jumps_carries_fractions_and_accelerates() {
        let p = |x: f32| TouchPoint { id: 0, x, y: 0.5 };
        let settings = PointerSettings::default();
        let mut tracker = PointerTracker::default();
        assert_eq!(tracker.step(p(0.5), &settings), None);
        assert_eq!(tracker.step(p(0.9), &settings), None, "contact jump");
        // 0.0004 * 1250 = 0.5 px: nothing on the first report, one pixel after the second.
        assert_eq!(tracker.step(p(0.9004), &settings), None);
        assert_eq!(tracker.step(p(0.9009), &settings), Some([1, 0]));
        let fast = PointerSettings {
            sensitivity: 1.0,
            acceleration: 1.0,
        };
        let mut a = PointerTracker::default();
        let mut b = PointerTracker::default();
        a.step(p(0.5), &settings);
        b.step(p(0.5), &fast);
        let slow = a.step(p(0.52), &settings).unwrap()[0];
        let quick = b.step(p(0.52), &fast).unwrap()[0];
        assert!(quick > slow);
        assert!(
            PointerSettings {
                sensitivity: f32::NAN,
                acceleration: 9.0
            }
            .sanitized()
            .acceleration
                <= 1.0
        );
    }

    #[test]
    fn abandoned_edge_gesture_returns_to_pointer_but_a_swipe_does_not() {
        let start = TouchPoint {
            id: 0,
            x: 0.04,
            y: 0.5,
        };
        let mut gesture = EdgeGesture {
            started: Instant::now(),
            left: true,
            start,
            last: start,
        };
        assert!(!gesture.abandoned());
        gesture.last.x = 0.2;
        assert!(!gesture.abandoned(), "a qualifying swipe stays anchored");
        gesture.last = TouchPoint {
            id: 0,
            x: 0.04,
            y: 0.9,
        };
        assert!(gesture.abandoned(), "far vertical drag is pointer motion");
        gesture.last = start;
        gesture.started = Instant::now() - Duration::from_millis(800);
        assert!(gesture.abandoned(), "window expired");
    }

    #[test]
    fn gyro_gate_modes_and_stick_pause() {
        let mut mapper = GyroAimMapper::default();
        mapper.config.activation = GyroActivation::L1;
        let mut input = blank_input();
        mapper.config.gate = GyroGate::Hold;
        assert!(!mapper.gate_open(&input));
        input.buttons[4] = 1.0;
        assert!(mapper.gate_open(&input));
        mapper.config.gate = GyroGate::Disable;
        assert!(!mapper.gate_open(&input));
        input.buttons[4] = 0.0;
        assert!(mapper.gate_open(&input));
        mapper.config.gate = GyroGate::Toggle;
        input.buttons[4] = 1.0;
        assert!(mapper.gate_open(&input));
        assert!(mapper.gate_open(&input), "held button does not retoggle");
        input.buttons[4] = 0.0;
        assert!(mapper.gate_open(&input));
        input.buttons[4] = 1.0;
        assert!(!mapper.gate_open(&input));
        mapper.config.activation = GyroActivation::Always;
        mapper.config.stick_off_threshold = 0.4;
        input.axes[2] = 0.8;
        assert!(!mapper.gate_open(&input));
        input.axes[2] = 0.1;
        assert!(mapper.gate_open(&input));
        assert_eq!(GyroGate::from_label("ratchet"), Some(GyroGate::Disable));
    }

    #[test]
    fn gyro_stick_mix_scales_and_clamps() {
        assert_eq!(
            mix_gyro_into_stick([0.0, 0.0], [50.0, -25.0], 100.0, 1.0),
            [0.5, -0.25]
        );
        assert_eq!(
            mix_gyro_into_stick([0.8, 0.0], [100.0, 0.0], 100.0, 1.0),
            [1.0, 0.0]
        );
        assert_eq!(
            mix_gyro_into_stick([0.3, 0.3], [9.0, 9.0], 0.0, 1.0),
            [0.3, 0.3]
        );
    }

    #[test]
    fn windows_scancode_and_extended_key_tables() {
        for vk in [
            0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22, 0x6F, 0xA3, 0xA5,
        ] {
            assert!(key_is_extended(vk), "{vk:#x}");
        }
        for vk in [0x41, 0x0D, 0x20, 0xA2, 0xA4, 0x70] {
            assert!(!key_is_extended(vk), "{vk:#x}");
        }
        assert!(key_uses_scancode(0x41, 0x1E));
        assert!(!key_uses_scancode(0x41, 0));
        assert!(
            !key_uses_scancode(0x67, 0x47),
            "numpad stays on virtual keys"
        );
        assert!(
            !key_uses_scancode(0xAF, 0x30),
            "media stays on virtual keys"
        );
    }

    #[test]
    fn wasd_uses_hysteresis() {
        assert_eq!(wasd_state([false; 4], [0.0, -0.4]), [false; 4]);
        assert_eq!(
            wasd_state([false; 4], [0.0, -0.6]),
            [true, false, false, false]
        );
        assert_eq!(
            wasd_state([true, false, false, false], [0.0, -0.4]),
            [true, false, false, false]
        );
        assert_eq!(
            wasd_state([true, false, false, false], [0.0, -0.3]),
            [false; 4]
        );
        assert_eq!(
            wasd_state([false; 4], [0.9, 0.9]),
            [false, false, true, true]
        );
    }

    #[test]
    fn disabled_flick_stays_off_but_flick_without_gyro_runs() {
        let mut mapper = GyroAimMapper::default();
        let settings = controls::MotionSettings {
            flick_enabled: true,
            counts_per_turn: 3600.0,
            ..Default::default()
        };
        let mut input = blank_input();
        input.accel = [0, 0, 8192];
        let mut processor = motion::MotionProcessor::default();
        let mut total = 0;
        for tick in 0..30u32 {
            input.axes[2] = if tick == 2 { 1.0 } else { 0.0 };
            input.sensor_timestamp = Some(tick * 30_000);
            let reading = processor.update(&input, Instant::now());
            total += mapper.compute(&input, &reading, &settings, false)[0];
        }
        assert_eq!(total, 0, "gyro aim off and flick_without_gyro off");
        mapper.config.flick_without_gyro = true;
        mapper.set_config(mapper.config.clone());
        for tick in 30..90u32 {
            input.axes[2] = if tick == 33 { 1.0 } else { 0.0 };
            input.sensor_timestamp = Some(tick * 30_000);
            let reading = processor.update(&input, Instant::now());
            total += mapper.compute(&input, &reading, &settings, false)[0];
        }
        assert!((total - 900).abs() <= 1, "{total}");
    }
}
