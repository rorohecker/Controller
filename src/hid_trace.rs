//! Bounded, opt-in input capture and pure replay analysis. No output API exists here.
use aftertouch_protocol as protocol;
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub const MAX_EVENTS: usize = 2048;
pub const MAX_JSON: usize = 1024 * 1024;
const MAX_US: u64 = 10_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Event {
    Connection {
        at_us: u64,
        transport: Option<String>,
    },
    Input {
        at_us: u64,
        bytes: Vec<u8>,
    },
    Timeout {
        at_us: u64,
    },
    ReadError {
        at_us: u64,
    },
    Truncated {
        at_us: u64,
        dropped_events: usize,
        transport: Option<String>,
    },
}
impl Event {
    fn at(&self) -> u64 {
        match self {
            Self::Connection { at_us, .. }
            | Self::Input { at_us, .. }
            | Self::Timeout { at_us }
            | Self::ReadError { at_us }
            | Self::Truncated { at_us, .. } => *at_us,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trace {
    pub schema_version: u8,
    pub events: Vec<Event>,
}

#[derive(Default)]
pub struct Recorder {
    started: Option<Instant>,
    events: std::collections::VecDeque<Event>,
    identity: Option<(String, String)>,
    before_window: Option<String>,
    dropped: usize,
}
impl Recorder {
    pub fn start(&mut self) {
        *self = Self {
            started: Some(Instant::now()),
            events: std::collections::VecDeque::with_capacity(MAX_EVENTS),
            ..Self::default()
        };
    }
    pub fn stop(&mut self) {
        self.started = None;
    }
    pub fn active(&self) -> bool {
        self.started.is_some()
    }
    pub fn len(&self) -> usize {
        self.events.len() + usize::from(self.dropped > 0)
    }
    pub fn snapshot(&self) -> Trace {
        let mut events = Vec::with_capacity(self.len());
        if self.dropped > 0 {
            events.push(Event::Truncated {
                at_us: self.events.front().map_or(0, Event::at),
                dropped_events: self.dropped,
                transport: self.before_window.clone(),
            });
        }
        events.extend(self.events.iter().cloned());
        Trace {
            schema_version: 1,
            events,
        }
    }
    fn now(&mut self) -> Option<u64> {
        let elapsed = self.started?.elapsed().as_micros() as u64;
        if elapsed > MAX_US {
            self.stop();
            None
        } else {
            Some(elapsed)
        }
    }
    fn push(&mut self, event: Event) {
        // Keep the most recent reports and lifecycle markers for the full session.
        if self.events.len() >= MAX_EVENTS - 1 {
            match self.events.pop_front().unwrap() {
                Event::Connection { transport, .. } => self.before_window = transport,
                Event::ReadError { .. } => self.before_window = None,
                _ => {}
            }
            self.dropped += 1;
        }
        self.events.push_back(event);
    }
    pub fn connection(&mut self, identity: Option<(String, String)>) {
        if !self.active() {
            return;
        }
        if self.events.is_empty() || self.identity != identity {
            if let Some(at_us) = self.now() {
                self.push(Event::Connection {
                    at_us,
                    transport: identity.as_ref().map(|(_, bus)| bus.clone()),
                });
            }
            self.identity = identity;
        }
    }
    pub fn input(&mut self, bytes: &[u8]) {
        if let Some(at_us) = self.now() {
            self.push(Event::Input {
                at_us,
                bytes: bytes.iter().copied().take(78).collect(),
            });
        }
    }
    pub fn timeout(&mut self) {
        if let Some(at_us) = self.now() {
            self.push(Event::Timeout { at_us });
        }
    }
    pub fn read_error(&mut self) {
        if let Some(at_us) = self.now() {
            self.push(Event::ReadError { at_us });
        }
    }
    pub fn tick(&mut self) {
        let _ = self.now();
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub dropped_events: usize,
    pub events: usize,
    pub valid_reports: usize,
    pub invalid_reports: usize,
    pub connections: usize,
    pub disconnects: usize,
    pub read_errors: usize,
    pub timeouts: usize,
    pub sequence_gaps: u64,
    pub duplicate_sequences: usize,
    pub stale_intervals: usize,
    pub interval_us: Vec<u64>,
    pub duration_us: u64,
}
pub fn analyze_json(json: &str) -> Result<Analysis, String> {
    if json.len() > MAX_JSON {
        return Err("Input trace exceeds 1 MiB.".into());
    }
    let trace: Trace = serde_json::from_str(json).map_err(|_| "Invalid input trace file.")?;
    analyze(&trace)
}
pub fn analyze_background(
    json: String,
    request_id: u32,
    proxy: std::sync::mpsc::Sender<serde_json::Value>,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);
    if json.len() > MAX_JSON {
        return Err("Input trace exceeds 1 MiB.".into());
    }
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("A trace is already being analyzed. Try again shortly.".into());
    }
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::Release);
        }
    }
    let slot = Slot;
    std::thread::Builder::new()
        .name("controller-input-replay".into())
        .spawn(move || {
            let _slot = slot;
            let event = match analyze_json(&json) {
                Ok(report) => {
                    serde_json::json!({"type":"hidReplay","requestId":request_id,"report":report})
                }
                Err(error) => {
                    serde_json::json!({"type":"hidReplay","requestId":request_id,"error":error})
                }
            };
            let _ = proxy.send(event);
        })
        .map(|_| ())
        .map_err(|e| format!("Could not start trace analysis: {e}"))
}
pub fn export_background(
    trace: Trace,
    proxy: std::sync::mpsc::Sender<serde_json::Value>,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("A trace export is already running.".into());
    }
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::Release);
        }
    }
    let slot = Slot;
    std::thread::Builder::new()
        .name("controller-input-export".into())
        .spawn(move || {
            let _slot = slot;
            let _ = proxy.send(serde_json::json!({"type":"hidTrace","trace":trace}));
        })
        .map(|_| ())
        .map_err(|e| format!("Could not start trace export: {e}"))
}
pub fn analyze(trace: &Trace) -> Result<Analysis, String> {
    if trace.schema_version != 1 || trace.events.len() > MAX_EVENTS {
        return Err("Unsupported trace version or too many events.".into());
    }
    let mut result = Analysis {
        events: trace.events.len(),
        ..Analysis::default()
    };
    let mut transport = None;
    let mut last_time = 0;
    let mut last_report = None;
    let mut sequence = None;
    for event in &trace.events {
        let at = event.at();
        if at < last_time || at > MAX_US {
            return Err("Trace timestamps must increase within ten seconds.".into());
        }
        last_time = at;
        match event {
            Event::Truncated {
                transport: bus,
                dropped_events,
                ..
            } => {
                if result.events > 0 && !std::ptr::eq(event, &trace.events[0]) {
                    return Err("Truncation marker must be first.".into());
                }
                if bus
                    .as_deref()
                    .is_some_and(|bus| !matches!(bus, "USB" | "Bluetooth"))
                {
                    return Err("Unknown trace transport.".into());
                }
                result.dropped_events = *dropped_events;
                transport = bus.as_deref();
                sequence = None;
                last_report = None;
            }
            Event::Connection { transport: bus, .. } => {
                if let Some(bus) = bus {
                    if !matches!(bus.as_str(), "USB" | "Bluetooth") {
                        return Err("Unknown trace transport.".into());
                    }
                    result.connections += 1;
                } else {
                    result.disconnects += 1;
                }
                transport = bus.as_deref();
                sequence = None;
                last_report = None;
            }
            Event::Input { bytes, .. } => {
                if bytes.len() > 78 {
                    return Err("Input reports cannot exceed 78 bytes.".into());
                }
                let shape = matches!(
                    (transport, bytes.len(), bytes.first()),
                    (Some("USB"), 64, Some(1))
                        | (Some("Bluetooth"), 78, Some(0x31))
                        | (Some("Bluetooth"), 10, Some(1))
                );
                if let Some(input) = shape.then(|| protocol::parse_input(bytes)).flatten() {
                    result.valid_reports += 1;
                    if let Some(previous) = last_report {
                        let interval = at - previous;
                        result.interval_us.push(interval);
                        if interval >= 250_000 {
                            result.stale_intervals += 1;
                        }
                    }
                    last_report = Some(at);
                    if let Some(next) = input.sequence {
                        if let Some(previous) = sequence {
                            let delta = next.wrapping_sub(previous);
                            if delta == 0 {
                                result.duplicate_sequences += 1;
                            } else if (2..128).contains(&delta) {
                                result.sequence_gaps += u64::from(delta - 1);
                            }
                        }
                        sequence = Some(next);
                    }
                } else {
                    result.invalid_reports += 1;
                }
            }
            Event::Timeout { .. } => {
                result.timeouts += 1;
            }
            Event::ReadError { .. } => {
                result.read_errors += 1;
                transport = None;
                sequence = None;
                last_report = None;
            }
        }
    }
    result.duration_us = last_time;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn usb(at_us: u64, sequence: u8) -> Event {
        let mut bytes = vec![0; 64];
        bytes[0] = 1;
        bytes[7] = sequence;
        Event::Input { at_us, bytes }
    }
    #[test]
    fn replay_wrap_reconnect_malformed_and_stale_input_without_outputs() {
        let trace = Trace {
            schema_version: 1,
            events: vec![
                Event::Connection {
                    at_us: 0,
                    transport: Some("USB".into()),
                },
                usb(1000, 255),
                usb(2000, 0),
                usb(3000, 3),
                Event::Input {
                    at_us: 4000,
                    bytes: vec![1; 63],
                },
                Event::Timeout { at_us: 5000 },
                usb(300_000, 4),
                Event::ReadError { at_us: 301_000 },
                Event::Connection {
                    at_us: 302_000,
                    transport: Some("USB".into()),
                },
                usb(303_000, 8),
            ],
        };
        let report = analyze(&trace).unwrap();
        assert_eq!(report.sequence_gaps, 2);
        assert_eq!(report.invalid_reports, 1);
        assert_eq!(report.valid_reports, 5);
        assert_eq!(report.connections, 2);
        assert_eq!(report.stale_intervals, 1);
        assert_eq!(report.read_errors, 1);
        assert_eq!(report.timeouts, 1);
    }
    #[test]
    fn bluetooth_crc_and_basic_reports_use_the_production_decoder() {
        let mut full = vec![0; 78];
        full[0] = 0x31;
        let crc = protocol::crc32_seeded(0xa1, &full[..74]);
        full[74..].copy_from_slice(&crc.to_le_bytes());
        let mut bad = full.clone();
        bad[5] ^= 1;
        let mut basic = vec![0; 10];
        basic[0] = 1;
        let trace = Trace {
            schema_version: 1,
            events: vec![
                Event::Connection {
                    at_us: 0,
                    transport: Some("Bluetooth".into()),
                },
                Event::Input {
                    at_us: 1,
                    bytes: full,
                },
                Event::Input {
                    at_us: 2,
                    bytes: bad,
                },
                Event::Input {
                    at_us: 3,
                    bytes: basic,
                },
            ],
        };
        let report = analyze(&trace).unwrap();
        assert_eq!(report.valid_reports, 2);
        assert_eq!(report.invalid_reports, 1);
    }
    #[test]
    fn imports_reject_oversize_unknown_fields_and_time_travel() {
        assert!(analyze_json(&" ".repeat(MAX_JSON + 1)).is_err());
        assert!(analyze_json(r#"{"schemaVersion":1,"events":[],"output":true}"#).is_err());
        assert!(
            analyze(&Trace {
                schema_version: 1,
                events: vec![usb(3, 0), usb(2, 1)]
            })
            .is_err()
        );
    }
    #[test]
    fn recorder_keeps_a_bounded_window_until_deadline_and_requires_opt_in() {
        let mut recorder = Recorder::default();
        recorder.input(&[0; 78]);
        assert!(recorder.snapshot().events.is_empty());
        recorder.start();
        recorder.connection(Some(("local".into(), "USB".into())));
        for _ in 0..MAX_EVENTS + 1 {
            recorder.input(&[0; 78]);
        }
        assert!(recorder.active());
        assert_eq!(recorder.snapshot().events.len(), MAX_EVENTS);
        assert!(matches!(
            recorder.snapshot().events[0],
            Event::Truncated { .. }
        ));
        recorder.started = Some(Instant::now() - std::time::Duration::from_secs(11));
        recorder.tick();
        assert!(!recorder.active());
    }
}
