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
    pub duration_ms: u16,
    #[serde(default)]
    pub haptic: Option<ActionHaptic>,
    #[serde(default)]
    pub left_trigger: Option<TriggerSetting>,
    #[serde(default)]
    pub right_trigger: Option<TriggerSetting>,
}
fn default_window() -> u16 {
    280
}
fn default_cooldown() -> u16 {
    180
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
            || self.name.len() > 40
            || !matches!(self.source.as_str(), "press" | "double" | "hold" | "event")
            || self.button >= 23
            || !(80..=1000).contains(&self.window_ms)
            || self.cooldown_ms > 10_000
            || !(20..=2000).contains(&self.duration_ms)
            || self.event.len() > 80
            || (self.source == "event" && self.event.trim().is_empty())
            || (self.haptic.is_none()
                && self.left_trigger.is_none()
                && self.right_trigger.is_none())
        {
            return Err("Choose a name, input, and feel for each game action");
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
        if self.fired.get(&rule.id).is_none_or(|last| {
            now.saturating_duration_since(*last)
                >= Duration::from_millis(u64::from(rule.cooldown_ms))
        }) {
            self.fired.insert(rule.id.clone(), now);
            output.push(rule.clone());
        }
    }
    pub fn event(&mut self, event: &str, now: Instant) -> Vec<ActionRule> {
        let mut output = Vec::new();
        for i in 0..self.rules.len() {
            if self.rules[i].source == "event" && self.rules[i].event.eq_ignore_ascii_case(event) {
                self.fire(i, now, &mut output);
            }
        }
        output
    }
    pub fn update(&mut self, buttons: &[f32; 23], now: Instant) -> Vec<ActionRule> {
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
            duration_ms: 120,
            haptic: Some(ActionHaptic {
                shape: "sine".into(),
                frequency: 80,
                strength: 40,
                pan: 0,
            }),
            left_trigger: None,
            right_trigger: None,
        }
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
