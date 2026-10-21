//! Bounded UDP telemetry receiver for Forza Data Out and the local JSON adapter.
//! Forza offsets follow the documented Sled/Dash layouts (FM: 311/331, FH4/5: 324).

use serde::Deserialize;
use std::{
    io::ErrorKind,
    net::{IpAddr, UdpSocket},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TelemetryFormat {
    #[default]
    Forza,
    Json,
}

impl TelemetryFormat {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "forza" => Some(Self::Forza),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Forza => "forza",
            Self::Json => "json",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub rpm: f32,
    pub max_rpm: f32,
    pub speed_mps: Option<f32>,
    pub gear: Option<u8>,
    pub brake: Option<u8>,
    pub slip: f32,
    pub front_slip: f32,
    pub kerb: [bool; 4],
    pub source: &'static str,
}

impl Sample {
    pub fn brake_slip(self) -> bool {
        self.brake.is_some_and(|b| b > 60) && self.front_slip > 1.0
    }
}

fn f32_at(data: &[u8], at: usize) -> Option<f32> {
    let value = f32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?);
    value.is_finite().then_some(value)
}

pub fn decode(data: &[u8]) -> Option<Sample> {
    let (base, source) = match data.len() {
        232 => (None, "Sled"),
        311 => (Some(232), "Motorsport Dash"),
        324 => (Some(244), "Horizon Dash"),
        331 => (Some(232), "Motorsport 2023"),
        _ => return None,
    };
    if u32::from_le_bytes(data.get(0..4)?.try_into().ok()?) == 0 {
        return None;
    }
    let max_rpm = f32_at(data, 8)?;
    let rpm = f32_at(data, 16)?;
    if !(500.0..=25_000.0).contains(&max_rpm) || !(0.0..=30_000.0).contains(&rpm) {
        return None;
    }
    let mut slip = 0.0f32;
    let mut front_slip = 0.0f32;
    let mut kerb = [false; 4];
    for (i, wheel) in kerb.iter_mut().enumerate() {
        let wheel_slip = f32_at(data, 84 + i * 4)?.abs();
        slip = slip.max(wheel_slip);
        if i < 2 {
            front_slip = front_slip.max(wheel_slip);
        }
        *wheel = u32::from_le_bytes(data.get(116 + i * 4..120 + i * 4)?.try_into().ok()?) != 0;
    }
    let (speed_mps, brake, gear) = if let Some(base) = base {
        let speed = f32_at(data, base + 12)?;
        (
            Some(speed.clamp(0.0, 200.0)),
            Some(*data.get(base + 72)?),
            Some(*data.get(base + 75)?),
        )
    } else {
        (None, None, None)
    };
    Some(Sample {
        rpm,
        max_rpm,
        speed_mps,
        gear,
        brake,
        slip: slip.min(10.0),
        front_slip: front_slip.min(10.0),
        kerb,
        source,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonTelemetry {
    rpm: f32,
    max_rpm: f32,
    #[serde(default)]
    speed_kmh: Option<f32>,
    #[serde(default)]
    gear: Option<u8>,
    #[serde(default)]
    brake: Option<u8>,
    #[serde(default)]
    slip: Option<f32>,
    #[serde(default)]
    front_slip: Option<f32>,
    #[serde(default)]
    kerb: Option<[bool; 4]>,
}

pub fn decode_json(data: &[u8]) -> Option<Sample> {
    if data.len() > 512 {
        return None;
    }
    let event: JsonTelemetry = serde_json::from_slice(data).ok()?;
    if !event.max_rpm.is_finite()
        || !(500.0..=25_000.0).contains(&event.max_rpm)
        || !event.rpm.is_finite()
        || !(0.0..=30_000.0).contains(&event.rpm)
        || event
            .speed_kmh
            .is_some_and(|v| !v.is_finite() || !(0.0..=720.0).contains(&v))
        || event.gear.is_some_and(|v| v > 10)
        || event
            .slip
            .is_some_and(|v| !v.is_finite() || !(0.0..=10.0).contains(&v))
        || event
            .front_slip
            .is_some_and(|v| !v.is_finite() || !(0.0..=10.0).contains(&v))
    {
        return None;
    }
    Some(Sample {
        rpm: event.rpm,
        max_rpm: event.max_rpm,
        speed_mps: event.speed_kmh.map(|v| v / 3.6),
        gear: event.gear,
        brake: event.brake,
        slip: event.slip.unwrap_or(0.0),
        front_slip: event.front_slip.unwrap_or(0.0),
        kerb: event.kerb.unwrap_or([false; 4]),
        source: "Local JSON",
    })
}

#[derive(Default)]
pub struct TelemetryBridge {
    socket: Option<UdpSocket>,
    port: u16,
    lan: bool,
    format: TelemetryFormat,
    started: Option<Instant>,
    last_seen: Option<Instant>,
    last_gear: Option<u8>,
    last_emit: Option<Instant>,
    last_shift_cue: Option<Instant>,
    last_brake_cue: Option<Instant>,
    last_kerb_cue: Option<Instant>,
    last_slip_cue: Option<Instant>,
}

pub struct TelemetryTick {
    pub sample: Sample,
    pub cue: Option<TelemetryCue>,
    pub emit_ui: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryCue {
    Shift,
    BrakeSlip,
    KerbLeft,
    KerbRight,
    KerbBoth,
    Slip,
}

fn due(last: Option<Instant>, delay: Duration) -> bool {
    last.is_none_or(|when| when.elapsed() >= delay)
}

impl TelemetryBridge {
    pub fn enabled(&self) -> bool {
        self.socket.is_some()
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn lan(&self) -> bool {
        self.lan
    }
    pub fn format(&self) -> &'static str {
        self.format.label()
    }
    pub fn stale(&self) -> bool {
        self.enabled()
            && self
                .last_seen
                .or(self.started)
                .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
    }
    pub fn set(&mut self, enabled: bool, port: u16, lan: bool) -> Result<(), String> {
        self.set_format(enabled, port, lan, "forza")
    }
    pub fn set_format(
        &mut self,
        enabled: bool,
        port: u16,
        lan: bool,
        format: &str,
    ) -> Result<(), String> {
        if !enabled {
            *self = Self::default();
            return Ok(());
        }
        let format =
            TelemetryFormat::parse(format).ok_or("Choose Forza Dash or Local JSON telemetry.")?;
        if !(1024..=65_535).contains(&port) {
            return Err("Choose a telemetry port from 1024 to 65535.".into());
        }
        if self.enabled() && self.port == port && self.lan == lan && self.format == format {
            return Ok(());
        }
        *self = Self::default(); // release the previous binding before changing address or port
        let address = if lan { "0.0.0.0" } else { "127.0.0.1" };
        let socket = UdpSocket::bind((address, port))
            .map_err(|e| format!("Could not listen on {address}:{port}: {e}"))?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        *self = Self {
            socket: Some(socket),
            port,
            lan,
            format,
            started: Some(Instant::now()),
            ..Self::default()
        };
        Ok(())
    }
    pub fn poll(&mut self) -> Option<TelemetryTick> {
        let socket = self.socket.as_ref()?;
        let mut latest = None;
        let mut buf = [0u8; 513];
        for _ in 0..8 {
            match socket.recv_from(&mut buf) {
                Ok((n, remote)) => {
                    if n > 512 {
                        continue;
                    }
                    if self.lan
                        && !matches!(remote.ip(), IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() || ip.is_loopback())
                    {
                        continue;
                    }
                    let sample = match self.format {
                        TelemetryFormat::Forza => decode(&buf[..n]),
                        TelemetryFormat::Json => decode_json(&buf[..n]),
                    };
                    if let Some(sample) = sample {
                        latest = Some(sample);
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        let sample = latest?;
        self.last_seen = Some(Instant::now());
        let shift = matches!((self.last_gear, sample.gear), (Some(old), Some(new)) if old != new && old < 10 && new < 10);
        self.last_gear = sample.gear;
        let left_kerb = sample.kerb[0] || sample.kerb[2];
        let right_kerb = sample.kerb[1] || sample.kerb[3];
        let cue = if shift && due(self.last_shift_cue, Duration::from_millis(120)) {
            self.last_shift_cue = Some(Instant::now());
            Some(TelemetryCue::Shift)
        } else if sample.brake_slip() && due(self.last_brake_cue, Duration::from_millis(110)) {
            self.last_brake_cue = Some(Instant::now());
            Some(TelemetryCue::BrakeSlip)
        } else if (left_kerb || right_kerb) && due(self.last_kerb_cue, Duration::from_millis(150)) {
            self.last_kerb_cue = Some(Instant::now());
            Some(match (left_kerb, right_kerb) {
                (true, false) => TelemetryCue::KerbLeft,
                (false, true) => TelemetryCue::KerbRight,
                _ => TelemetryCue::KerbBoth,
            })
        } else if sample.slip > 1.0 && due(self.last_slip_cue, Duration::from_millis(200)) {
            self.last_slip_cue = Some(Instant::now());
            Some(TelemetryCue::Slip)
        } else {
            None
        };
        let emit_ui = self
            .last_emit
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(100));
        if emit_ui {
            self.last_emit = Some(Instant::now());
        }
        Some(TelemetryTick {
            sample,
            cue,
            emit_ui,
        })
    }
}

pub fn rev_color(sample: Sample) -> [u8; 3] {
    let ratio = (sample.rpm / sample.max_rpm).clamp(0.0, 1.0);
    let stepped = (ratio * 12.0).round() / 12.0;
    if stepped < 0.55 {
        [30, (80.0 + stepped * 190.0) as u8, 125]
    } else if stepped < 0.85 {
        [(stepped * 255.0) as u8, 165, 30]
    } else {
        [235, (150.0 * (1.0 - stepped)) as u8, 50]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_fm_and_horizon_dash_offsets() {
        for (len, base) in [(311, 232), (324, 244), (331, 232)] {
            let mut packet = vec![0u8; len];
            packet[0] = 1;
            packet[8..12].copy_from_slice(&8000f32.to_le_bytes());
            packet[16..20].copy_from_slice(&6000f32.to_le_bytes());
            packet[base + 12..base + 16].copy_from_slice(&40f32.to_le_bytes());
            packet[base + 72] = 150;
            packet[base + 75] = 3;
            packet[84..88].copy_from_slice(&1.3f32.to_le_bytes());
            packet[116..120].copy_from_slice(&1u32.to_le_bytes());
            let sample = decode(&packet).unwrap();
            assert_eq!(sample.speed_mps, Some(40.0));
            assert_eq!(sample.brake, Some(150));
            assert_eq!(sample.gear, Some(3));
            assert!(sample.brake_slip());
            assert!(sample.kerb[0]);
        }
    }
    #[test]
    fn rejects_bad_frames() {
        assert!(decode(&[0u8; 311]).is_none());
        assert!(decode(&[0u8; 312]).is_none());
    }
    #[test]
    fn local_json_adapts_bounded_game_signals() {
        let sample = decode_json(br#"{"rpm":6100,"maxRpm":8000,"speedKmh":108,"gear":4,"brake":150,"slip":1.4,"frontSlip":1.3,"kerb":[true,false,false,false]}"#).unwrap();
        assert_eq!(sample.source, "Local JSON");
        assert_eq!(sample.speed_mps, Some(30.0));
        assert!(sample.brake_slip());
        assert!(sample.kerb[0]);
        assert!(decode_json(br#"{"rpm":6100,"maxRpm":8000,"slip":100}"#).is_none());
        assert!(decode_json(br#"{"rpm":6100,"maxRpm":8000,"shape":"square"}"#).is_none());
        assert!(decode_json(br#"{"rpm":6100,"maxRpm":8000,"kerb":[true]}"#).is_none());
        assert!(decode_json(&vec![b' '; 513]).is_none());
    }
    #[test]
    fn listener_reports_missing_first_packet_then_recovers() {
        let socket = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let mut bridge = TelemetryBridge {
            socket: Some(socket),
            started: Some(Instant::now() - Duration::from_secs(2)),
            ..TelemetryBridge::default()
        };
        assert!(bridge.stale());
        bridge.last_seen = Some(Instant::now());
        assert!(!bridge.stale());
    }
}
