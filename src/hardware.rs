//! Local controller identities and read-only Edge snapshots. No feature SET operations.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
static EDGE_READING: AtomicBool = AtomicBool::new(false);
pub fn start_edge_read(
    path: std::ffi::CString,
    owner: String,
    firmware: Option<String>,
    request_id: u32,
    events: std::sync::mpsc::Sender<serde_json::Value>,
) -> Result<(), String> {
    if EDGE_READING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("The previous Edge read is still finishing".into());
    }
    let spawned=std::thread::Builder::new().name("edge-profile-reader".into()).spawn(move||{
        struct Busy;impl Drop for Busy{fn drop(&mut self){EDGE_READING.store(false,Ordering::Release);}}let _busy=Busy;
        let result=(||{
            let api=hidapi::HidApi::new().map_err(|e|e.to_string())?;
            let info=api.device_list().find(|d|d.path()==path.as_c_str()&&d.vendor_id()==aftertouch_protocol::SONY_VID&&d.product_id()==aftertouch_protocol::EDGE_PID&&matches!(d.bus_type(),hidapi::BusType::Usb)&&d.usage_page()==1&&d.usage()==5).ok_or("The selected USB Edge is no longer available")?;
            let hid=info.open_device(&api).map_err(|e|e.to_string())?;
            if crate::device::controller_identity(info,&hid)!=owner{return Err("Controller identity changed; no Edge backup was captured".into());}
            let started=std::time::Instant::now();let mut slots=Vec::new();
            for slot in 0..4{let mut chunks=Vec::new();for chunk in 0..3{
                if started.elapsed()>std::time::Duration::from_secs(4){return Err("Edge profile read timed out; no backup was captured".into());}
                let id=0x70+(slot*3+chunk) as u8;let mut report=vec![0;64];report[0]=id;
                let n=hid.get_feature_report(&mut report).map_err(|e|format!("Feature {id:02x}: {e}"))?;
                if n!=64{return Err("Incomplete Edge report; no backup was captured".into());}chunks.push(report);
            }slots.push(chunks);}
            if started.elapsed()>std::time::Duration::from_secs(4){return Err("Edge profile read timed out; no backup was saved".into());}
            let data=EdgeSnapshot{schema_version:1,controller_id:owner.clone(),firmware,captured_at:SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e|e.to_string())?.as_millis() as u64,slots};data.validate()?;Ok::<_,String>(data)
        })();
        let event=match result{Ok(data)=>serde_json::json!({"type":"edgeSnapshot","requestId":request_id,"data":data}),Err(error)=>serde_json::json!({"type":"edgeReadStatus","requestId":request_id,"controllerId":owner,"error":error})};let _=events.send(event);
    });
    if let Err(error) = spawned {
        EDGE_READING.store(false, Ordering::Release);
        return Err(error.to_string());
    }
    Ok(())
}
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControllerRecord {
    pub controller_id: String,
    pub name: String,
    #[serde(default)]
    pub player: u8,
    #[serde(default)]
    pub endpoint_id: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Library {
    pub schema_version: u8,
    pub controllers: Vec<ControllerRecord>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: 1,
            controllers: Vec::new(),
        }
    }
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.contains(char::is_control)
}
impl Library {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.controllers.len() > 64 {
            return Err("Unsupported controller library or more than 64 controllers".into());
        }
        let (mut ids, mut players, mut endpoints) =
            (HashSet::new(), HashSet::new(), HashSet::new());
        for c in &self.controllers {
            if !text(&c.controller_id, 200)
                || !text(c.name.trim(), 160)
                || c.name.chars().count() > 40
                || c.player > 5
                || !ids.insert(&c.controller_id)
            {
                return Err("Controller names, identities and player assignments must be valid and distinct".into());
            }
            if c.player != 0 && !players.insert(c.player) {
                return Err("That player light is assigned to another saved controller".into());
            }
            if let Some(id) = &c.endpoint_id
                && (!text(id, 2048) || !endpoints.insert(id))
            {
                return Err("Choose a distinct audio endpoint for each controller".into());
            }
        }
        Ok(())
    }
}
fn root() -> Result<PathBuf, String> {
    Ok(crate::profiles::config_base()?
        .join("CONTROLLER")
        .join("hardware"))
}
pub fn load_library() -> Result<Library, String> {
    let p = root()?.join("controllers.json");
    if !p.exists() {
        return Ok(Library::default());
    }
    let data = read_bounded(&p, 256 * 1024)?;
    let library: Library = serde_json::from_slice(&data)
        .map_err(|e| format!("Controller library could not be read: {e}"))?;
    library.validate()?;
    Ok(library)
}
fn read_bounded(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    f.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Hardware data exceeds its size limit".into());
    }
    Ok(bytes)
}
#[tauri::command]
pub fn controller_library() -> Result<Library, String> {
    load_library()
}
#[tauri::command]
pub fn save_controller_library(data: Library) -> Result<(), String> {
    data.validate()?;
    let root = root()?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    crate::profiles::write_archive(
        &root.join("controllers.json"),
        &serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?,
    )
}
pub fn assignment(id: &str) -> Result<Option<ControllerRecord>, String> {
    Ok(load_library()?
        .controllers
        .into_iter()
        .find(|c| c.controller_id == id))
}

#[tauri::command]
pub async fn controller_audio_endpoints() -> Result<Vec<serde_json::Value>, String> {
    tauri::async_runtime::spawn_blocking(||{
        let mut result=Vec::new();
        #[cfg(windows)] {
            use wasapi::{DeviceEnumerator,Direction,SampleType,ShareMode,WaveFormat};
            wasapi::initialize_mta().ok().map_err(|e|e.to_string())?;
            struct Com;impl Drop for Com{fn drop(&mut self){wasapi::deinitialize();}}let _com=Com;
            let devices=DeviceEnumerator::new().map_err(|e|e.to_string())?.get_device_collection(&Direction::Render).map_err(|e|e.to_string())?;
            let format=WaveFormat::new(16,16,&SampleType::Int,48000,4,Some(0));
            for d in (&devices).into_iter().take(64){let d=d.map_err(|e|e.to_string())?;let client=d.get_iaudioclient().map_err(|e|e.to_string())?;
                if client.is_supported(&format,&ShareMode::Exclusive).is_ok(){result.push(serde_json::json!({"id":d.get_id().map_err(|e|e.to_string())?,"name":d.get_friendlyname().map_err(|e|e.to_string())?}));}
            }
        }
        #[cfg(not(windows))] {
            use cpal::traits::{HostTrait,DeviceTrait};
            for d in cpal::default_host().output_devices().map_err(|e|e.to_string())?.take(64){
                if d.supported_output_configs().is_ok_and(|mut configs|configs.any(|c|c.channels()==4&&c.min_sample_rate()<=48000&&c.max_sample_rate()>=48000))&&let Ok(id)=d.id(){result.push(serde_json::json!({"id":id.to_string(),"name":d.to_string()}));}
            }
        }
        Ok(result)
    }).await.map_err(|e|e.to_string())?
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeSnapshot {
    pub schema_version: u8,
    pub controller_id: String,
    pub firmware: Option<String>,
    pub captured_at: u64,
    pub slots: Vec<Vec<Vec<u8>>>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeSlot {
    pub revision: u8,
    pub slot: usize,
    pub assigned: bool,
    pub checksum_valid: bool,
    pub name: String,
    pub left_range: [u8; 2],
    pub right_range: [u8; 2],
    pub grip_level: u8,
    pub trigger_level: u8,
}
fn crc32(data: impl IntoIterator<Item = u8>) -> u32 {
    let mut crc = !0u32;
    for b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32.wrapping_mul(crc & 1));
        }
    }
    !crc
}
fn content_crc(chunks: &[Vec<u8>]) -> u32 {
    crc32(
        chunks[0][2..60]
            .iter()
            .chain(&chunks[1][2..60])
            .chain(&chunks[2][2..56])
            .copied(),
    )
}
impl EdgeSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !text(&self.controller_id, 200)
            || self.firmware.as_ref().is_some_and(|s| !text(s, 200))
            || self.slots.len() != 4
        {
            return Err("Unsupported Edge snapshot".into());
        }
        for (i, s) in self.slots.iter().enumerate() {
            if s.len() != 3
                || s.iter()
                    .enumerate()
                    .any(|(chunk, b)| b.len() != 64 || b[0] != 0x70 + (i * 3 + chunk) as u8)
            {
                return Err("Edge snapshots require four slots with three complete, ordered feature reports each".into());
            }
        }
        Ok(())
    }
    pub fn describe(&self) -> Result<Vec<EdgeSlot>, String> {
        self.validate()?;
        Ok(self
            .slots
            .iter()
            .enumerate()
            .map(|(slot, s)| {
                let label: Vec<u8> = s[0][6..60].iter().chain(&s[1][2..28]).copied().collect();
                let utf16: Vec<u16> = label
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .take_while(|c| *c != 0)
                    .collect();
                EdgeSlot {
                    revision: s[0][2],
                    slot,
                    assigned: s[0][1] != 0x10,
                    checksum_valid: content_crc(s)
                        == u32::from_le_bytes(s[2][56..60].try_into().unwrap()),
                    name: String::from_utf16_lossy(&utf16),
                    left_range: [s[2][4], s[2][5]],
                    right_range: if s[2][31] & 0x80 != 0 {
                        [s[2][4], s[2][5]]
                    } else {
                        [s[2][6], s[2][7]]
                    },
                    grip_level: s[2][8],
                    trigger_level: s[2][9],
                }
            })
            .collect())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeEdit {
    pub name: String,
    pub left_range: [u8; 2],
    pub right_range: [u8; 2],
    pub grip_level: u8,
    pub trigger_level: u8,
}
#[tauri::command]
pub fn inspect_edge_snapshot(data: EdgeSnapshot) -> Result<Vec<EdgeSlot>, String> {
    data.describe()
}
#[tauri::command]
pub fn edit_edge_snapshot(
    data: EdgeSnapshot,
    slot: usize,
    edit: EdgeEdit,
) -> Result<serde_json::Value, String> {
    data.validate()?;
    if !(1..=3).contains(&slot) {
        return Err("The default Edge slot is read-only".into());
    }
    let description = data.describe()?;
    if !description[slot].assigned
        || !description[slot].checksum_valid
        || data.slots[slot][0][2] != 1
    {
        return Err("This slot is empty, has an unknown revision, or fails its content checksum; keep its original backup".into());
    }
    let name: Vec<u16> = edit.name.encode_utf16().collect();
    if name.is_empty()
        || name.len() > 40
        || edit.name.contains(char::is_control)
        || edit.left_range[0] >= edit.left_range[1]
        || edit.right_range[0] >= edit.right_range[1]
        || ![0, 2, 3, 255].contains(&edit.grip_level)
        || ![0, 6, 9, 255].contains(&edit.trigger_level)
    {
        return Err(
            "Choose a name, increasing trigger ranges, and supported strength levels".into(),
        );
    }
    let mut chunks = data.slots[slot].clone();
    let mut label = [0u8; 80];
    for (i, c) in name.into_iter().enumerate() {
        label[i * 2..i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
    chunks[0][6..60].copy_from_slice(&label[..54]);
    chunks[1][2..28].copy_from_slice(&label[54..]);
    chunks[2][4..6].copy_from_slice(&edit.left_range);
    chunks[2][6..8].copy_from_slice(&edit.right_range);
    chunks[2][8] = edit.grip_level;
    chunks[2][9] = edit.trigger_level;
    // Separate ranges; every other mapping/stick/unknown field stays byte-for-byte intact.
    chunks[2][31] &= 0x7f;
    let crc = content_crc(&chunks);
    chunks[2][56..60].copy_from_slice(&crc.to_le_bytes());
    Ok(
        serde_json::json!({"format":"controller-edge-local-draft-v1","controllerId":data.controller_id,"slot":slot,"edit":edit,"readReports":chunks,"hardwareWritten":false,"message":"Local draft only. Hardware writing is not enabled."}),
    )
}
#[tauri::command]
pub fn store_edge_snapshot(data: EdgeSnapshot) -> Result<String, String> {
    data.validate()?;
    let p = root()?.join("edge-backups");
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    if fs::read_dir(&p)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
        .count()
        >= 128
    {
        return Err(
            "128 Edge backups are already saved. Export and remove older backups first.".into(),
        );
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let id = format!("{stamp}");
    crate::profiles::write_archive(
        &p.join(format!("{id}.json")),
        &serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?,
    )?;
    Ok(id)
}
#[tauri::command]
pub fn edge_snapshots() -> Result<Vec<serde_json::Value>, String> {
    let p = root()?.join("edge-backups");
    if !p.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for f in fs::read_dir(p).map_err(|e| e.to_string())?.take(256) {
        let f = f.map_err(|e| e.to_string())?;
        let path = f.path();
        if f.file_type().map_err(|e| e.to_string())?.is_file()
            && path.extension().is_some_and(|s| s == "json")
        {
            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if id.len() > 40 || !id.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let result = (|| {
                let data: EdgeSnapshot = serde_json::from_slice(&read_bounded(&path, 16 * 1024)?)
                    .map_err(|e| e.to_string())?;
                data.validate()?;
                Ok::<_, String>(data)
            })();
            entries.push(match result {
                Ok(data) => serde_json::json!({"id":id,"data":data}),
                Err(error) => serde_json::json!({"id":id,"error":error}),
            });
            if entries.len() >= 128 {
                break;
            }
        }
    }
    entries.sort_by(|a, b| b["id"].as_str().cmp(&a["id"].as_str()));
    Ok(entries)
}
#[tauri::command]
pub fn delete_edge_snapshot(id: String) -> Result<(), String> {
    if id.is_empty() || id.len() > 40 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Choose a saved backup ID".into());
    }
    let path = root()?.join("edge-backups").join(format!("{id}.json"));
    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("The backup is not an ordinary file".into());
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> EdgeSnapshot {
        let mut s = EdgeSnapshot {
            schema_version: 1,
            controller_id: "pad-test".into(),
            firmware: None,
            captured_at: 1,
            slots: (0..4)
                .map(|i| {
                    (0..3)
                        .map(|j| {
                            let mut b = vec![0; 64];
                            b[0] = 0x70 + (i * 3 + j) as u8;
                            b[2] = 1;
                            b
                        })
                        .collect()
                })
                .collect(),
        };
        for b in &mut s.slots {
            b[2][5] = 255;
            b[2][7] = 255;
            let crc = content_crc(b);
            b[2][56..60].copy_from_slice(&crc.to_le_bytes());
        }
        s
    }
    #[test]
    fn edge_edit_preserves_original_and_rejects_default_or_corrupt_data() {
        let s = snapshot();
        let before = s.slots.clone();
        let make = || EdgeEdit {
            name: "Bow".into(),
            left_range: [0, 240],
            right_range: [10, 250],
            grip_level: 2,
            trigger_level: 6,
        };
        assert!(edit_edge_snapshot(s.clone(), 0, make()).is_err());
        let result = edit_edge_snapshot(s.clone(), 1, make()).unwrap();
        assert_eq!(s.slots, before);
        assert_eq!(result["hardwareWritten"], false);
        let chunks: Vec<Vec<u8>> = serde_json::from_value(result["readReports"].clone()).unwrap();
        assert_eq!(
            content_crc(&chunks),
            u32::from_le_bytes(chunks[2][56..60].try_into().unwrap())
        );
        assert_eq!(&chunks[1][28..60], &before[1][1][28..60]);
        let mut bad = s;
        bad.slots[1][0][8] ^= 1;
        assert!(edit_edge_snapshot(bad, 1, make()).is_err());
    }
    #[test]
    fn roster_rejects_duplicate_players_or_endpoints() {
        let c = ControllerRecord {
            controller_id: "a".into(),
            name: "Desk".into(),
            player: 1,
            endpoint_id: Some("audio-id".into()),
        };
        let mut b = c.clone();
        b.controller_id = "b".into();
        let mut l = Library {
            schema_version: 1,
            controllers: vec![c, b],
        };
        assert!(l.validate().is_err());
        l.controllers[1].player = 2;
        assert!(l.validate().is_err());
        l.controllers[1].endpoint_id = None;
        assert!(l.validate().is_ok());
    }
    #[test]
    fn edge_snapshot_rejects_partial_or_reordered_reports() {
        let mut s = snapshot();
        s.slots[0][1].pop();
        assert!(s.validate().is_err());
    }
}
