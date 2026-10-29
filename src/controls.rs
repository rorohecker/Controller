//! Per-game output transforms and pure touch selection state.
use aftertouch_protocol::InputState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct MotionSettings {
    pub counts_per_turn: f32,
    pub space: String,
    pub aim_multiplier: f32,
    pub aim_button: u8,
    pub flick_enabled: bool,
    pub flick_time_ms: u16,
    pub flick_threshold: f32,
}
impl Default for MotionSettings {
    fn default() -> Self {
        Self {
            counts_per_turn: 101.25,
            space: "local".into(),
            aim_multiplier: 1.0,
            aim_button: 6,
            flick_enabled: false,
            flick_time_ms: 100,
            flick_threshold: 0.85,
        }
    }
}
impl MotionSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.counts_per_turn.is_finite()
            || !(50.0..=100_000.0).contains(&self.counts_per_turn)
            || !matches!(self.space.as_str(), "local" | "world")
            || !self.aim_multiplier.is_finite()
            || !(0.05..=4.0).contains(&self.aim_multiplier)
            || self.aim_button >= 23
            || !(20..=250).contains(&self.flick_time_ms)
            || !self.flick_threshold.is_finite()
            || !(0.5..=0.99).contains(&self.flick_threshold)
        {
            return Err("Motion sensitivity or flick settings are out of range");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct StickCurve {
    pub mode: String,
    pub inner: f32,
    pub outer: f32,
    pub exponent: f32,
    pub anti_deadzone: f32,
}
impl Default for StickCurve {
    fn default() -> Self {
        Self {
            mode: "axial".into(),
            inner: 0.08,
            outer: 1.0,
            exponent: 1.0,
            anti_deadzone: 0.0,
        }
    }
}
impl StickCurve {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.mode.as_str(), "axial" | "radial")
            || !self.inner.is_finite()
            || !(0.0..=0.4).contains(&self.inner)
            || !self.outer.is_finite()
            || !(0.6..=1.0).contains(&self.outer)
            || !self.exponent.is_finite()
            || !(0.25..=3.0).contains(&self.exponent)
            || !self.anti_deadzone.is_finite()
            || !(0.0..=0.3).contains(&self.anti_deadzone)
        {
            return Err("Stick curve settings are out of range");
        }
        Ok(())
    }
    fn amount(&self, value: f32) -> f32 {
        if !value.is_finite() || value <= self.inner {
            return 0.0;
        }
        let normalized = ((value - self.inner) / (self.outer - self.inner))
            .clamp(0.0, 1.0)
            .powf(self.exponent);
        self.anti_deadzone + (1.0 - self.anti_deadzone) * normalized
    }
    pub fn apply(&self, pair: [f32; 2]) -> [f32; 2] {
        let pair = pair.map(|v| {
            if v.is_finite() {
                v.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        });
        if self.mode == "axial" {
            pair.map(|v| v.signum() * self.amount(v.abs()))
        } else {
            let radius = pair[0].hypot(pair[1]);
            let value = self.amount(radius.min(1.0));
            if radius > 0.0 {
                pair.map(|v| v / radius * value)
            } else {
                [0.0; 2]
            }
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct StickSettings {
    pub left: StickCurve,
    pub right: StickCurve,
}
impl StickSettings {
    pub fn apply(&self, axes: [f32; 4]) -> [f32; 4] {
        let l = self.left.apply([axes[0], axes[1]]);
        let r = self.right.apply([axes[2], axes[3]]);
        [l[0], l[1], r[0], r[1]]
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RadialSector {
    pub label: String,
    #[serde(default)]
    pub chord: Vec<u16>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct RadialSettings {
    pub enabled: bool,
    pub mode: String,
    pub activation_button: u8,
    pub deadzone: f32,
    pub feedback: bool,
    pub sectors: Vec<RadialSector>,
}
impl Default for RadialSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "pc".into(),
            activation_button: 17,
            deadzone: 0.2,
            feedback: true,
            sectors: (1..=8)
                .map(|i| RadialSector {
                    label: format!("Slot {i}"),
                    chord: vec![],
                })
                .collect(),
        }
    }
}
impl RadialSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.mode.as_str(), "pc" | "game" | "both")
            || self.activation_button >= 23
            || matches!(self.activation_button, 1 | 6 | 7 | 16 | 18)
            || !self.deadzone.is_finite()
            || !(0.1..=0.5).contains(&self.deadzone)
            || !(4..=8).contains(&self.sectors.len())
            || self.sectors.iter().any(|s| {
                s.label.trim().is_empty()
                    || s.label.chars().count() > 24
                    || s.label.contains(char::is_control)
                    || s.chord.len() > 4
                    || s.chord.iter().any(|k| !(1..=254).contains(k))
                    || s.chord
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        != s.chord.len()
            })
        {
            return Err("Radial menu needs 4–8 named sectors and up to four keys per sector");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlProfile {
    pub motion: MotionSettings,
    pub sticks: StickSettings,
    pub radial: RadialSettings,
}
impl ControlProfile {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.motion.validate()?;
        self.sticks.left.validate()?;
        self.sticks.right.validate()?;
        self.radial.validate()
    }
}

#[derive(Default)]
pub struct FlickState {
    angle: Option<f32>,
    pending: f32,
    remaining: f32,
    neutral: bool,
}
impl FlickState {
    pub fn reset(&mut self) {
        *self = Self {
            neutral: true,
            ..Default::default()
        };
    }
    pub fn update(&mut self, stick: [f32; 2], dt: f32, settings: &MotionSettings) -> f32 {
        if dt <= 0.0 || dt > 0.05 {
            self.reset();
            return 0.0;
        }
        let radius = stick[0].hypot(stick[1]);
        if self.neutral {
            if radius < 0.3 {
                self.neutral = false;
            } else {
                return 0.0;
            }
        }
        let mut turn = 0.0;
        if radius >= settings.flick_threshold {
            let angle = stick[0].atan2(-stick[1]).to_degrees();
            if let Some(previous) = self.angle {
                turn += (angle - previous + 180.0).rem_euclid(360.0) - 180.0;
            } else {
                self.pending += angle;
                self.remaining = f32::from(settings.flick_time_ms) / 1000.0;
            }
            self.angle = Some(angle);
        } else if radius < settings.flick_threshold * 0.8 {
            self.angle = None;
        }
        if self.remaining > 0.0 {
            let fraction = (dt / self.remaining).min(1.0);
            let step = self.pending * fraction;
            turn += step;
            self.pending -= step;
            self.remaining = (self.remaining - dt).max(0.0);
        }
        turn
    }
}

#[derive(Default)]
pub struct RadialState {
    pub open: bool,
    pub selected: Option<usize>,
    contact: Option<u8>,
    wait_lift: bool,
    armed: bool,
}
pub struct RadialOutput {
    pub consume: bool,
    pub selected: Option<usize>,
    pub changed: bool,
    pub committed: Option<usize>,
    pub cancelled: bool,
}
impl RadialState {
    pub fn reset(&mut self) {
        *self = Self {
            wait_lift: true,
            ..Default::default()
        };
    }
    pub fn update(
        &mut self,
        input: &InputState,
        settings: &RadialSettings,
        allowed: bool,
    ) -> RadialOutput {
        let held = input.buttons[usize::from(settings.activation_button)] > 0.5;
        let point = input.touch[0].or(input.touch[1]);
        let point_count = input.touch.iter().filter(|p| p.is_some()).count();
        let previous = self.selected;
        let was_open = self.open;
        let mut committed = None;
        let mut cancelled = false;
        if self.wait_lift {
            if !held && point_count == 0 {
                self.wait_lift = false;
                self.armed = true;
            }
            return RadialOutput {
                consume: true,
                selected: None,
                changed: was_open,
                committed: None,
                cancelled: false,
            };
        }
        if !held {
            self.armed = true;
        }
        if !allowed
            || !settings.enabled
            || input.panic_combo
            || input.buttons[1] > 0.5
            || point_count > 1
        {
            cancelled = self.open;
            self.open = false;
            self.selected = None;
            self.contact = None;
            if cancelled {
                self.wait_lift = true;
            }
        } else if self.open {
            if point_count != 1 || self.contact != point.map(|p| p.id) {
                self.open = false;
                self.selected = None;
                self.wait_lift = true;
                cancelled = true;
            }
        } else if self.armed && held && point_count == 1 {
            self.open = true;
            self.contact = Some(point.unwrap().id);
            self.armed = false;
        }
        if self.open {
            let point = point.unwrap();
            let dx = (point.x - 0.5) * 3.56;
            let dy = (point.y - 0.5) * 2.0;
            if dx.hypot(dy) < settings.deadzone {
                self.selected = None;
            } else {
                let angle = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU);
                let step = std::f32::consts::TAU / settings.sectors.len() as f32;
                let candidate = ((angle / step).round() as usize) % settings.sectors.len();
                let keep = self.selected.is_some_and(|i| {
                    let center = i as f32 * step;
                    let distance = (angle - center + std::f32::consts::PI)
                        .rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI;
                    distance.abs() < step * 0.5 + 8f32.to_radians()
                });
                if !keep {
                    self.selected = Some(candidate);
                }
            }
            // Use the release packet's contact position, including the center
            // cancel region, rather than committing the previous report's sector.
            if !held {
                committed = self.selected;
                self.open = false;
                self.selected = None;
                self.wait_lift = true;
            }
        }
        RadialOutput {
            consume: self.open || self.wait_lift,
            selected: self.selected,
            changed: was_open != self.open || previous != self.selected,
            committed,
            cancelled,
        }
    }
}

pub struct RadialMapper {
    pub settings: RadialSettings,
    pub state: RadialState,
    player: crate::macros::Player,
    focused: bool,
    pc_mode: bool,
    last_cue: Option<std::time::Instant>,
    capture: bool,
}
impl Default for RadialMapper {
    fn default() -> Self {
        Self {
            settings: RadialSettings::default(),
            state: RadialState::default(),
            player: crate::macros::Player::default(),
            focused: true,
            pc_mode: false,
            last_cue: None,
            capture: false,
        }
    }
}
impl RadialMapper {
    fn perform(output: Vec<crate::macros::Output>) {
        for value in output {
            if let crate::macros::Output::Key(key, up) = value {
                super::send_key(key, up);
            }
        }
    }
    pub fn cancel(&mut self) {
        self.state.reset();
        Self::perform(self.player.stop());
    }
    pub fn set(&mut self, settings: RadialSettings) -> Result<(), &'static str> {
        settings.validate()?;
        if settings.enabled
            && settings
                .sectors
                .iter()
                .flat_map(|s| &s.chord)
                .any(|key| !super::supported_key(*key))
        {
            return Err("A radial shortcut contains an unsupported key on this OS");
        }
        self.cancel();
        self.settings = settings;
        Ok(())
    }
    pub fn focus(&mut self, focused: bool) {
        if self.focused != focused {
            self.cancel();
            self.focused = focused;
        }
    }
    pub fn tick(&mut self) {
        if self.focused {
            Self::perform(self.player.stop());
        } else {
            Self::perform(self.player.tick(std::time::Instant::now()));
        }
    }
    pub fn update(
        &mut self,
        input: &InputState,
        pc_mode: bool,
        focused: bool,
    ) -> (RadialOutput, bool) {
        self.focus(focused);
        if self.pc_mode != pc_mode {
            self.cancel();
            self.pc_mode = pc_mode;
        }
        let allowed = focused
            || self.settings.mode == "both"
            || if pc_mode {
                self.settings.mode == "pc"
            } else {
                self.settings.mode == "game"
            };
        let mut output = self.state.update(input, &self.settings, allowed);
        output.changed |= self.capture != output.consume;
        self.capture = output.consume;
        if let Some(index) = output.committed
            && !focused
            && !self.settings.sectors[index].chord.is_empty()
        {
            let mut keys = self.settings.sectors[index].chord.clone();
            keys.sort_by_key(|k| !matches!(*k,16..=18|91|92|160..=165));
            let program = crate::macros::Program {
                repeat: 1,
                steps: vec![crate::macros::Step::Chord { keys, hold_ms: 40 }],
            };
            if let Ok(release) = self.player.start(program, std::time::Instant::now()) {
                Self::perform(release);
                self.tick();
            }
        }
        let cue = self.settings.feedback
            && output.changed
            && output.selected.is_some()
            && self
                .last_cue
                .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(100));
        if cue {
            self.last_cue = Some(std::time::Instant::now());
        }
        (output, cue)
    }
}
impl Drop for RadialMapper {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> InputState {
        let mut input = aftertouch_protocol::parse_input(&[1; 64]).unwrap();
        input.axes = [0.0; 4];
        input.buttons = [0.0; 23];
        input.touch = [None, None];
        input
    }
    fn point(id: u8, x: f32, y: f32) -> aftertouch_protocol::TouchPoint {
        aftertouch_protocol::TouchPoint { id, x, y }
    }
    #[test]
    fn default_virtual_curve_preserves_legacy_deadzone_without_double_processing() {
        let curve = StickCurve::default();
        for x in [-1.0f32, -0.5, -0.08, 0.0, 0.08, 0.5, 1.0] {
            let expected = if x.abs() <= 0.08 {
                0.0
            } else {
                x.signum() * (x.abs() - 0.08) / 0.92
            };
            assert!((curve.apply([x, 0.0])[0] - expected).abs() < 0.00001);
        }
        let radial = StickCurve {
            mode: "radial".into(),
            inner: 0.1,
            outer: 0.9,
            exponent: 2.0,
            anti_deadzone: 0.1,
        };
        assert_eq!(radial.apply([0.05, 0.05]), [0.0; 2]);
        let output = radial.apply([1.0, 1.0]);
        assert!((output[0].hypot(output[1]) - 1.0).abs() < 0.00001);
        assert!(radial.apply([f32::NAN, 0.0]).iter().all(|v| v.is_finite()));
        assert!(
            StickCurve {
                exponent: 0.0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn flick_completes_after_a_tap_and_wraps_short_turns() {
        let settings = MotionSettings::default();
        let mut flick = FlickState::default();
        flick.reset();
        flick.update([0.0; 2], 0.01, &settings);
        let mut angle = flick.update([1.0, 0.0], 0.01, &settings);
        for _ in 0..12 {
            angle += flick.update([0.0; 2], 0.01, &settings);
        }
        assert!((angle - 90.0).abs() < 0.0001);
        flick.reset();
        assert_eq!(flick.update([1.0, 0.0], 0.01, &settings), 0.0); // held stick does not rearm after focus/config changes
        flick.update([0.0; 2], 0.01, &settings);
        let a = 179f32.to_radians();
        let b = (-179f32).to_radians();
        flick.update([a.sin(), -a.cos()], 0.01, &settings);
        for _ in 0..12 {
            flick.update([a.sin(), -a.cos()], 0.01, &settings);
        }
        assert!((flick.update([b.sin(), -b.cos()], 0.01, &settings) - 2.0).abs() < 0.0001);
    }
    #[test]
    fn radial_commits_on_button_release_and_waits_for_touch_lift() {
        let settings = RadialSettings {
            enabled: true,
            ..Default::default()
        };
        let mut state = RadialState::default();
        let mut input = input();
        state.update(&input, &settings, true);
        input.buttons[17] = 1.0;
        input.touch[0] = Some(point(3, 0.5, 0.5));
        assert!(state.update(&input, &settings, true).consume);
        assert_eq!(state.selected, None);
        input.touch[0] = Some(point(3, 0.5, 0.0));
        assert_eq!(state.update(&input, &settings, true).selected, Some(0));
        input.buttons[17] = 0.0;
        let selected = state.update(&input, &settings, true);
        assert_eq!(selected.committed, Some(0));
        assert!(selected.consume);
        assert!(state.update(&input, &settings, true).consume);
        input.touch = [None, None];
        state.update(&input, &settings, true);
        assert!(!state.update(&input, &settings, true).consume);
    }
    #[test]
    fn radial_cancels_second_fingers_contact_changes_circle_and_blocked_scope() {
        let settings = RadialSettings {
            enabled: true,
            ..Default::default()
        };
        for reason in 0..4 {
            let mut state = RadialState::default();
            let mut input = input();
            state.update(&input, &settings, true);
            input.buttons[17] = 1.0;
            input.touch[0] = Some(point(1, 1.0, 0.5));
            state.update(&input, &settings, true);
            match reason {
                0 => input.touch[1] = Some(point(2, 0.0, 0.5)),
                1 => input.touch[0] = Some(point(2, 1.0, 0.5)),
                2 => input.buttons[1] = 1.0,
                _ => {}
            }
            let output = state.update(&input, &settings, reason != 3);
            assert!(output.cancelled);
            assert_eq!(output.committed, None);
            assert!(!state.open);
        }
    }
    #[test]
    fn radial_release_uses_current_touch_position() {
        let settings = RadialSettings {
            enabled: true,
            ..Default::default()
        };
        for (x, y, expected) in [(0.5, 0.5, None), (1.0, 0.5, Some(2))] {
            let mut state = RadialState::default();
            let mut input = input();
            state.update(&input, &settings, true);
            input.buttons[17] = 1.0;
            input.touch[0] = Some(point(1, 0.5, 0.0));
            assert_eq!(state.update(&input, &settings, true).selected, Some(0));
            input.buttons[17] = 0.0;
            input.touch[0] = Some(point(1, x, y));
            assert_eq!(state.update(&input, &settings, true).committed, expected);
        }
    }
}
