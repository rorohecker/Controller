//! Bounded, loopback-only game events and a subset of the public DSX UDP protocol.
use aftertouch_protocol::TriggerSetting;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::UdpSocket;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkSettings {
    pub enabled: bool,
    pub port: u16,
    pub format: String,
    pub recognition: crate::recognition::Policy,
}
impl Default for LinkSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 6969,
            format: "json".into(),
            recognition: crate::recognition::Policy::default(),
        }
    }
}
impl LinkSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.recognition.validate()?;
        if self.port < 1024 || !matches!(self.format.as_str(), "json" | "dsx") {
            return Err("Choose JSON or DSX and a port from 1024 to 65535");
        }
        Ok(())
    }
}
#[derive(Default, Debug)]
pub struct GameUpdate {
    pub detection: Option<crate::recognition::Observation>,
    pub recognition_status: Option<crate::recognition::Status>,
    pub weapon_reset: bool,
    pub weapon: Option<String>,
    pub events: Vec<String>,
    pub triggers: [Option<TriggerSetting>; 2],
    pub rgb: Option<[u8; 3]>,
    pub reset: bool,
    pub player_leds: Option<u8>,
    pub microphone_led: Option<u8>,
    pub packet: Option<String>,
    pub format: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonEvent {
    #[serde(default)]
    detection: Option<crate::recognition::Observation>,
    #[serde(default)]
    weapon: Option<String>,
    #[serde(default)]
    events: Vec<String>,
}

pub fn decode(data: &[u8], format: &str) -> Result<GameUpdate, &'static str> {
    if data.len() > 8192 {
        return Err("Game packet exceeds 8 KiB");
    }
    if format == "json" {
        let p: JsonEvent = serde_json::from_slice(data)
            .map_err(|_| "Expected a weapon and/or events JSON packet")?;
        if let Some(o) = &p.detection {
            o.validate()?;
            if p.weapon.is_some() {
                return Err("Use weapon or detection, not both");
            }
        }
        if p.weapon
            .as_ref()
            .is_some_and(|w| w.is_empty() || w.len() > 80)
            || p.events.len() > 16
            || p.events.iter().any(|e| e.is_empty() || e.len() > 80)
            || p.weapon.is_none() && p.events.is_empty() && p.detection.is_none()
        {
            return Err("Invalid game event names");
        }
        return Ok(GameUpdate {
            detection: p.detection,
            weapon: p.weapon,
            events: p.events,
            ..GameUpdate::default()
        });
    }
    let p: Value = serde_json::from_slice(data).map_err(|_| "Invalid DSX JSON")?;
    let instructions = p["instructions"]
        .as_array()
        .filter(|i| !i.is_empty() && i.len() <= 16)
        .ok_or("Expected DSX instructions")?;
    let mut output = GameUpdate::default();
    for instruction in instructions {
        let kind = instruction["type"]
            .as_u64()
            .ok_or("Invalid DSX instruction")?;
        let args = instruction["parameters"]
            .as_array()
            .ok_or("Invalid DSX parameters")?;
        let number = |index: usize, max: u64| -> Result<u8, &'static str> {
            args.get(index)
                .and_then(Value::as_u64)
                .filter(|v| *v <= max)
                .map(|v| v as u8)
                .ok_or("DSX value out of range")
        };
        if number(0, 255)? != 0 {
            continue;
        } // Only the selected controller, index zero.
        match kind {
            1 => {
                let side = number(1, 2)?;
                if side == 0 {
                    return Err("Invalid DSX trigger");
                }
                let mode = number(2, 26)?;
                output.triggers[usize::from(side - 1)] = Some(dsx_trigger(mode, &args[3..])?);
            }
            2 => {
                let gain = f32::from(number(4, 255)?) / 255.;
                output.rgb = Some(
                    [number(1, 255)?, number(2, 255)?, number(3, 255)?]
                        .map(|v| (f32::from(v) * gain).round() as u8),
                );
            }
            5 => {
                output.microphone_led = Some(match number(1, 2)? {
                    0 => 1,
                    1 => 2,
                    _ => 0,
                })
            }
            6 => output.player_leds = Some([4, 10, 21, 27, 31, 0][usize::from(number(1, 5)?)]),
            7 => output.reset = true,
            0 => {} // Status request; reply is sent by the listener.
            _ => return Err("This DSX instruction is not supported; use trigger, RGB, or reset"),
        }
    }
    Ok(output)
}
fn dsx_trigger(mode: u8, args: &[Value]) -> Result<TriggerSetting, &'static str> {
    let n = |i: usize, max: u64| -> Result<u8, &'static str> {
        args.get(i)
            .and_then(Value::as_u64)
            .filter(|v| *v <= max)
            .map(|v| v as u8)
            .ok_or("DSX trigger value out of range")
    };
    let mut t = TriggerSetting {
        mode: "off".into(),
        strength: 100.,
        force_cap: Some(8),
        curve: vec![0.; 11],
        ..TriggerSetting::default()
    };
    match mode {
        0 | 20 => {}
        13 | 21 | 23 | 17 => {
            let start = n(0, 9)?;
            let force = n(1, 8)?;
            t.mode = if matches!(mode, 23 | 17) {
                "vibration"
            } else {
                "resistance"
            }
            .into();
            t.zone_forces = Some(
                (0..10)
                    .map(|i| {
                        if i >= start {
                            f32::from(force) / 8.
                        } else {
                            0.
                        }
                    })
                    .collect(),
            );
            if matches!(mode, 23 | 17) {
                t.frequency_hz = Some(n(2, 255)?.max(1));
            }
        }
        16 | 22 => {
            t.mode = "weapon".into();
            t.start_zone = Some(n(0, 7)?);
            t.end_zone = Some(n(1, 9)?);
            t.strength = f32::from(n(2, 8)?) * 12.5;
        }
        24 => {
            let start = n(0, 7)?;
            let end = n(1, 9)?;
            if end <= start {
                return Err("DSX slope needs an increasing range");
            }
            let a = f32::from(n(2, 8)?) / 8.;
            let b = f32::from(n(3, 8)?) / 8.;
            t.mode = "resistance".into();
            t.zone_forces = Some(
                (0..10)
                    .map(|i| {
                        if i < start {
                            0.
                        } else {
                            a + (b - a) * f32::from(i.saturating_sub(start).min(end - start))
                                / f32::from(end - start)
                        }
                    })
                    .collect(),
            );
        }
        25 | 26 => {
            let offset = usize::from(mode == 26);
            t.mode = if mode == 26 {
                "vibration"
            } else {
                "resistance"
            }
            .into();
            let mut forces = Vec::new();
            for i in 0..10 {
                forces.push(f32::from(n(i + offset, 8)?) / 8.);
            }
            t.zone_forces = Some(forces);
            if mode == 26 {
                t.frequency_hz = Some(n(0, 255)?.max(1));
            }
        }
        _ => return Err("Unsupported legacy DSX trigger mode; use FEEDBACK, WEAPON, or VIBRATION"),
    }
    t.validate()?;
    Ok(t)
}

#[derive(Default)]
pub struct GameLink {
    recognizer: crate::recognition::Recognizer,
    socket: Option<UdpSocket>,
    settings: LinkSettings,
    recording: bool,
}
impl GameLink {
    pub fn record(&mut self, enabled: bool) {
        self.recording = enabled;
    }
    pub fn set(&mut self, settings: LinkSettings) -> Result<(), String> {
        settings.validate().map_err(str::to_string)?;
        if self.settings == settings {
            return Ok(());
        }
        if settings.enabled && self.socket.is_some() && settings.port == self.settings.port {
            self.settings = settings;
            self.recognizer.reconfigure();
            return Ok(());
        }
        let socket = if settings.enabled {
            let socket = UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, settings.port))
                .map_err(|e| format!("Port {} unavailable: {e}", settings.port))?;
            socket.set_nonblocking(true).map_err(|e| e.to_string())?;
            Some(socket)
        } else {
            None
        };
        self.socket = socket;
        self.settings = settings;
        self.recognizer.reconfigure();
        Ok(())
    }
    pub fn stop(&mut self) {
        self.recognizer = crate::recognition::Recognizer::default();
        self.socket = None;
        self.settings.enabled = false;
        self.recording = false;
    }
    pub fn poll(
        &mut self,
        connected: bool,
        battery: Option<u8>,
    ) -> Vec<Result<GameUpdate, &'static str>> {
        let mut output = Vec::new();
        if let Some(status) = self
            .recognizer
            .tick(&self.settings.recognition, std::time::Instant::now())
        {
            output.push(Ok(GameUpdate {
                weapon_reset: status.clear,
                recognition_status: Some(status),
                ..GameUpdate::default()
            }));
        }
        let Some(socket) = &self.socket else {
            return output;
        };
        let mut buffer = [0; 8193];
        for _ in 0..16 {
            match socket.recv_from(&mut buffer) {
                Ok((n, peer)) => {
                    let update = decode(&buffer[..n], &self.settings.format).map(|mut update| {
                        let (weapon, status) = self.recognizer.observe(
                            update.detection.take(),
                            update.weapon.take(),
                            &self.settings.recognition,
                            std::time::Instant::now(),
                        );
                        update.weapon = weapon;
                        update.recognition_status = status;
                        if self.recording {
                            update.packet =
                                Some(String::from_utf8_lossy(&buffer[..n]).into_owned());
                        }
                        update.format = Some(self.settings.format.clone());
                        update
                    });
                    if self.settings.format == "dsx" {
                        let _=socket.send_to(serde_json::json!({"Status":if update.is_ok(){"OK"}else{"Unsupported packet"},"isControllerConnected":connected,"BatteryLevel":battery,"BatteryKnown":battery.is_some()}).to_string().as_bytes(),peer);
                    }
                    output.push(update);
                }
                Err(_) => break,
            }
        }
        output
    }
    pub fn manual_weapon(
        &mut self,
        weapon: Option<String>,
    ) -> Result<crate::recognition::Status, &'static str> {
        self.recognizer.manual(
            weapon,
            &self.settings.recognition,
            std::time::Instant::now(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_returns_measured_battery_and_led_modes_stay_independent() {
        let update = decode(
            br#"{"instructions":[{"type":6,"parameters":[0,2]},{"type":5,"parameters":[0,1]}]}"#,
            "dsx",
        )
        .unwrap();
        assert_eq!(update.player_leds, Some(21));
        assert_eq!(update.microphone_led, Some(2));
        assert!(update.triggers.iter().all(Option::is_none));
        assert!(
            decode(
                br#"{"instructions":[{"type":6,"parameters":[0,6]}]}"#,
                "dsx"
            )
            .is_err()
        );
        let mut link = GameLink::default();
        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        link.set(LinkSettings {
            enabled: true,
            port,
            format: "dsx".into(),
            ..LinkSettings::default()
        })
        .unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        sender
            .send_to(
                br#"{"instructions":[{"type":0,"parameters":[0]}]}"#,
                ("127.0.0.1", port),
            )
            .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while link.poll(true, Some(75)).is_empty() {
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let mut buffer = [0; 1024];
        let n = sender.recv(&mut buffer).unwrap();
        let status: Value = serde_json::from_slice(&buffer[..n]).unwrap();
        assert_eq!(status["BatteryLevel"], 75);
        assert_eq!(status["BatteryKnown"], true);
        assert_eq!(status["isControllerConnected"], true);
    }
    #[test]
    fn validates_events_and_dsx_effects() {
        let e = decode(br#"{"weapon":"smg","events":["dash"]}"#, "json").unwrap();
        assert_eq!(e.weapon.as_deref(), Some("smg"));
        assert_eq!(e.events, ["dash"]);
        let e = decode(
            br#"{"instructions":[{"type":1,"parameters":[0,2,22,2,6,8]}]}"#,
            "dsx",
        )
        .unwrap();
        let t = e.triggers[1].as_ref().unwrap();
        assert_eq!(t.mode, "weapon");
        assert_eq!(t.strength, 100.);
        assert!(
            decode(
                br#"{"instructions":[{"type":1,"parameters":[0,2,22,7,2,8]}]}"#,
                "dsx"
            )
            .is_err()
        );
        assert!(decode(br#"{"events":[""]}"#, "json").is_err());
        assert!(decode(&vec![32; 8193], "json").is_err());
    }
    #[test]
    fn listener_receives_on_loopback_and_can_release_port() {
        let mut link = GameLink::default();
        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        link.set(LinkSettings {
            enabled: true,
            port,
            format: "json".into(),
            ..LinkSettings::default()
        })
        .unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .send_to(
                br#"{"events":["roll"]}"#,
                (std::net::Ipv4Addr::LOCALHOST, port),
            )
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let packet = loop {
            if let Some(packet) = link.poll(true, Some(70)).into_iter().next() {
                break packet;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Loopback packet was not delivered"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(packet.unwrap().events, ["roll"]);
        link.stop();
        assert!(UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok());
    }
}
