//! Adapter observations are hints, never memory inspection or proof of a game event.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    pub allow_hud: bool,
    pub threshold: f32,
    pub confirmations: u8,
    pub stale_ms: u16,
    pub manual_hold_ms: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_hud: false,
            threshold: 0.9,
            confirmations: 2,
            stale_ms: 1500,
            manual_hold_ms: 0,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.threshold.is_finite()
            || !(0.5..=1.0).contains(&self.threshold)
            || !(1..=10).contains(&self.confirmations)
            || !(250..=10000).contains(&self.stale_ms)
            || self.manual_hold_ms > 300000
        {
            return Err(
                "Recognition needs 50-100% confidence, 1-10 confirmations, and a 250-10000 ms freshness limit",
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub weapon: String,
    pub source: String,
    pub confidence: f32,
    #[serde(default)]
    pub age_ms: u32,
    #[serde(default)]
    pub adapter: String,
}
impl Observation {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.weapon.trim().is_empty()
            || self.weapon.len() > 80
            || self.weapon.contains(char::is_control)
            || !matches!(self.source.as_str(), "telemetry" | "hud")
            || !self.confidence.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || self.age_ms > 60000
            || self.adapter.len() > 80
            || self.adapter.contains(char::is_control)
        {
            return Err("Invalid weapon observation");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub candidate: Option<String>,
    pub status: String,
    pub weapon: Option<String>,
    pub source: Option<String>,
    pub confidence: Option<f32>,
    pub adapter: Option<String>,
    pub clear: bool,
}
#[derive(Default, Debug)]
pub struct Recognizer {
    candidate: Option<(String, u8, Instant)>,
    current: Option<(String, Instant)>,
    manual: Option<(String, Option<Instant>)>,
    last_status: String,
    last_report: Option<Instant>,
}
impl Recognizer {
    pub fn reconfigure(&mut self) {
        self.candidate = None;
        self.last_status.clear();
        self.last_report = None;
    }
    fn report(
        &mut self,
        status: &str,
        observation: Option<&Observation>,
        clear: bool,
        now: Instant,
    ) -> Option<Status> {
        if !clear
            && self.last_status == status
            && self
                .last_report
                .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_millis(250))
        {
            return None;
        }
        self.last_status = status.into();
        self.last_report = Some(now);
        Some(Status {
            candidate: observation.map(|o| o.weapon.clone()),
            status: status.into(),
            weapon: self
                .manual
                .as_ref()
                .map(|m| m.0.clone())
                .or_else(|| self.current.as_ref().map(|c| c.0.clone())),
            source: observation.map(|o| o.source.clone()),
            confidence: observation.map(|o| o.confidence),
            adapter: observation.map(|o| o.adapter.clone()),
            clear,
        })
    }
    pub fn manual(
        &mut self,
        weapon: Option<String>,
        policy: &Policy,
        now: Instant,
    ) -> Result<Status, &'static str> {
        if weapon
            .as_ref()
            .is_some_and(|w| w.trim().is_empty() || w.len() > 80 || w.contains(char::is_control))
        {
            return Err("Choose a valid manual weapon name");
        }
        self.candidate = None;
        self.current = None;
        self.manual = weapon.map(|w| {
            (
                w,
                (policy.manual_hold_ms > 0)
                    .then(|| now + Duration::from_millis(u64::from(policy.manual_hold_ms))),
            )
        });
        let name = if self.manual.is_some() {
            "manual"
        } else {
            "waiting"
        };
        self.last_status.clear();
        Ok(self.report(name, None, false, now).unwrap())
    }
    pub fn tick(&mut self, policy: &Policy, now: Instant) -> Option<Status> {
        if self.manual.is_none() && !policy.enabled && self.current.take().is_some() {
            return self.report("disabled", None, true, now);
        }
        if self
            .manual
            .as_ref()
            .is_some_and(|m| m.1.is_some_and(|end| now >= end))
        {
            self.manual = None;
            self.candidate = None;
            return self.report("waiting", None, true, now);
        }
        if self.manual.is_none()
            && self.current.as_ref().is_some_and(|c| {
                now.saturating_duration_since(c.1)
                    > Duration::from_millis(u64::from(policy.stale_ms))
            })
        {
            self.current = None;
            self.candidate = None;
            return self.report("stale", None, true, now);
        }
        None
    }
    pub fn observe(
        &mut self,
        observation: Option<Observation>,
        legacy: Option<String>,
        policy: &Policy,
        now: Instant,
    ) -> (Option<String>, Option<Status>) {
        if self.manual.is_some() {
            return (None, self.report("manual", None, false, now));
        }
        if !policy.enabled {
            return (
                legacy,
                observation
                    .as_ref()
                    .and_then(|o| self.report("disabled", Some(o), false, now)),
            );
        }
        let Some(o) = observation.or_else(|| {
            legacy.map(|weapon| Observation {
                weapon,
                source: "telemetry".into(),
                confidence: 1.,
                age_ms: 0,
                adapter: "legacy-json".into(),
            })
        }) else {
            return (None, None);
        };
        if o.validate().is_err() {
            self.candidate = None;
            return (None, self.report("invalid", None, false, now));
        }
        let reject = if o.source == "hud" && !policy.allow_hud {
            Some("hud-disabled")
        } else if o.age_ms > u32::from(policy.stale_ms) {
            Some("stale-frame")
        } else if o.confidence < policy.threshold {
            Some("low-confidence")
        } else {
            None
        };
        if let Some(status) = reject {
            self.candidate = None;
            return (None, self.report(status, Some(&o), false, now));
        }
        if let Some((name, count, last)) = &mut self.candidate
            && name == &o.weapon
            && now.saturating_duration_since(*last)
                <= Duration::from_millis(u64::from(policy.stale_ms))
        {
            *count = count.saturating_add(1);
            *last = now;
        } else {
            self.candidate = Some((o.weapon.clone(), 1, now));
        }
        if self.candidate.as_ref().unwrap().1 < policy.confirmations {
            return (None, self.report("confirming", Some(&o), false, now));
        }
        let changed = self.current.as_ref().is_none_or(|c| c.0 != o.weapon);
        self.current = Some((
            o.weapon.clone(),
            now.checked_sub(Duration::from_millis(u64::from(o.age_ms)))
                .unwrap_or(now),
        ));
        if changed {
            self.last_status.clear();
        }
        (
            changed.then(|| o.weapon.clone()),
            self.report("active", Some(&o), false, now),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn observation(name: &str) -> Observation {
        Observation {
            weapon: name.into(),
            source: "telemetry".into(),
            confidence: 1.,
            age_ms: 0,
            adapter: "test".into(),
        }
    }
    #[test]
    fn confidence_confirmation_and_stale_clear() {
        let p = Policy {
            enabled: true,
            ..Policy::default()
        };
        let t = Instant::now();
        let mut r = Recognizer::default();
        assert!(r.observe(Some(observation("smg")), None, &p, t).0.is_none());
        assert_eq!(
            r.observe(
                Some(observation("smg")),
                None,
                &p,
                t + Duration::from_millis(10)
            )
            .0
            .as_deref(),
            Some("smg")
        );
        let mut low = observation("sniper");
        low.confidence = 0.4;
        assert!(
            r.observe(Some(low), None, &p, t + Duration::from_millis(20))
                .0
                .is_none()
        );
        assert!(r.tick(&p, t + Duration::from_secs(2)).unwrap().clear);
    }
    #[test]
    fn manual_choice_holds_until_explicit_clear_and_hud_is_opt_in() {
        let p = Policy {
            enabled: true,
            confirmations: 1,
            ..Policy::default()
        };
        let t = Instant::now();
        let mut r = Recognizer::default();
        r.manual(Some("bow".into()), &p, t).unwrap();
        assert!(r.observe(Some(observation("smg")), None, &p, t).0.is_none());
        assert!(r.tick(&p, t + Duration::from_secs(3600)).is_none());
        r.manual(None, &p, t).unwrap();
        let mut hud = observation("sniper");
        hud.source = "hud".into();
        assert_eq!(
            r.observe(Some(hud), None, &p, t).1.unwrap().status,
            "hud-disabled"
        );
    }
    #[test]
    fn malformed_and_old_observations_do_not_switch() {
        let p = Policy {
            enabled: true,
            confirmations: 1,
            ..Policy::default()
        };
        let t = Instant::now();
        let mut r = Recognizer::default();
        let mut o = observation("smg");
        o.age_ms = 2000;
        assert!(r.observe(Some(o), None, &p, t).0.is_none());
        let mut bad = observation("smg");
        bad.confidence = f32::NAN;
        assert!(bad.validate().is_err());
        assert!(r.observe(Some(bad), None, &p, t).0.is_none());
    }
    #[test]
    fn reported_frame_age_counts_toward_expiry_and_manual_survives_policy_edits() {
        let p = Policy {
            enabled: true,
            confirmations: 1,
            ..Policy::default()
        };
        let t = Instant::now();
        let mut r = Recognizer::default();
        let mut o = observation("smg");
        o.age_ms = 1000;
        assert!(r.observe(Some(o), None, &p, t).0.is_some());
        assert!(r.tick(&p, t + Duration::from_millis(600)).unwrap().clear);
        r.manual(Some("bow".into()), &p, t).unwrap();
        r.reconfigure();
        assert!(r.observe(Some(observation("smg")), None, &p, t).0.is_none());
    }
}
