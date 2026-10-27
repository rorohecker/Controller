//! A bounded macro clock. Execution stays on the input thread; cancellation releases every key.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Program {
    pub steps: Vec<Step>,
    pub repeat: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Step {
    Chord {
        keys: Vec<u16>,
        #[serde(rename = "holdMs")]
        hold_ms: u16,
    },
    Wait {
        ms: u16,
    },
    Click {
        button: String,
        #[serde(rename = "holdMs")]
        hold_ms: u16,
    },
    Scroll {
        amount: i16,
    },
    Launch {
        path: String,
        args: Vec<String>,
    },
}

impl Program {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.steps.is_empty() || self.steps.len() > 64 || !(1..=10).contains(&self.repeat) {
            return Err("Use 1–64 macro steps and 1–10 repeats");
        }
        let mut duration = 0u32;
        for step in &self.steps {
            duration += match step {
                Step::Chord { keys, hold_ms } => {
                    if keys.is_empty()
                        || keys.len() > 8
                        || keys.iter().any(|k| !(1..=254).contains(k))
                        || keys.iter().collect::<std::collections::HashSet<_>>().len() != keys.len()
                        || !(20..=5000).contains(hold_ms)
                    {
                        return Err("Use up to eight distinct keys held for 20–5000 ms");
                    }
                    u32::from(*hold_ms) + 20
                }
                Step::Wait { ms } if (10..=10000).contains(ms) => u32::from(*ms),
                Step::Click { button, hold_ms }
                    if matches!(button.as_str(), "left" | "right")
                        && (20..=5000).contains(hold_ms) =>
                {
                    u32::from(*hold_ms) + 20
                }
                Step::Scroll { amount } if (-20..=20).contains(amount) && *amount != 0 => 20,
                Step::Launch { path, args }
                    if !path.is_empty()
                        && path.len() <= 1024
                        && !path.contains('\0')
                        && valid_args(args)
                        && std::path::Path::new(path).is_absolute() =>
                {
                    200
                }
                _ => return Err("Invalid macro step"),
            };
        }
        if duration * u32::from(self.repeat) > 60_000 {
            return Err("A macro must finish within 60 seconds");
        }
        Ok(())
    }
}

pub fn valid_args(args: &[String]) -> bool {
    args.len() <= 16 && args.iter().all(|a| a.len() <= 1024 && !a.contains('\0'))
}

#[derive(Debug, PartialEq)]
pub enum Output {
    Key(u16, bool),
    Mouse(String, bool),
    Scroll(i16),
    Launch(String, Vec<String>),
}

#[derive(Default)]
pub struct Player {
    program: Option<Program>,
    index: usize,
    iteration: u8,
    due: Option<Instant>,
    release: Vec<Output>,
}
impl Player {
    pub fn active(&self) -> bool {
        self.program.is_some()
    }
    pub fn start(&mut self, program: Program, now: Instant) -> Result<Vec<Output>, &'static str> {
        program.validate()?;
        let release = self.stop();
        self.program = Some(program);
        self.index = 0;
        self.iteration = 0;
        self.due = Some(now);
        Ok(release)
    }
    pub fn stop(&mut self) -> Vec<Output> {
        self.program = None;
        self.due = None;
        self.index = 0;
        self.iteration = 0;
        std::mem::take(&mut self.release)
    }
    pub fn tick(&mut self, now: Instant) -> Vec<Output> {
        if self.due.is_none_or(|due| now < due) {
            return Vec::new();
        }
        if !self.release.is_empty() {
            self.due = Some(now + Duration::from_millis(20));
            return std::mem::take(&mut self.release);
        }
        let Some(program) = &self.program else {
            return Vec::new();
        };
        if self.index == program.steps.len() {
            self.iteration += 1;
            if self.iteration >= program.repeat {
                return self.stop();
            }
            self.index = 0;
        }
        let step = program.steps[self.index].clone();
        self.index += 1;
        let (output, delay) = match step {
            Step::Chord { mut keys, hold_ms } => {
                keys.sort_by_key(|vk| !matches!(vk,16..=18|91|92|160..=165));
                self.release = keys.iter().rev().map(|k| Output::Key(*k, true)).collect();
                (
                    keys.into_iter().map(|k| Output::Key(k, false)).collect(),
                    u64::from(hold_ms),
                )
            }
            Step::Click { button, hold_ms } => {
                self.release = vec![Output::Mouse(button.clone(), true)];
                (vec![Output::Mouse(button, false)], u64::from(hold_ms))
            }
            Step::Wait { ms } => (Vec::new(), u64::from(ms)),
            Step::Scroll { amount } => (vec![Output::Scroll(amount)], 20),
            Step::Launch { path, args } => (vec![Output::Launch(path, args)], 200),
        };
        self.due = Some(now + Duration::from_millis(delay));
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_releases_chord_in_reverse_and_stops_repeats() {
        let mut p = Player::default();
        let now = Instant::now();
        p.start(
            Program {
                steps: vec![Step::Chord {
                    keys: vec![75, 17],
                    hold_ms: 100,
                }],
                repeat: 3,
            },
            now,
        )
        .unwrap();
        assert_eq!(
            p.tick(now),
            vec![Output::Key(17, false), Output::Key(75, false)]
        );
        assert!(p.tick(now + Duration::from_millis(99)).is_empty());
        assert_eq!(p.stop(), vec![Output::Key(75, true), Output::Key(17, true)]);
        assert!(p.tick(now + Duration::from_secs(1)).is_empty());
    }
    #[test]
    fn repeat_and_wait_follow_clock_without_busy_waiting() {
        let mut p = Player::default();
        let t = Instant::now();
        p.start(
            Program {
                steps: vec![
                    Step::Click {
                        button: "left".into(),
                        hold_ms: 20,
                    },
                    Step::Wait { ms: 10 },
                ],
                repeat: 2,
            },
            t,
        )
        .unwrap();
        assert_eq!(p.tick(t), vec![Output::Mouse("left".into(), false)]);
        assert_eq!(
            p.tick(t + Duration::from_millis(20)),
            vec![Output::Mouse("left".into(), true)]
        );
        assert!(p.tick(t + Duration::from_millis(40)).is_empty());
        assert_eq!(
            p.tick(t + Duration::from_millis(50)),
            vec![Output::Mouse("left".into(), false)]
        );
        p.tick(t + Duration::from_millis(70));
        p.tick(t + Duration::from_millis(90));
        p.tick(t + Duration::from_millis(100));
        assert!(!p.active());
    }
    #[test]
    fn rejects_unbounded_duration_duplicate_keys_and_shell_free_launch_errors() {
        assert!(
            Program {
                steps: vec![Step::Wait { ms: 10000 }],
                repeat: 10
            }
            .validate()
            .is_err()
        );
        assert!(
            Program {
                steps: vec![Step::Chord {
                    keys: vec![17, 17],
                    hold_ms: 20
                }],
                repeat: 1
            }
            .validate()
            .is_err()
        );
        assert!(!valid_args(&["bad\0arg".into()]));
        assert!(
            Program {
                steps: vec![Step::Launch {
                    path: "relative.exe".into(),
                    args: vec![]
                }],
                repeat: 1
            }
            .validate()
            .is_err()
        );
    }
}
