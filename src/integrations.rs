//! Read-only integration discovery and bounded experiment scheduling.
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[cfg(windows)]
pub fn windows_build() -> u32 {
    #[repr(C)]
    struct Version {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        text: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(value: *mut Version) -> i32;
    }
    let mut version = Version {
        size: std::mem::size_of::<Version>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        text: [0; 128],
    };
    if unsafe { RtlGetVersion(&mut version) } >= 0 {
        version.build
    } else {
        0
    }
}
#[cfg(not(windows))]
pub fn windows_build() -> u32 {
    0
}

pub fn environment() -> Value {
    let session = if cfg!(target_os = "linux") {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            "wayland"
        } else if std::env::var_os("DISPLAY").is_some() {
            "x11"
        } else {
            "headless"
        }
    } else {
        std::env::consts::OS
    };
    json!({"session":session,"desktopCandidate":session!="headless",
    "portalSessionBus":std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
    "permission":"not_probed","message":match session{
        "wayland"=>"Wayland session detected. Test desktop access to request a backend/portal session; environment detection does not prove permission.",
        "x11"=>"X11 session detected. Test desktop access to check the current display connection.",
        "headless"=>"No desktop display is available in this process environment.",
        "macos"=>"Accessibility and Input Monitoring permissions are checked separately.",
        _=>"Windows input is limited by the focused app's privilege level."
    }})
}

#[cfg(windows)]
fn processes() -> Result<Vec<Value>, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::*,
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("Could not list running processes".into());
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    let mut result = Vec::new();
    while found {
        let end = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if entry.th32ProcessID > 4
            && !matches!(
                name.to_ascii_lowercase().as_str(),
                "controller.exe" | "aftertouch.exe"
            )
        {
            result.push(json!({"pid":entry.th32ProcessID,"name":name}));
        }
        if result.len() >= 1024 {
            break;
        }
        found = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    result.sort_by_key(|v| {
        (
            v["name"].as_str().unwrap_or("").to_ascii_lowercase(),
            v["pid"].as_u64(),
        )
    });
    Ok(result)
}
#[cfg(not(windows))]
fn processes() -> Result<Vec<Value>, String> {
    Ok(Vec::new())
}

#[tauri::command]
pub async fn integration_info() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(||{
        use cpal::traits::{HostTrait,DeviceTrait};
        let mut endpoints=Vec::new();
        if cfg!(windows) && let Ok(devices)=cpal::default_host().output_devices(){
            for device in devices.take(64){
                let name=device.to_string();let lower=name.to_ascii_lowercase();
                // Physical controller endpoints are never loopback sources.
                if lower.contains("dualsense")||lower.contains("wireless controller"){continue;}
                if let(Ok(id),Ok(format))=(device.id(),device.default_output_config())
                    && format.channels()==4&&format.sample_rate()==48_000{
                    endpoints.push(json!({"id":id.to_string(),"name":name,"channels":4,"sampleRate":48000}));
                }
            }
        }
        Ok(json!({"windowsBuild":windows_build(),"processCapture":cfg!(windows)&&windows_build()>=20348,
            "processes":processes()?,"nativeRelayEndpoints":endpoints,"environment":environment(),
            "nativeDualSenseDriverIntegrated":false,"nativeDriverMessage":"The bundled ViGEmBus exposes Xbox 360/DS4, not a native DualSense HID/audio device. Native USB/dongle games can use controller handoff; the four-channel relay is an endpoint prototype."}))
    }).await.map_err(|e|e.to_string())?
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DongleProbe {
    pub compatible: bool,
    pub firmware: Option<String>,
    pub evidence: &'static str,
}
impl Default for DongleProbe {
    fn default() -> Self {
        Self {
            compatible: false,
            firmware: None,
            evidence: "Not identified; Sony USB identity alone cannot distinguish a cable from a dongle",
        }
    }
}
pub fn dongle_probe(version: &[u8], config: &[u8]) -> DongleProbe {
    // Read-only DS5Dongle F8/F7 contract, schema 5. No F6 SET or flash operation.
    if version.first() != Some(&0xf8) || config.first() != Some(&0xf7) {
        return DongleProbe::default();
    }
    let bytes = &version[1..];
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..end];
    if bytes.len() < 3
        || bytes.len() > 48
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b".-_+ ".contains(b))
        || bytes.iter().filter(|b| **b == b'.').count() < 2
        || !bytes.iter().any(u8::is_ascii_digit)
    {
        return DongleProbe::default();
    }
    let c = &config[1..];
    if c.len() < 24 || c[0] != 5 {
        return DongleProbe::default();
    }
    let gain = f32::from_le_bytes(c[1..5].try_into().unwrap());
    if !gain.is_finite()
        || !(0.0..=4.0).contains(&gain)
        || c[5] > 127
        || c[6] > 127
        || c[7] > 7
        || c[8] > 60
        || c[10] > 3
        || !(16..=127).contains(&c[11])
        || c[12] > 2
        || c[15] > 3
        || c[16] > 3
        || c[18] > 10
        || c[21] > 1
        || [9, 13, 14, 17, 19, 22, 23].iter().any(|i| c[*i] > 1)
    {
        return DongleProbe::default();
    }
    DongleProbe {
        compatible: true,
        firmware: Some(String::from_utf8_lossy(bytes).trim().into()),
        evidence: "Read-only F8 version and F7 schema-5 configuration match DS5Dongle's interface; firmware identity is not authenticated",
    }
}

#[tauri::command]
pub fn inspect_native_report(report_base64: String) -> Result<Value, String> {
    use base64::Engine;
    if report_base64.len() > 100 {
        return Err("Choose one USB output report, at most 64 bytes".into());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(report_base64)
        .map_err(|e| e.to_string())?;
    if !matches!(raw.len(), 48 | 63 | 64)
        || raw.first() != Some(&2)
        || raw.len() == 64 && raw[63] != 0
    {
        return Err("Expected a 48/63-byte USB 0x02 report or a zero-padded 64-byte report".into());
    }
    if raw.len() > 48 && raw[48..].iter().any(|v| *v != 0) {
        return Err("Unknown USB extension fields cannot be converted to the standard Bluetooth control frame".into());
    }
    let mut usb = [0u8; 63];
    let n = raw.len().min(63);
    usb[..n].copy_from_slice(&raw[..n]);
    let bt = aftertouch_protocol::bt_report(&usb, 0);
    Ok(
        json!({"writesHardware":false,"usbBytes":raw.len(),"flags":[usb[1],usb[2]],"rightTriggerMode":usb[11],"leftTriggerMode":usb[22],"rgb":[usb[45],usb[46],usb[47]],"bluetoothBytes":bt.len(),"bluetoothBase64":base64::engine::general_purpose::STANDARD.encode(bt),"message":"Packet conversion only. A virtual HID/audio driver must provide native game output; no driver is installed or emulated by this preview."}),
    )
}

pub struct LinkExperiment {
    pub start: Instant,
    pub deadline: Instant,
    pub next_frame: u32,
    pub sent: u32,
    pub skipped: u32,
    pub max_late_us: u64,
    pub write_us: u64,
    pub callback_base: u64,
    pub startup_us: u64,
    pub sequence: u8,
    pub bluetooth: bool,
    pub strength: f32,
    pub last_event: Instant,
}
impl LinkExperiment {
    pub fn new(now: Instant, bluetooth: bool, strength: u8) -> Result<Self, String> {
        if !(1..=25).contains(&strength) {
            return Err("Experiment strength must be 1-25%".into());
        }
        Ok(Self {
            start: now,
            deadline: now + Duration::from_secs(2),
            next_frame: 0,
            sent: 0,
            skipped: 0,
            max_late_us: 0,
            write_us: 0,
            callback_base: 0,
            startup_us: 0,
            sequence: 0,
            bluetooth,
            strength: f32::from(strength) / 100.0,
            last_event: now,
        })
    }
    pub fn due(&mut self, now: Instant) -> Option<[i8; 64]> {
        if now >= self.deadline || !self.bluetooth {
            return None;
        }
        let frames = now.saturating_duration_since(self.start).as_micros() * 3000 / 32_000_000;
        if frames < u128::from(self.next_frame) {
            return None;
        }
        let index = frames.min(187) as u32;
        self.skipped += index.saturating_sub(self.next_frame);
        let scheduled = u64::from(index) * 32_000 / 3;
        self.max_late_us = self.max_late_us.max(
            (now.saturating_duration_since(self.start).as_micros() as u64)
                .saturating_sub(scheduled),
        );
        self.next_frame = index + 1;
        let mut pcm = [0; 64];
        for frame in 0..32 {
            let time = (index * 32 + frame) as f32 / 3000.0;
            let gate = if (time * 4.0).fract() < 0.5 { 1.0 } else { 0.0 };
            let sample = ((time * 80.0 * std::f32::consts::TAU).sin()
                * gate
                * self.strength
                * 127.0)
                .round() as i8;
            pcm[frame as usize * 2] = sample;
            pcm[frame as usize * 2 + 1] = sample;
        }
        Some(pcm)
    }
    pub fn event(&self, running: bool) -> Value {
        json!({"type":"linkExperiment","running":running,"transport":if self.bluetooth{"Bluetooth"}else{"USB"},"durationMs":2000,"positionMs":self.start.elapsed().as_millis().min(2000),"sent":self.sent,"skipped":self.skipped,"maxScheduleLateUs":self.max_late_us,"maxWriteUs":self.write_us,"startupUs":self.startup_us,"unit":if self.bluetooth{"packets"}else{"audio callbacks"},"completed":!running&&self.start.elapsed()>=Duration::from_secs(2),"physicalLatencyMeasured":false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dongle_detection_requires_both_read_only_contracts() {
        let version = b"\xf8v1.2.3\0";
        let mut config = [0u8; 25];
        config[0] = 0xf7;
        config[1] = 5;
        config[2..6].copy_from_slice(&1.0f32.to_le_bytes());
        config[12] = 32;
        assert!(dongle_probe(version, &config).compatible);
        assert!(!dongle_probe(b"\xf8Sony", &config).compatible);
        config[15] = 255;
        assert!(!dongle_probe(version, &config).compatible);
        assert!(!dongle_probe(version, &[]).compatible);
    }
    #[test]
    fn bluetooth_schedule_is_bounded_and_never_catches_up_with_a_burst() {
        let now = Instant::now();
        let mut run = LinkExperiment::new(now, true, 25).unwrap();
        assert!(run.due(now).unwrap().iter().all(|v| v.abs() <= 32));
        assert!(run.due(now).is_none());
        assert!(run.due(now + Duration::from_millis(100)).is_some());
        assert_eq!(run.skipped, 8);
        assert!(run.due(now + Duration::from_millis(100)).is_none());
        assert!(run.due(now + Duration::from_secs(2)).is_none());
        assert!(LinkExperiment::new(now, true, 26).is_err());
    }
    #[test]
    fn native_report_preview_is_read_only_and_rejects_unknown_formats() {
        use base64::Engine;
        let mut report = [0u8; 63];
        report[0] = 2;
        report[45..48].copy_from_slice(&[1, 2, 3]);
        let result =
            inspect_native_report(base64::engine::general_purpose::STANDARD.encode(report))
                .unwrap();
        assert_eq!(result["writesHardware"], false);
        assert_eq!(result["bluetoothBytes"], 78);
        assert_eq!(result["rgb"], json!([1, 2, 3]));
        assert!(inspect_native_report("A".repeat(1000)).is_err());
    }
}
