//! Opt-in USB Edge transactions. Never used by discovery or automated hardware checks.
use crate::hardware::{EdgeEdit, EdgeSnapshot};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

fn content(slot: &[Vec<u8>]) -> Vec<u8> {
    slot.iter().flat_map(|r| r[2..60].iter().copied()).collect()
}
pub fn validate_request(
    data: &EdgeSnapshot,
    slot: usize,
    edit: Option<EdgeEdit>,
) -> Result<(), String> {
    target(data, slot, edit).map(|_| ())
}
fn target(
    data: &EdgeSnapshot,
    slot: usize,
    edit: Option<EdgeEdit>,
) -> Result<Vec<Vec<u8>>, String> {
    data.validate()?;
    if !(1..=3).contains(&slot) {
        return Err("The default Edge slot is read-only.".into());
    }
    let description = data.describe()?;
    if data.slots[slot][0][2] != 1
        || !description[slot].assigned
        || !description[slot].checksum_valid
    {
        return Err("The selected backup has an empty, unknown or damaged profile. Keep its original bytes.".into());
    }
    if let Some(edit) = edit {
        serde_json::from_value(
            crate::hardware::edit_edge_snapshot(data.clone(), slot, edit)?["readReports"].clone(),
        )
        .map_err(|e| e.to_string())
    } else {
        Ok(data.slots[slot].clone())
    }
}
fn write_reports(slot: usize, chunks: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, String> {
    if !(1..=3).contains(&slot) || chunks.len() != 3 || chunks.iter().any(|c| c.len() != 64) {
        return Err("Invalid Edge write plan.".into());
    }
    Ok(chunks
        .iter()
        .enumerate()
        .map(|(part, chunk)| {
            let mut report = chunk.clone();
            report[0] = 0x5f + slot as u8;
            report[1] = part as u8;
            let crc = aftertouch_protocol::crc32_seeded(0x53, &report[..60]);
            report[60..64].copy_from_slice(&crc.to_le_bytes());
            report
        })
        .collect())
}
trait SlotIo {
    fn read(&mut self) -> Result<EdgeSnapshot, String>;
    fn set(&mut self, report: &[u8]) -> Result<(), String>;
}
fn transactions() -> Result<Vec<(std::path::PathBuf, Value)>, String> {
    let folder = crate::hardware::root()?.join("edge-transactions");
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut records = Vec::new();
    for entry in entries.take(1024) {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().is_none_or(|s| s != "json") {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16 * 1024 {
            return Err("An Edge transaction record is not a normal bounded file.".into());
        }
        let data: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if data["schemaVersion"] != 1 {
            return Err("An Edge transaction record has an unsupported version.".into());
        }
        records.push((path, data));
        if records.len() > 128 {
            return Err("Too many Edge transaction records; export older history.".into());
        }
    }
    Ok(records)
}
pub fn backup_protected(id: &str) -> Result<bool, String> {
    Ok(transactions()?.iter().any(|(_, record)| {
        record["originalBackupId"].as_str() == Some(id)
            && !matches!(record["phase"].as_str(), Some("verified" | "recovered"))
    }))
}
fn mark_recovered(source: &EdgeSnapshot, slot: usize) -> Result<(), String> {
    for (path, mut record) in transactions()? {
        if record["controllerId"] != source.controller_id
            || record["slot"] != slot
            || matches!(record["phase"].as_str(), Some("verified" | "recovered"))
        {
            continue;
        }
        let Some(id) = record["originalBackupId"].as_str() else {
            continue;
        };
        if id.is_empty() || id.len() > 40 || !id.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let backup = crate::hardware::root()?
            .join("edge-backups")
            .join(format!("{id}.json"));
        let metadata = std::fs::symlink_metadata(&backup).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 {
            continue;
        }
        let original: EdgeSnapshot =
            serde_json::from_slice(&std::fs::read(backup).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        original.validate()?;
        if content(&original.slots[slot]) == content(&source.slots[slot]) {
            record["phase"] = json!("recovered");
            crate::profiles::write_archive(
                &path,
                &serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?,
            )?;
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn edge_transactions() -> Result<Vec<Value>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        transactions().map(|records| {
            records
                .into_iter()
                .map(|(path, mut record)| {
                    record["id"] = json!(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
                    record
                })
                .collect()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn archive_edge_transaction(id: String) -> Result<(), String> {
    if id.is_empty() || id.len() > 40 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Choose a saved transaction ID.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let records=transactions()?;let (path,data)=records.into_iter().find(|(p,_)|p.file_stem().and_then(|s|s.to_str())==Some(&id)).ok_or("Transaction not found.")?;
        if !matches!(data["phase"].as_str(),Some("verified"|"recovered")){return Err("Restore and verify the original backup before archiving an unfinished transaction.".into());}
        std::fs::remove_file(path).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
#[derive(Default)]
struct Progress {
    backup: Option<String>,
    written: usize,
}
struct CommitRequest<'a> {
    source: &'a EdgeSnapshot,
    slot: usize,
    edit: Option<EdgeEdit>,
}
fn transact(
    io: &mut impl SlotIo,
    request: CommitRequest<'_>,
    progress: &mut Progress,
    mut backup: impl FnMut(EdgeSnapshot) -> Result<String, String>,
    mut journal: impl FnMut(&str, &Progress) -> Result<(), String>,
    cancelled: &AtomicBool,
) -> Result<EdgeSnapshot, String> {
    let CommitRequest { source, slot, edit } = request;
    let restoring = edit.is_none();
    let desired = target(source, slot, edit)?;
    let fresh = io.read()?;
    fresh.validate()?;
    if fresh.controller_id != source.controller_id {
        return Err("Controller identity changed; nothing was written.".into());
    }
    if !restoring && content(&fresh.slots[slot]) != content(&source.slots[slot]) {
        return Err("The onboard slot changed. Read profiles again before editing.".into());
    }
    if cancelled.load(Ordering::Acquire) {
        return Err("Edge write cancelled before commit.".into());
    }
    progress.backup = Some(backup(fresh.clone())?);
    journal("prepared", progress)?;
    let reports = write_reports(slot, &desired)?;
    if cancelled.load(Ordering::Acquire) {
        return Err("Edge write cancelled; the original backup is saved.".into());
    }
    // Complete all three pages once commit starts; do not leave a partial slot on an ordinary cancellation.
    for report in reports {
        journal("writing", progress)?;
        io.set(&report)?;
        progress.written += 1;
    }
    let after = io.read()?;
    after.validate()?;
    if after.controller_id != fresh.controller_id
        || content(&after.slots[slot]) != content(&desired)
    {
        return Err("Readback did not match. Keep the original backup and restore it before using this slot.".into());
    }
    for other in 0..4 {
        if other != slot && content(&after.slots[other]) != content(&fresh.slots[other]) {
            return Err("Another onboard slot changed during verification. Keep both snapshots and inspect the controller.".into());
        }
    }
    journal("verified", progress)?;
    Ok(after)
}
pub struct Worker {
    rx: Receiver<Value>,
    cancel: Arc<AtomicBool>,
    request_id: u32,
    owner: String,
}
impl Worker {
    pub fn poll(&self) -> Option<Value> {
        match self.rx.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(
                json!({"type":"edgeWriteStatus","requestId":self.request_id,"controllerId":self.owner,"verified":false,"error":"The Edge worker ended unexpectedly. Keep the saved original backup and inspect the transaction before retrying."}),
            ),
        }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(1500) {
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(_) => {}
            }
        }
    }
}
pub fn start(
    path: std::ffi::CString,
    source: EdgeSnapshot,
    slot: usize,
    edit: Option<EdgeEdit>,
    request_id: u32,
) -> Result<Worker, String> {
    target(&source, slot, edit.clone())?;
    if crate::hardware::EDGE_READING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("An Edge operation is already running.".into());
    }
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let owner = source.controller_id.clone();
    let spawn=std::thread::Builder::new().name("edge-profile-transaction".into()).spawn(move || {
        struct Busy; impl Drop for Busy { fn drop(&mut self) { crate::hardware::EDGE_READING.store(false,Ordering::Release); } } let _busy=Busy;
        let mut progress=Progress::default();
        let operation=(|| {
            let dir=crate::hardware::root()?.join("edge-transactions");std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
            if transactions()?.len()>=128 {return Err("Export and archive older Edge transactions before writing another profile.".into());}
            let id=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
            let journal_path=dir.join(format!("{id}.json"));
            let mut journal=|phase:&str,p:&Progress| crate::profiles::write_archive(&journal_path,&serde_json::to_vec_pretty(&json!({"schemaVersion":1,"controllerId":source.controller_id,"slot":slot,"phase":phase,"originalBackupId":p.backup,"pagesAcknowledged":p.written,"requestId":request_id})).map_err(|e|e.to_string())?);
            let api=hidapi::HidApi::new().map_err(|e|e.to_string())?;
            let info=api.device_list().find(|d|d.path()==path.as_c_str()&&d.vendor_id()==aftertouch_protocol::SONY_VID&&d.product_id()==aftertouch_protocol::EDGE_PID&&matches!(d.bus_type(),hidapi::BusType::Usb)&&d.usage_page()==1&&d.usage()==5).ok_or("Reconnect the same Edge over USB before writing.")?;
            let hid=info.open_device(&api).map_err(|e|e.to_string())?;
            if crate::device::controller_identity(info,&hid)!=source.controller_id {return Err("The selected controller identity changed.".into());}
            let mut io=UsbSlot { hid, owner:source.controller_id.clone(), firmware:source.firmware.clone(), started:Instant::now() };
            let restoring=edit.is_none();let result=transact(&mut io,CommitRequest { source: &source, slot, edit },&mut progress,crate::hardware::store_edge_snapshot,&mut journal,&signal);
            if result.is_ok()&&restoring {let _=mark_recovered(&source,slot);}
            if result.is_err() {let _=journal("failed",&progress);} result
        })();
        let event=match operation {Ok(data)=>json!({"type":"edgeWriteStatus","requestId":request_id,"controllerId":source.controller_id,"verified":true,"originalBackupId":progress.backup,"pagesAcknowledged":progress.written,"data":data}),Err(error)=>json!({"type":"edgeWriteStatus","requestId":request_id,"controllerId":source.controller_id,"verified":false,"originalBackupId":progress.backup,"pagesAcknowledged":progress.written,"error":error})};let _=tx.send(event);
    });
    if let Err(e) = spawn {
        crate::hardware::EDGE_READING.store(false, Ordering::Release);
        return Err(e.to_string());
    }
    Ok(Worker {
        rx,
        cancel,
        request_id,
        owner,
    })
}
struct UsbSlot {
    hid: hidapi::HidDevice,
    owner: String,
    firmware: Option<String>,
    started: Instant,
}
impl UsbSlot {
    fn deadline(&self) -> Result<(), String> {
        if self.started.elapsed() > Duration::from_secs(8) {
            Err("Edge operation timed out. Keep the original backup for recovery.".into())
        } else {
            Ok(())
        }
    }
}
impl SlotIo for UsbSlot {
    fn read(&mut self) -> Result<EdgeSnapshot, String> {
        let mut slots = Vec::new();
        for slot in 0..4 {
            let mut pages = Vec::new();
            for part in 0..3 {
                self.deadline()?;
                let id = 0x70 + (slot * 3 + part) as u8;
                let mut report = vec![0; 64];
                report[0] = id;
                let count = self
                    .hid
                    .get_feature_report(&mut report)
                    .map_err(|e| e.to_string())?;
                if count != 64 || report[0] != id {
                    return Err("Incomplete Edge readback.".into());
                }
                pages.push(report);
            }
            slots.push(pages);
        }
        Ok(EdgeSnapshot {
            schema_version: 1,
            controller_id: self.owner.clone(),
            firmware: self.firmware.clone(),
            captured_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as u64,
            slots,
        })
    }
    fn set(&mut self, report: &[u8]) -> Result<(), String> {
        self.deadline()?;
        self.hid
            .send_feature_report(report)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn crc(bytes: impl Iterator<Item = u8>) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320u32.wrapping_mul(crc & 1));
            }
        }
        !crc
    }
    fn fixture() -> EdgeSnapshot {
        let mut data = EdgeSnapshot {
            schema_version: 1,
            controller_id: "edge-test".into(),
            firmware: Some("test".into()),
            captured_at: 1,
            slots: vec![vec![vec![0; 64]; 3]; 4],
        };
        for (slot, pages) in data.slots.iter_mut().enumerate() {
            for (part, report) in pages.iter_mut().enumerate() {
                report[0] = 0x70 + (slot * 3 + part) as u8;
                report[1] = part as u8;
            }
            pages[0][2] = 1;
            pages[2][5] = 255;
            pages[2][7] = 255;
            let sum = crc(pages[0][2..60]
                .iter()
                .chain(&pages[1][2..60])
                .chain(&pages[2][2..56])
                .copied());
            pages[2][56..60].copy_from_slice(&sum.to_le_bytes());
        }
        data
    }
    fn edit() -> EdgeEdit {
        EdgeEdit {
            name: "New bow".into(),
            left_range: [5, 250],
            right_range: [8, 245],
            grip_level: 2,
            trigger_level: 6,
        }
    }
    struct Fake {
        data: EdgeSnapshot,
        backup: Arc<AtomicBool>,
        writes: usize,
        fail_after: usize,
        mismatch: bool,
        reads: usize,
    }
    impl Fake {
        fn new(data: EdgeSnapshot) -> Self {
            Self {
                data,
                backup: Arc::new(AtomicBool::new(false)),
                writes: 0,
                fail_after: usize::MAX,
                mismatch: false,
                reads: 0,
            }
        }
    }
    impl SlotIo for Fake {
        fn read(&mut self) -> Result<EdgeSnapshot, String> {
            self.reads += 1;
            let mut data = self.data.clone();
            if self.mismatch && self.reads > 1 {
                data.slots[1][2][5] ^= 1;
            }
            Ok(data)
        }
        fn set(&mut self, report: &[u8]) -> Result<(), String> {
            assert!(
                self.backup.load(Ordering::Acquire),
                "a durable backup must precede every SET"
            );
            if self.writes >= self.fail_after {
                return Err("injected disconnect".into());
            }
            let slot = usize::from(report[0] - 0x5f);
            let page = usize::from(report[1]);
            assert!((1..=3).contains(&slot));
            assert_eq!(
                u32::from_le_bytes(report[60..64].try_into().unwrap()),
                aftertouch_protocol::crc32_seeded(0x53, &report[..60])
            );
            self.data.slots[slot][page][2..60].copy_from_slice(&report[2..60]);
            self.writes += 1;
            Ok(())
        }
    }
    fn run(
        io: &mut Fake,
        source: &EdgeSnapshot,
        edit: Option<EdgeEdit>,
        progress: &mut Progress,
    ) -> Result<EdgeSnapshot, String> {
        let saved = io.backup.clone();
        transact(
            io,
            CommitRequest {
                source,
                slot: 1,
                edit,
            },
            progress,
            move |_| {
                saved.store(true, Ordering::Release);
                Ok("saved-original".into())
            },
            |_, _| Ok(()),
            &AtomicBool::new(false),
        )
    }
    #[test]
    fn successful_write_preserves_other_slots_and_restore_replays_original() {
        let source = fixture();
        let mut io = Fake::new(source.clone());
        let mut p = Progress::default();
        let after = run(&mut io, &source, Some(edit()), &mut p).unwrap();
        assert_eq!(io.writes, 3);
        assert_eq!(p.backup.as_deref(), Some("saved-original"));
        assert_eq!(after.slots[0], source.slots[0]);
        assert_eq!(after.slots[2], source.slots[2]);
        assert!(after.describe().unwrap()[1].checksum_valid);
        assert_eq!(after.describe().unwrap()[1].name, "New bow");
        run(&mut io, &source, None, &mut Progress::default()).unwrap();
        assert_eq!(content(&io.data.slots[1]), content(&source.slots[1]));
    }
    #[test]
    fn stale_identity_slot_and_invalid_target_never_write() {
        let source = fixture();
        for identity in [false, true] {
            let mut io = Fake::new(source.clone());
            if identity {
                io.data.controller_id = "other".into();
            } else {
                io.data.slots[1][0][10] = 1;
            }
            assert!(run(&mut io, &source, Some(edit()), &mut Progress::default()).is_err());
            assert_eq!(io.writes, 0);
            assert!(!io.backup.load(Ordering::Acquire));
        }
        assert!(write_reports(0, &source.slots[0]).is_err());
        let mut corrupt = source.clone();
        corrupt.slots[1][0][2] = 2;
        assert!(target(&corrupt, 1, None).is_err());
        corrupt.slots[1][0].truncate(10);
        assert!(target(&corrupt, 1, None).is_err());
    }
    #[test]
    fn backup_or_journal_failure_prevents_commit_and_partial_failure_keeps_recovery_id() {
        let source = fixture();
        let mut io = Fake::new(source.clone());
        let mut p = Progress::default();
        assert!(
            transact(
                &mut io,
                CommitRequest {
                    source: &source,
                    slot: 1,
                    edit: Some(edit())
                },
                &mut p,
                |_| Err("disk full".into()),
                |_, _| Ok(()),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(io.writes, 0);
        assert!(p.backup.is_none());
        let saved = io.backup.clone();
        assert!(
            transact(
                &mut io,
                CommitRequest {
                    source: &source,
                    slot: 1,
                    edit: Some(edit())
                },
                &mut p,
                move |_| {
                    saved.store(true, Ordering::Release);
                    Ok("original".into())
                },
                |_, _| Err("journal failure".into()),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(io.writes, 0);
        io.fail_after = 1;
        assert!(run(&mut io, &source, Some(edit()), &mut p).is_err());
        assert_eq!(p.written, 1);
        assert!(p.backup.is_some());
    }
    #[test]
    fn readback_mismatch_and_cancel_are_not_reported_as_success() {
        let source = fixture();
        let mut io = Fake::new(source.clone());
        io.mismatch = true;
        assert!(
            run(&mut io, &source, Some(edit()), &mut Progress::default())
                .unwrap_err()
                .contains("Readback")
        );
        let mut io = Fake::new(source.clone());
        assert!(
            transact(
                &mut io,
                CommitRequest {
                    source: &source,
                    slot: 1,
                    edit: Some(edit())
                },
                &mut Progress::default(),
                |_| panic!("cancel must precede backup"),
                |_, _| Ok(()),
                &AtomicBool::new(true)
            )
            .is_err()
        );
        assert_eq!(io.writes, 0);
    }
}
