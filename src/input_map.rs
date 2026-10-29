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
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pointer_sensitivity: f32,
    reverse_scroll: bool,
    touch_previous: Option<TouchPoint>,
    scroll_last: Option<Instant>,
    wheel_remainder: f32,
    launches: Vec<PcLaunchBinding>,
    launch_held: HashSet<u8>,
    last_launch: HashMap<u8, Instant>,
    launch_error: Option<String>,
    programs: Vec<(u8, crate::macros::Program)>,
    program_held: HashSet<u8>,
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
            pointer_sensitivity: 0.7,
            reverse_scroll: true,
            touch_previous: None,
            scroll_last: None,
            wheel_remainder: 0.0,
            launches: Vec::new(),
            launch_held: HashSet::new(),
            last_launch: HashMap::new(),
            launch_error: None,
            programs: Vec::new(),
            program_held: HashSet::new(),
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
    pub fn stop_macros(&mut self) {
        let output = self.player.stop();
        self.perform(output);
        self.keyboard.set_suspended(true);
        self.wait_release = true;
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
        self.pointer_sensitivity = pointer_sensitivity.clamp(0.4, 1.5);
        self.reverse_scroll = reverse_scroll;
        self.launches = launches;
        self.wait_release = true;
    }

    pub fn tick(&mut self) {
        if self.enabled && !self.ui_focused && self.player.active() {
            let output = self.player.tick(Instant::now());
            self.perform(output);
            if !self.player.active() {
                self.wait_release = true;
            }
        } else {
            self.keyboard.tick();
        }
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
        self.touch_previous = None;
        self.scroll_last = None;
        self.wheel_remainder = 0.0;
        self.launch_held.clear();
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled || self.ui_focused {
            return;
        }
        let down = input.buttons.map(|value| value > 0.5);
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
            .find(|(button, _)| down[*button as usize] && !self.program_held.contains(button))
            .map(|(_, program)| program.clone());
        self.program_held = self
            .programs
            .iter()
            .filter(|(button, _)| down[*button as usize])
            .map(|(button, _)| *button)
            .collect();
        if let Some(program) = triggered {
            self.release_all();
            match self.player.start(program, now) {
                Ok(output) => self.perform(output),
                Err(e) => self.launch_error = Some(e.into()),
            }
        }
        if self.player.active() {
            self.tick();
            return;
        }
        self.keyboard.update(input);
        for launch in &self.launches {
            if down[launch.button as usize] {
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
            [Some(_), Some(_)] => {
                self.touch_previous = None;
            }
            [Some(point), None] | [None, Some(point)] => {
                if let Some(previous) = self.touch_previous {
                    let delta_x = point.x - previous.x;
                    let delta_y = point.y - previous.y;
                    let dx = (delta_x * 1250.0 * self.pointer_sensitivity).round() as i32;
                    let dy = (delta_y * 780.0 * self.pointer_sensitivity).round() as i32;
                    if delta_x.abs() <= 0.2 && delta_y.abs() <= 0.2 && (dx != 0 || dy != 0) {
                        let dx = dx.clamp(-36, 36);
                        let dy = dy.clamp(-36, 36);
                        send_mouse(dx, dy, 0, MOUSEEVENTF_MOVE);
                    }
                }
                self.touch_previous = Some(point);
            }
            [None, None] => {
                self.touch_previous = None;
            }
        }
        let now = Instant::now();
        let elapsed = self.scroll_last.replace(now).map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f32().min(0.05)
        });
        let wheel = self.scroll_delta(input.axes[3], elapsed);
        if wheel != 0 {
            send_mouse(0, 0, wheel, MOUSEEVENTF_WHEEL);
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum GyroActivation {
    #[default]
    Always,
    L2,
    R2,
    L1,
    R1,
    Touchpad,
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

    fn active(&self, input: &InputState) -> bool {
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

#[derive(Default)]
pub struct GyroAimMapper {
    pub config: GyroAimConfig,
    remainder_x: f32,
    remainder_y: f32,
    flick: controls::FlickState,
    filtered: [f32; 2],
}

impl GyroAimMapper {
    pub fn set_config(&mut self, config: GyroAimConfig) {
        self.config = config;
        self.remainder_x = 0.0;
        self.remainder_y = 0.0;
        self.flick.reset();
        self.filtered = [0.0; 2];
    }

    pub fn compute(
        &mut self,
        input: &InputState,
        motion: &motion::MotionReading,
        settings: &controls::MotionSettings,
        suspended: bool,
    ) -> [i32; 2] {
        if self.config.enabled
            && motion.valid
            && !suspended
            && !input.panic_combo
            && motion.clock == "duplicate"
        {
            return [0; 2];
        }
        if !self.config.enabled
            || !motion.valid
            || suspended
            || input.panic_combo
            || motion.dt <= 0.0
        {
            self.remainder_x = 0.0;
            self.remainder_y = 0.0;
            self.filtered = [0.0; 2];
            self.flick.reset();
            return [0; 2];
        }
        let dt = motion.dt;
        let active = self.config.activation.active(input);
        let axes = motion::aim_axes(motion, settings.space == "world");
        let deadzone = f32::from(self.config.deadzone) / 1024.0;
        let target = if active {
            axes.map(|v| v.signum() * (v.abs() - deadzone).max(0.0))
        } else {
            [0.0; 2]
        };
        let tau = 0.002 + self.config.smoothing.clamp(0.0, 0.85) * 0.065;
        let blend = 1.0 - (-dt / tau).exp();
        if active {
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
        let scale = settings.counts_per_turn / 360.0;
        let flick = if settings.flick_enabled {
            self.flick
                .update([input.axes[2], input.axes[3]], dt, settings)
        } else {
            self.flick.reset();
            0.0
        };
        let dx = (self.filtered[0] * dt * self.config.yaw_scale * self.config.sensitivity * aim
            + flick)
            * scale;
        let dy = self.filtered[1]
            * dt
            * self.config.pitch_scale
            * self.config.sensitivity
            * aim
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemapBinding {
    pub button: u8,
    pub vk: u16,
    pub modifier: Option<u8>,
    pub behavior: RemapBehavior,
    pub sequence: Vec<u16>,
    pub chord: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemapBehavior {
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

#[derive(Default)]
pub struct RemapMapper {
    enabled: bool,
    suspended: bool,
    bindings: Vec<RemapBinding>,
    keys_down: HashSet<u16>,
    taps_down: HashSet<(u8, Option<u8>)>,
    macro_queue: VecDeque<u16>,
    macro_held: Option<(u16, Instant)>,
    next_macro_at: Option<Instant>,
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
        self.taps_down.clear();
        self.macro_queue.clear();
        self.next_macro_at = None;
    }

    pub fn update(&mut self, input: &InputState) {
        if !self.enabled || self.suspended {
            return;
        }
        let selected = self.active_bindings(input);
        let active = self.active_keys(input);
        let mut releasing: Vec<_> = self.keys_down.difference(&active).copied().collect();
        releasing.sort_by_key(|vk| matches!(vk,16..=18|91|92|160..=165));
        for vk in &releasing {
            if self.macro_held.is_none_or(|(held, _)| held != *vk) {
                send_key(*vk, true);
            }
        }
        let mut pressing: Vec<_> = active.difference(&self.keys_down).copied().collect();
        pressing.sort_by_key(|vk| !matches!(vk,16..=18|91|92|160..=165));
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
            let identity = (binding.button, binding.modifier);
            taps_down.insert(identity);
            if self.taps_down.contains(&identity) {
                continue;
            }
            if binding.behavior == RemapBehavior::Tap {
                let keys = if binding.chord.is_empty() {
                    vec![binding.vk]
                } else {
                    binding.chord.clone()
                };
                for key in &keys {
                    if !self.keys_down.contains(key) {
                        send_key(*key, false);
                    }
                }
                for key in keys.iter().rev() {
                    if !self.keys_down.contains(key) {
                        send_key(*key, true);
                    }
                }
            } else if self.macro_queue.len() + binding.sequence.len() < 32 {
                self.macro_queue.push_back(binding.vk);
                self.macro_queue.extend(binding.sequence.iter().copied());
            }
        }
        self.taps_down = taps_down;
        self.advance_macro();
    }

    pub fn tick(&mut self) {
        if self.enabled && !self.suspended {
            self.advance_macro();
        }
    }

    fn active_bindings(&self, input: &InputState) -> Vec<usize> {
        let active_chords: Vec<&RemapBinding> = self
            .bindings
            .iter()
            .filter(|binding| {
                binding
                    .modifier
                    .is_some_and(|modifier| input.buttons[modifier as usize] > 0.5)
                    && input.buttons[binding.button as usize] > 0.5
            })
            .collect();
        let chorded: HashSet<u8> = active_chords.iter().map(|binding| binding.button).collect();
        let engaged_modifiers: HashSet<u8> = active_chords
            .iter()
            .filter_map(|binding| binding.modifier)
            .collect();
        self.bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| {
                input.buttons[binding.button as usize] > 0.5
                    && (binding.modifier.is_some() || !engaged_modifiers.contains(&binding.button))
                    && binding
                        .modifier
                        .map_or(!chorded.contains(&binding.button), |modifier| {
                            input.buttons[modifier as usize] > 0.5
                        })
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn active_keys(&self, input: &InputState) -> HashSet<u16> {
        self.active_bindings(input)
            .iter()
            .map(|&index| &self.bindings[index])
            .filter(|binding| binding.behavior == RemapBehavior::Hold)
            .flat_map(|binding| {
                if binding.chord.is_empty() {
                    vec![binding.vk]
                } else {
                    binding.chord.clone()
                }
            })
            .collect()
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
            self.next_macro_at = Some(now + Duration::from_millis(20));
        }
        if self.next_macro_at.is_some_and(|at| now < at) {
            return;
        }
        if let Some(vk) = self.macro_queue.pop_front() {
            if !self.keys_down.contains(&vk) {
                send_key(vk, false);
            }
            self.macro_held = Some((vk, now + Duration::from_millis(45)));
        }
    }
}

#[derive(Default)]
pub struct MouseMapper {
    enabled: bool,
    gestures: bool,
    previous: Option<TouchPoint>,
    previous_scroll: Option<f32>,
    previous_angle: Option<f32>,
    wheel_remainder: f32,
    left_down: bool,
    edge_anchor: Option<EdgeGesture>,
    previous_zone: Option<TouchZone>,
    last_zone_tick: Option<Instant>,
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

struct EdgeGesture {
    started: Instant,
    left: bool,
    start: TouchPoint,
    last: TouchPoint,
}

impl EdgeGesture {
    fn arrow_key(&self) -> Option<u16> {
        if self.started.elapsed() > Duration::from_millis(750)
            || (self.last.y - self.start.y).abs() > 0.12
        {
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
}

impl MouseMapper {
    pub fn cancel_pointer(&mut self) {
        if self.left_down {
            send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
        }
        self.left_down = false;
        self.previous = None;
        self.previous_scroll = None;
        self.previous_angle = None;
        self.edge_anchor = None;
        self.previous_zone = None;
        self.last_zone_tick = None;
        self.wheel_remainder = 0.0;
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled && self.left_down {
            send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
        }
        self.enabled = enabled;
        self.previous = None;
        self.previous_scroll = None;
        self.previous_angle = None;
        self.wheel_remainder = 0.0;
        self.left_down = false;
        self.edge_anchor = None;
        self.previous_zone = None;
        self.last_zone_tick = None;
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
        let cue = if self.gestures {
            self.update_zone(input.touch)
        } else {
            None
        };
        match input.touch {
            [Some(a), Some(b)] => {
                self.previous = None;
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
                    self.previous = None;
                } else if self.gestures
                    && self.previous.is_none()
                    && (point.x <= 0.12 || point.x >= 0.88)
                {
                    self.edge_anchor = Some(EdgeGesture {
                        started: Instant::now(),
                        left: point.x <= 0.12,
                        start: point,
                        last: point,
                    });
                    self.previous = None;
                } else if point.y >= 0.12 {
                    if let Some(previous) = self.previous {
                        let dx = ((point.x - previous.x) * 1250.0).round() as i32;
                        let dy = ((point.y - previous.y) * 780.0).round() as i32;
                        let dx = dx.clamp(-36, 36);
                        let dy = dy.clamp(-36, 36);
                        if dx != 0 || dy != 0 {
                            send_mouse(dx, dy, 0, MOUSEEVENTF_MOVE);
                        }
                    }
                    self.previous = Some(point);
                } else {
                    // Top strip clutch: leaving it starts a fresh drag.
                    self.previous = None;
                    self.edge_anchor = None;
                }
            }
            [None, None] => {
                if self.gestures
                    && let Some(gesture) = self.edge_anchor.take()
                    && let Some(vk) = gesture.arrow_key()
                {
                    tap_key(vk);
                }
                self.previous = None;
                self.previous_scroll = None;
                self.previous_angle = None;
            }
        }
        let should_press = input.buttons[17] > 0.5 && input.touch.iter().any(Option::is_some);
        if should_press != self.left_down {
            send_mouse(
                0,
                0,
                0,
                if should_press {
                    MOUSEEVENTF_LEFTDOWN
                } else {
                    MOUSEEVENTF_LEFTUP
                },
            );
            self.left_down = should_press;
        }
        cue
    }
}

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
    command.args(args).spawn()?;
    Ok(())
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

#[cfg(windows)]
pub(super) fn send_key(vk: u16, up: bool) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
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

fn tap_key(vk: u16) {
    send_key(vk, false);
    send_key(vk, true);
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mapper = RemapMapper {
            enabled: true,
            bindings: vec![
                RemapBinding {
                    button: 0,
                    vk: 65,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                    chord: vec![],
                },
                RemapBinding {
                    button: 1,
                    vk: 65,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                    chord: vec![],
                },
            ],
            ..Default::default()
        };
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
        let mapper = RemapMapper {
            enabled: true,
            bindings: vec![
                RemapBinding {
                    button: 0,
                    vk: 75,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                    chord: vec![17, 16, 75],
                },
                RemapBinding {
                    button: 1,
                    vk: 17,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                    chord: vec![],
                },
            ],
            ..RemapMapper::default()
        };
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
        let mapper = RemapMapper {
            enabled: true,
            bindings: vec![
                RemapBinding {
                    button: 0,
                    vk: 65,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                    chord: vec![],
                },
                RemapBinding {
                    button: 0,
                    vk: 66,
                    modifier: Some(4),
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                    chord: vec![],
                },
                RemapBinding {
                    button: 4,
                    vk: 67,
                    modifier: None,
                    behavior: RemapBehavior::Hold,
                    sequence: vec![],
                    chord: vec![],
                },
            ],
            ..Default::default()
        };
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
}
