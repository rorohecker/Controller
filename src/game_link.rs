//! Bounded, loopback-only game events and a subset of the public DSX UDP protocol.
use aftertouch_protocol::TriggerSetting;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::UdpSocket;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkSettings {
    pub enabled: bool,
    pub port: u16,
    pub format: String,
}
impl Default for LinkSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 6969,
            format: "json".into(),
        }
    }
}
impl LinkSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.port < 1024 || !matches!(self.format.as_str(), "json" | "dsx") {
            return Err("Choose JSON or DSX and a port from 1024 to 65535");
        }
        Ok(())
    }
}
#[derive(Default, Debug)]
pub struct GameUpdate {
    pub weapon: Option<String>,
    pub events: Vec<String>,
    pub triggers: [Option<TriggerSetting>; 2],
    pub rgb: Option<[u8; 3]>,
    pub reset: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonEvent {
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
        if p.weapon
            .as_ref()
            .is_some_and(|w| w.is_empty() || w.len() > 80)
            || p.events.len() > 16
            || p.events.iter().any(|e| e.is_empty() || e.len() > 80)
            || p.weapon.is_none() && p.events.is_empty()
        {
            return Err("Invalid game event names");
        }
        return Ok(GameUpdate {
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
    socket: Option<UdpSocket>,
    settings: LinkSettings,
}
impl GameLink {
    pub fn set(&mut self, settings: LinkSettings) -> Result<(), String> {
        settings.validate().map_err(str::to_string)?;
        if self.settings.enabled == settings.enabled
            && self.settings.port == settings.port
            && self.settings.format == settings.format
        {
            return Ok(());
        }
        if settings.enabled && self.socket.is_some() && settings.port == self.settings.port {
            self.settings = settings;
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
        Ok(())
    }
    pub fn stop(&mut self) {
        self.socket = None;
        self.settings.enabled = false;
    }
    pub fn poll(&self, connected: bool) -> Vec<Result<GameUpdate, &'static str>> {
        let mut output = Vec::new();
        let Some(socket) = &self.socket else {
            return output;
        };
        let mut buffer = [0; 8193];
        for _ in 0..16 {
            match socket.recv_from(&mut buffer) {
                Ok((n, peer)) => {
                    let update = decode(&buffer[..n], &self.settings.format);
                    if self.settings.format == "dsx" {
                        let _=socket.send_to(serde_json::json!({"Status":if update.is_ok(){"OK"}else{"Unsupported packet"},"isControllerConnected":connected,"BatteryLevel":0}).to_string().as_bytes(),peer);
                    }
                    output.push(update);
                }
                Err(_) => break,
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        })
        .unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .send_to(
                br#"{"events":["roll"]}"#,
                (std::net::Ipv4Addr::LOCALHOST, port),
            )
            .unwrap();
        assert_eq!(link.poll(true)[0].as_ref().unwrap().events, ["roll"]);
        link.stop();
        assert!(UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok());
    }
}
