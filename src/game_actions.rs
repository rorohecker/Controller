use aftertouch_protocol::TriggerSetting;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRule {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub button: u8,
    #[serde(default)]
    pub event: String,
    #[serde(default = "default_window")]
    pub window_ms: u16,
    #[serde(default = "default_cooldown")]
    pub cooldown_ms: u16,
    #[serde(default)]
    pub condition_button: Option<u8>,
    #[serde(default = "default_threshold")]
    pub condition_threshold: f32,
    #[serde(default)]
    pub cancel_on_release: bool,
    #[serde(default)]
    pub cancel_event: String,
    #[serde(default = "default_retrigger")]
    pub retrigger: String,
    pub duration_ms: u16,
    #[serde(default)]
    pub haptic: Option<ActionHaptic>,
    #[serde(default)]
    pub left_trigger: Option<TriggerSetting>,
    #[serde(default)]
    pub right_trigger: Option<TriggerSetting>,
    #[serde(default)]
    pub timeline: Option<crate::audio_haptics::authoring::EffectClip>,
}
fn default_window() -> u16 {
    280
}
fn default_cooldown() -> u16 {
    180
}
fn default_threshold() -> f32 {
    0.5
}
fn default_retrigger() -> String {
    "restart".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActionHaptic {
    pub shape: String,
    pub frequency: u16,
    pub strength: u8,
    #[serde(default)]
    pub pan: i16,
}

impl ActionRule {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_empty()
            || self.id.len() > 80
            || self.name.trim().is_empty()
            || self.name.chars().count() > 40
            || !matches!(self.source.as_str(), "press" | "double" | "hold" | "event")
            || self.button >= 23
            || !(80..=1000).contains(&self.window_ms)
            || self.cooldown_ms > 10_000
            || self.condition_button.is_some_and(|b| b >= 23)
            || !self.condition_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.condition_threshold)
            || self.cancel_event.len() > 80
            || !matches!(self.retrigger.as_str(), "restart" | "ignore")
            || !(20..=2000).contains(&self.duration_ms)
            || self.event.len() > 80
            || (self.source == "event" && self.event.trim().is_empty())
            || (self.timeline.is_none()
                && self.haptic.is_none()
                && self.left_trigger.is_none()
                && self.right_trigger.is_none())
        {
            return Err("Choose a name, input, and feel for each game action");
        }
        if let Some(clip) = &self.timeline {
            clip.validate()?;
            if self.haptic.is_some() || self.left_trigger.is_some() || self.right_trigger.is_some()
            {
                return Err("Choose a timeline or a simple action feel");
            }
        }
        if let Some(h) = &self.haptic
            && (!matches!(h.shape.as_str(), "sine" | "square" | "saw" | "noise")
                || !(20..=300).contains(&h.frequency)
                || h.strength > 100
                || !(-100..=100).contains(&h.pan))
        {
            return Err("Action haptic settings are out of range");
        }
        for trigger in [&self.left_trigger, &self.right_trigger]
            .into_iter()
            .flatten()
        {
            trigger.validate()?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct ActionMapper {
    rules: Vec<ActionRule>,
    down: [bool; 23],
    started: HashMap<u8, Instant>,
    taps: HashMap<u8, Instant>,
    pending: Vec<(String, u8, Instant)>,
    held: HashSet<String>,
    fired: HashMap<String, Instant>,
    active: HashMap<String, Instant>,
    cancelled: Vec<String>,
    values: [f32; 23],
}

impl ActionMapper {
    pub fn set(&mut self, rules: Vec<ActionRule>) -> Result<(), &'static str> {
        if rules.len() > 24 {
            return Err("A profile can contain up to 24 actions");
        }
        let mut ids = HashSet::new();
        for rule in &rules {
            rule.validate()?;
            if !ids.insert(&rule.id) {
                return Err("Action IDs must be distinct");
            }
        }
        *self = Self {
            rules,
            ..Self::default()
        };
        Ok(())
    }
    fn fire(&mut self, index: usize, now: Instant, output: &mut Vec<ActionRule>) {
        let rule = &self.rules[index];
        if rule
            .condition_button
            .is_some_and(|b| self.values[usize::from(b)] < rule.condition_threshold)
            || (rule.cancel_on_release
                && rule.source != "event"
                && self.values[usize::from(rule.button)] <= 0.5)
            || (rule.retrigger == "ignore"
                && self.active.get(&rule.id).is_some_and(|until| now < *until))
        {
            return;
        }
        if self.fired.get(&rule.id).is_none_or(|last| {
            now.saturating_duration_since(*last)
                >= Duration::from_millis(u64::from(rule.cooldown_ms))
        }) {
            self.fired.insert(rule.id.clone(), now);
            self.active.insert(
                rule.id.clone(),
                now + Duration::from_millis(
                    rule.timeline
                        .as_ref()
                        .map_or(u64::from(rule.duration_ms), |t| u64::from(t.duration_ms())),
                ),
            );
            output.push(rule.clone());
        }
    }
    pub fn event(&mut self, event: &str, now: Instant) -> Vec<ActionRule> {
        let mut output = Vec::new();
        for rule in &self.rules {
            if !rule.cancel_event.is_empty()
                && rule.cancel_event.eq_ignore_ascii_case(event)
                && self.active.remove(&rule.id).is_some()
            {
                self.cancelled.push(rule.id.clone());
            }
        }
        for i in 0..self.rules.len() {
            if self.rules[i].source == "event" && self.rules[i].event.eq_ignore_ascii_case(event) {
                self.fire(i, now, &mut output);
            }
        }
        output
    }
    pub fn update(&mut self, buttons: &[f32; 23], now: Instant) -> Vec<ActionRule> {
        self.values = *buttons;
        self.active.retain(|_, until| now < *until);
        for rule in &self.rules {
            let released = rule.cancel_on_release
                && rule.source != "event"
                && self.down[usize::from(rule.button)]
                && buttons[usize::from(rule.button)] <= 0.5;
            let blocked = rule
                .condition_button
                .is_some_and(|b| buttons[usize::from(b)] < rule.condition_threshold);
            if (released || blocked) && self.active.remove(&rule.id).is_some() {
                self.cancelled.push(rule.id.clone());
            }
        }
        let mut output = Vec::new();
        for (button, value) in buttons.iter().enumerate() {
            let button = button as u8;
            let down = *value > 0.5;
            if down && !self.down[usize::from(button)] {
                self.started.insert(button, now);
                let doubles: Vec<usize> = self
                    .rules
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.button == button && r.source == "double")
                    .map(|(i, _)| i)
                    .collect();
                let matched: Vec<usize> = doubles
                    .iter()
                    .copied()
                    .filter(|i| {
                        self.taps.get(&button).is_some_and(|at| {
                            now.saturating_duration_since(*at)
                                <= Duration::from_millis(u64::from(self.rules[*i].window_ms))
                        })
                    })
                    .collect();
                if !matched.is_empty() {
                    self.pending.retain(|(_, b, _)| *b != button);
                    self.taps.remove(&button);
                    for i in matched {
                        self.fire(i, now, &mut output);
                    }
                } else {
                    self.taps.insert(button, now);
                    let delay = doubles.iter().map(|i| self.rules[*i].window_ms).max();
                    for i in 0..self.rules.len() {
                        if self.rules[i].button == button && self.rules[i].source == "press" {
                            if let Some(ms) = delay {
                                self.pending.push((
                                    self.rules[i].id.clone(),
                                    button,
                                    now + Duration::from_millis(u64::from(ms)),
                                ));
                            } else {
                                self.fire(i, now, &mut output);
                            }
                        }
                    }
                }
            }
            if down {
                for i in 0..self.rules.len() {
                    let r = &self.rules[i];
                    if r.button == button
                        && r.source == "hold"
                        && !self.held.contains(&r.id)
                        && self.started.get(&button).is_some_and(|at| {
                            now.saturating_duration_since(*at)
                                >= Duration::from_millis(u64::from(r.window_ms))
                        })
                    {
                        self.held.insert(r.id.clone());
                        self.fire(i, now, &mut output);
                    }
                }
            } else {
                self.started.remove(&button);
                for r in &self.rules {
                    if r.button == button {
                        self.held.remove(&r.id);
                    }
                }
            }
            self.down[usize::from(button)] = down;
        }
        let ready: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, _, at)| now >= *at)
            .map(|(id, _, _)| id.clone())
            .collect();
        self.pending.retain(|(_, _, at)| now < *at);
        for id in ready {
            if let Some(i) = self.rules.iter().position(|r| r.id == id) {
                self.fire(i, now, &mut output);
            }
        }
        output
    }
    pub fn take_cancelled(&mut self) -> Vec<String> {
        std::mem::take(&mut self.cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rule(id: &str, source: &str) -> ActionRule {
        ActionRule {
            id: id.into(),
            name: id.into(),
            source: source.into(),
            button: 1,
            event: "dash".into(),
            window_ms: 280,
            cooldown_ms: 0,
            condition_button: None,
            condition_threshold: 0.5,
            cancel_on_release: false,
            cancel_event: String::new(),
            retrigger: default_retrigger(),
            duration_ms: 120,
            haptic: Some(ActionHaptic {
                shape: "sine".into(),
                frequency: 80,
                strength: 40,
                pan: 0,
            }),
            left_trigger: None,
            right_trigger: None,
            timeline: None,
        }
    }
    #[test]
    fn timeline_retrigger_uses_the_actual_clip_duration() {
        let now = Instant::now();
        let mut action = rule("timeline", "event");
        action.haptic = None;
        action.retrigger = "ignore".into();
        action.timeline=Some(serde_json::from_value(serde_json::json!({"id":"clip","name":"Clip","cues":[{"lane":"rightGrip","kind":"pulse","startMs":0,"durationMs":600,"strength":30.0}]})).unwrap());
        let mut mapper = ActionMapper::default();
        mapper.set(vec![action]).unwrap();
        assert_eq!(mapper.event("dash", now).len(), 1);
        assert!(
            mapper
                .event("dash", now + Duration::from_millis(200))
                .is_empty()
        );
        assert_eq!(
            mapper.event("dash", now + Duration::from_millis(601)).len(),
            1
        );
    }
    #[test]
    fn conditions_cancellation_and_retrigger_respect_live_input() {
        let now = Instant::now();
        let mut r = rule("aimed", "event");
        r.condition_button = Some(6);
        r.condition_threshold = 0.75;
        r.retrigger = "ignore".into();
        r.cancel_event = "cancel".into();
        let mut mapper = ActionMapper::default();
        mapper.set(vec![r]).unwrap();
        assert!(mapper.event("dash", now).is_empty());
        let mut buttons = [0.; 23];
        buttons[6] = 0.8;
        mapper.update(&buttons, now);
        assert_eq!(mapper.event("dash", now).len(), 1);
        assert!(
            mapper
                .event("dash", now + Duration::from_millis(10))
                .is_empty()
        );
        assert_eq!(
            mapper.event("dash", now + Duration::from_millis(121)).len(),
            1
        );
        mapper.event("cancel", now + Duration::from_millis(122));
        assert_eq!(mapper.take_cancelled(), vec!["aimed"]);
        mapper.event("dash", now + Duration::from_millis(123));
        buttons[6] = 0.7;
        mapper.update(&buttons, now + Duration::from_millis(124));
        assert_eq!(mapper.take_cancelled(), vec!["aimed"]);
    }
    #[test]
    fn release_cancels_effect_and_delayed_press_does_not_fire_after_release() {
        let now = Instant::now();
        let mut press = rule("press", "press");
        press.cancel_on_release = true;
        let mut mapper = ActionMapper::default();
        mapper.set(vec![press.clone()]).unwrap();
        let mut buttons = [0.; 23];
        buttons[1] = 1.;
        assert_eq!(mapper.update(&buttons, now).len(), 1);
        buttons[1] = 0.;
        mapper.update(&buttons, now + Duration::from_millis(10));
        assert_eq!(mapper.take_cancelled(), vec!["press"]);
        mapper.set(vec![press, rule("double", "double")]).unwrap();
        buttons[1] = 1.;
        mapper.update(&buttons, now);
        buttons[1] = 0.;
        mapper.update(&buttons, now + Duration::from_millis(10));
        assert!(
            mapper
                .update(&buttons, now + Duration::from_millis(300))
                .is_empty()
        );
        let mut bad = rule("bad", "event");
        bad.condition_threshold = f32::NAN;
        assert!(bad.validate().is_err());
        bad.condition_threshold = 0.5;
        bad.retrigger = "queue_forever".into();
        assert!(bad.validate().is_err());
    }
    #[test]
    fn action_names_count_characters_instead_of_utf8_bytes() {
        let mut r = rule("unicode", "press");
        r.name = "弓".repeat(40);
        assert!(r.validate().is_ok());
        r.name.push('弓');
        assert!(r.validate().is_err());
    }
    #[test]
    fn double_tap_suppresses_the_single_action_and_hold_fires_once() {
        let now = Instant::now();
        let mut m = ActionMapper::default();
        m.set(vec![
            rule("roll", "press"),
            rule("dash", "double"),
            rule("charge", "hold"),
        ])
        .unwrap();
        let mut b = [0.; 23];
        b[1] = 1.;
        assert!(m.update(&b, now).is_empty());
        b[1] = 0.;
        m.update(&b, now + Duration::from_millis(50));
        b[1] = 1.;
        assert_eq!(
            m.update(&b, now + Duration::from_millis(150))[0].name,
            "dash"
        );
        assert_eq!(
            m.update(&b, now + Duration::from_millis(450))[0].name,
            "charge"
        );
        assert!(m.update(&b, now + Duration::from_millis(700)).is_empty());
        b[1] = 0.;
        m.update(&b, now + Duration::from_millis(750));
        b[1] = 1.;
        m.update(&b, now + Duration::from_millis(850));
        b[1] = 0.;
        m.update(&b, now + Duration::from_millis(900));
        assert_eq!(
            m.update(&b, now + Duration::from_millis(1140))[0].name,
            "roll"
        );
    }
    #[test]
    fn event_actions_have_cooldown_and_invalid_rules_do_not_replace_working_rules() {
        let now = Instant::now();
        let mut m = ActionMapper::default();
        let mut r = rule("hit", "event");
        r.cooldown_ms = 200;
        m.set(vec![r]).unwrap();
        assert_eq!(m.event("dash", now).len(), 1);
        assert!(m.event("dash", now + Duration::from_millis(50)).is_empty());
        let mut bad = rule("bad", "event");
        bad.duration_ms = 5000;
        assert!(m.set(vec![bad]).is_err());
        assert_eq!(m.event("dash", now + Duration::from_millis(250)).len(), 1);
    }
}
