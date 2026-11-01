//! Controlled integration with an installed vDS runtime. No driver installation.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Inspect,
    Attach,
    Detach,
    ReleaseRecord,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lease {
    schema_version: u8,
    pub controller_id: String,
    address: String,
    profile: String,
    phase: String,
}
impl Lease {
    pub fn new(controller_id: String, address: String, edge: bool) -> Result<Self, String> {
        if controller_id.is_empty() || controller_id.len() > 200 {
            return Err("Select a controller first.".into());
        }
        Ok(Self {
            schema_version: 1,
            controller_id,
            address: normalize_address(&address)?,
            profile: if edge { "dse" } else { "ds5" }.into(),
            phase: "pending".into(),
        })
    }
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.controller_id.is_empty()
            || self.controller_id.len() > 200
            || !matches!(self.profile.as_str(), "ds5" | "dse")
            || !matches!(
                self.phase.as_str(),
                "pending" | "attached" | "unknown" | "detaching"
            )
        {
            return Err("Invalid native bridge ownership record.".into());
        }
        normalize_address(&self.address).map(|_| ())
    }
}
pub fn normalize_address(value: &str) -> Result<String, String> {
    if !matches!(value.len(), 12 | 17) {
        return Err("A valid controller Bluetooth address is required.".into());
    }
    let hex: String = value.chars().filter(|c| !matches!(c, ':' | '-')).collect();
    if hex.len() != 12
        || !hex.bytes().all(|b| b.is_ascii_hexdigit())
        || hex.bytes().all(|b| b == b'0')
        || hex.bytes().all(|b| b == b'f' || b == b'F')
    {
        return Err("A valid controller Bluetooth address is required.".into());
    }
    Ok(hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| String::from_utf8_lossy(pair).to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(":"))
}
pub fn tool_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .map(|p| vec![p.join("vDS/vdsctl.exe")])
        .unwrap_or_default();
    #[cfg(target_os = "linux")]
    let candidates = vec![
        PathBuf::from("/usr/local/bin/vdsctl"),
        PathBuf::from("/usr/bin/vdsctl"),
        PathBuf::from("/usr/sbin/vdsctl"),
    ];
    #[cfg(not(any(windows, target_os = "linux")))]
    let candidates: Vec<PathBuf> = Vec::new();
    candidates.into_iter().find(|p| p.is_file())
}
fn arguments(operation: Operation, lease: Option<&Lease>) -> Result<Vec<String>, String> {
    let args = match operation {
        Operation::Inspect => vec!["list".into()],
        Operation::Attach => {
            let l = lease.ok_or("No controller selected for the bridge.")?;
            l.validate()?;
            vec![
                "attach".into(),
                l.address.clone(),
                "--profile".into(),
                l.profile.clone(),
                "--ports".into(),
                "0".into(),
            ]
        }
        Operation::Detach => vec![
            "detach".into(),
            lease.ok_or("No app-owned bridge session.")?.address.clone(),
        ],
        Operation::ReleaseRecord => return Err("Record release does not call the provider.".into()),
    };
    Ok(args)
}
fn run_cli(path: &Path, args: &[String]) -> Result<String, String> {
    let mut command = Command::new(path);
    command
        .args(args)
        .current_dir(path.parent().ok_or("Provider folder missing")?)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start installed vdsctl: {e}"))?;
    let stdout = child.stdout.take().ok_or("Provider output pipe missing")?;
    let stderr = child.stderr.take().ok_or("Provider error pipe missing")?;
    let (tx, rx) = mpsc::sync_channel(2);
    let err_tx = tx.clone();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let result = stdout.take(32 * 1024 + 1).read_to_end(&mut data);
        let _ = tx.send((false, result.map(|_| data)));
    });
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let result = stderr.take(8 * 1024 + 1).read_to_end(&mut data);
        let _ = err_tx.send((true, result.map(|_| data)));
    });
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Bridge request timed out. Ownership is uncertain; inspect or detach before resuming monitoring.".into());
            }
        }
    };
    let mut out = Vec::new();
    let mut error = Vec::new();
    for _ in 0..2 {
        let (is_error, result) = rx
            .recv_timeout(Duration::from_millis(250))
            .map_err(|_| "Provider output did not finish")?;
        let bytes = result.map_err(|e| e.to_string())?;
        if is_error { error = bytes } else { out = bytes }
    }
    if out.len() > 32 * 1024 || error.len() > 8 * 1024 {
        return Err("Provider reply exceeds its size limit.".into());
    }
    if !status.success() {
        return Err(String::from_utf8_lossy(&error).chars().take(1024).collect());
    }
    String::from_utf8(out).map_err(|_| "Provider reply is not UTF-8.".into())
}
fn records(text: &str) -> Result<Vec<Value>, String> {
    if text.len() > 32 * 1024 {
        return Err("Provider reply is too large.".into());
    }
    let mut records = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if records.len() >= 64 {
            return Err("Too many provider records.".into());
        }
        let record: Value =
            serde_json::from_str(line).map_err(|_| "Provider returned invalid JSONL.")?;
        if record["OK"] == false {
            return Err(record["error"]
                .as_str()
                .unwrap_or("Provider rejected the request")
                .into());
        }
        records.push(record);
    }
    Ok(records)
}
fn controller_present(records: &[Value], address: &str) -> bool {
    records.iter().any(|r| {
        r["address"]
            .as_str()
            .and_then(|a| normalize_address(a).ok())
            .as_deref()
            == Some(address)
    })
}
fn controller_records(text: &str) -> Result<Vec<Value>, String> {
    let values = records(text)?;
    for value in &values {
        if value["connected"].as_bool().is_none()
            || value["address"]
                .as_str()
                .and_then(|s| normalize_address(s).ok())
                .is_none()
        {
            return Err("Provider controller readback has an unsupported format; monitoring remains paused.".into());
        }
    }
    Ok(values)
}
fn execute(path: &Path, operation: Operation, lease: Option<&Lease>) -> Result<Value, String> {
    if operation == Operation::Attach {
        let reply = records(&run_cli(path, &arguments(operation, lease)?)?)?;
        let expected = lease.ok_or("Missing native bridge target.")?;
        if reply.len() != 1
            || reply[0]["OK"] != true
            || reply[0]["address"]
                .as_str()
                .and_then(|s| normalize_address(s).ok())
                .as_deref()
                != Some(&expected.address)
            || reply[0]["profile"] != expected.profile
            || reply[0]["ports"] != json!([0])
        {
            return Err(
                "Provider did not acknowledge the requested controller, model and port.".into(),
            );
        }
    } else if operation == Operation::Detach {
        // A previous interrupted detach may already have removed the registration.
        // A valid fresh list, not an exit code alone, determines whether to resume.
        let _ = run_cli(path, &arguments(operation, lease)?);
    }
    let current = controller_records(&run_cli(path, &["list".into()])?)?;
    let targets = if operation == Operation::Detach {
        Vec::new()
    } else {
        records(&run_cli(path, &["list-targets".into()])?)?
    };
    if let Some(l) = lease {
        let present = controller_present(&current, &l.address);
        if operation == Operation::Attach && !present {
            return Err("Attach was acknowledged but not found in the bridge readback.".into());
        }
        if operation == Operation::Detach && present {
            return Err(
                "The controller is still registered with the bridge; monitoring remains paused."
                    .into(),
            );
        }
    }
    let connected = current.iter().filter(|r| r["connected"] == true).count();
    Ok(
        json!({"registered":current.len(),"connected":connected,"targets":targets.len(),"onlineTargets":targets.iter().filter(|r|r["online"]==true).count(),"ownedRegistered":lease.is_some_and(|l|controller_present(&current,&l.address)),"ownedConnected":lease.is_some_and(|l|current.iter().any(|r|r["address"].as_str().and_then(|a|normalize_address(a).ok()).as_deref()==Some(&l.address)&&r["connected"]==true))}),
    )
}
fn redact(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut start = 0;
    let mut i = 0;
    while i + 17 <= bytes.len() {
        if (0..17).all(|j| {
            if j % 3 == 2 {
                matches!(bytes[i + j], b':' | b'-')
            } else {
                bytes[i + j].is_ascii_hexdigit()
            }
        }) {
            out.push_str(&text[start..i]);
            out.push_str("[controller]");
            i += 17;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[start..]);
    out.chars().take(1024).collect()
}
struct Pending {
    receiver: mpsc::Receiver<Result<Value, String>>,
    operation: Operation,
    request_id: u32,
}
pub struct Manager {
    folder: PathBuf,
    pub session: Option<Lease>,
    recovery_error: Option<String>,
    pending: Option<Pending>,
    busy: Arc<AtomicBool>,
}
impl Manager {
    pub fn load(disable_hid: bool) -> Self {
        let folder = crate::hardware::root().unwrap_or_default();
        Self::from_folder(folder, disable_hid)
    }
    fn from_folder(folder: PathBuf, disable_hid: bool) -> Self {
        let mut manager = Self {
            folder,
            session: None,
            recovery_error: None,
            pending: None,
            busy: Arc::new(AtomicBool::new(false)),
        };
        if !disable_hid && !manager.folder.as_os_str().is_empty() {
            let path = manager.path();
            if path.exists() {
                let result = (|| {
                    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 8192 {
                        return Err("Invalid native bridge record file.".into());
                    }
                    let lease: Lease =
                        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                    lease.validate()?;
                    Ok(lease)
                })();
                match result {
                    Ok(lease) => manager.session = Some(lease),
                    Err(error) => manager.recovery_error = Some(error),
                }
            }
        }
        manager
    }
    fn path(&self) -> PathBuf {
        self.folder.join("native-provider-session.json")
    }
    fn save(&self) -> Result<(), String> {
        if self.folder.as_os_str().is_empty() {
            return Err("Native bridge settings folder is unavailable.".into());
        }
        std::fs::create_dir_all(&self.folder).map_err(|e| e.to_string())?;
        if let Some(session) = &self.session {
            crate::profiles::write_archive(
                &self.path(),
                &serde_json::to_vec(session).map_err(|e| e.to_string())?,
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(self.path(), std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| e.to_string())?;
            }
        } else if self.path().exists() {
            std::fs::remove_file(self.path()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn owns_controller(&self) -> bool {
        self.session.is_some() || self.recovery_error.is_some()
    }
    pub fn abandon_unstarted_attach(&mut self) -> Result<(), String> {
        if self.pending.is_some()
            || self.busy.load(Ordering::Acquire)
            || self.session.as_ref().is_none_or(|l| l.phase != "pending")
        {
            return Err("A bridge request may already own the controller.".into());
        }
        let saved = self.session.take();
        if let Err(error) = self.save() {
            self.session = saved;
            return Err(error);
        }
        Ok(())
    }
    pub fn status(&self) -> Value {
        json!({"type":"nativeProviderStatus","owned":self.owns_controller(),"busy":self.pending.is_some(),"owner":self.session.as_ref().map(|l|&l.controller_id),"phase":self.session.as_ref().map(|l|l.phase.as_str()).unwrap_or(if self.recovery_error.is_some(){"unknown"}else{"idle"}),"error":self.recovery_error.as_ref().map(|e|redact(e)),"experimental":true})
    }
    pub fn prepare_attach(&mut self, lease: Lease) -> Result<(), String> {
        if self.owns_controller() || self.pending.is_some() || self.busy.load(Ordering::Acquire) {
            return Err("Detach the current app-owned bridge session first.".into());
        }
        if tool_path().is_none() {
            return Err("Install vDS and its required drivers first; vdsctl was not found in a standard install folder.".into());
        }
        self.session = Some(lease);
        if let Err(error) = self.save() {
            self.session = None;
            return Err(error);
        }
        Ok(())
    }
    pub fn start(
        &mut self,
        operation: Operation,
        request_id: u32,
        confirmed: bool,
    ) -> Result<(), String> {
        if self.pending.is_some() || self.busy.load(Ordering::Acquire) {
            return Err("A native bridge request is still finishing.".into());
        }
        if operation == Operation::ReleaseRecord {
            if !confirmed {
                return Err(
                    "Confirm that the external bridge is stopped before releasing its record."
                        .into(),
                );
            }
            let saved = self.session.take();
            let error = self.recovery_error.take();
            if let Err(e) = self.save() {
                self.session = saved;
                self.recovery_error = error;
                return Err(e);
            }
            return Ok(());
        }
        if operation == Operation::Attach && self.session.is_none() {
            return Err("Select a Bluetooth controller before attaching.".into());
        }
        if operation == Operation::Detach && self.session.is_none() {
            return Err("No app-owned bridge session to detach. Use external recovery if its record is damaged.".into());
        }
        let path = tool_path().ok_or("Installed vdsctl was not found. Use the setup guide.")?;
        let lease = self.session.clone();
        let (tx, receiver) = mpsc::sync_channel(1);
        let busy = self.busy.clone();
        self.busy.store(true, Ordering::Release);
        if let Err(e) = std::thread::Builder::new()
            .name("native-provider-control".into())
            .spawn(move || {
                struct Done(Arc<AtomicBool>);
                impl Drop for Done {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release);
                    }
                }
                let done = Done(busy);
                let result = execute(&path, operation, lease.as_ref()).map_err(|e| redact(&e));
                drop(done);
                let _ = tx.send(result);
            })
        {
            self.busy.store(false, Ordering::Release);
            return Err(e.to_string());
        }
        self.pending = Some(Pending {
            receiver,
            operation,
            request_id,
        });
        Ok(())
    }
    pub fn poll(&mut self) -> Option<Value> {
        let pending = self.pending.as_ref()?;
        let result = match pending.receiver.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(_) => {
                Err("Native bridge worker ended unexpectedly; ownership remains paused.".into())
            }
        };
        let pending = self.pending.take().unwrap();
        let result = result.and_then(|data| {
            if pending.operation == Operation::Detach {
                let saved = self.session.take();
                if let Err(e) = self.save() {
                    self.session = saved;
                    return Err(e);
                }
                self.recovery_error = None;
            } else if pending.operation == Operation::Attach {
                if let Some(l) = self.session.as_mut() {
                    l.phase = "attached".into();
                }
                self.save()?;
            }
            Ok(data)
        });
        if result.is_err() && pending.operation != Operation::Inspect {
            if let Some(l) = self.session.as_mut() {
                l.phase = "unknown".into();
            }
            let _ = self.save();
        }
        let mut event = self.status();
        event["requestId"] = json!(pending.request_id);
        match result {
            Ok(data) => {
                event["verified"] = json!(true);
                event["data"] = data;
            }
            Err(error) => event["error"] = json!(redact(&error)),
        };
        Some(event)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addresses_arguments_and_reply_limits_are_strict() {
        assert_eq!(
            normalize_address("aabbccddeeff").unwrap(),
            "AA:BB:CC:DD:EE:FF"
        );
        for bad in [
            "--trace",
            "00:00:00:00:00:00",
            "FF:FF:FF:FF:FF:FF",
            "AA:BB:CC:DD:EE:FG",
            "aa;whoami",
        ] {
            assert!(normalize_address(bad).is_err());
        }
        let lease = Lease::new("pad-1".into(), "aabbccddeeff".into(), false).unwrap();
        assert_eq!(
            arguments(Operation::Attach, Some(&lease)).unwrap(),
            [
                "attach",
                "AA:BB:CC:DD:EE:FF",
                "--profile",
                "ds5",
                "--ports",
                "0"
            ]
        );
        assert!(
            records("{\"OK\":false,\"error\":\"driver unavailable\"}\n")
                .unwrap_err()
                .contains("driver")
        );
        assert!(records("not json").is_err());
        assert!(records(&" ".repeat(33000)).is_err());
        assert!(records("").unwrap().is_empty());
    }
    #[test]
    fn jsonl_readback_and_redaction_do_not_expose_hardware_addresses() {
        let list = records("{\"address\":\"AA:BB:CC:DD:EE:FF\",\"connected\":true}\n").unwrap();
        assert!(controller_present(&list, "AA:BB:CC:DD:EE:FF"));
        assert!(!controller_present(&list, "AA:BB:CC:DD:EE:00"));
        assert_eq!(
            redact("failed AA:BB:CC:DD:EE:FF or 11-22-33-44-55-66"),
            "failed [controller] or [controller]"
        );
    }
    #[test]
    fn malformed_controller_lists_cannot_prove_detachment() {
        for reply in [
            "null\n",
            "{}\n",
            "{\"OK\":true}\n",
            "{\"address\":\"AA:BB:CC:DD:EE:FF\"}\n",
        ] {
            assert!(controller_records(reply).is_err());
        }
        assert!(controller_records("").unwrap().is_empty());
    }
    #[test]
    fn saved_and_damaged_ownership_records_fail_closed_and_require_explicit_recovery() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let base = std::env::temp_dir().canonicalize().unwrap();
        let folder = base.join(format!(
            "controller-provider-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut manager = Manager::from_folder(folder.clone(), true);
        manager.session =
            Some(Lease::new("pad-test".into(), "aabbccddeeff".into(), false).unwrap());
        manager.save().unwrap();
        let restored = Manager::from_folder(folder.clone(), false);
        assert!(restored.owns_controller());
        assert_eq!(restored.status()["phase"], "pending");
        assert!(!Manager::from_folder(folder.clone(), true).owns_controller());
        std::fs::write(manager.path(), b"{damaged").unwrap();
        let mut damaged = Manager::from_folder(folder.clone(), false);
        assert!(damaged.owns_controller());
        assert_eq!(damaged.status()["phase"], "unknown");
        assert!(damaged.start(Operation::ReleaseRecord, 1, false).is_err());
        assert!(damaged.path().exists());
        damaged.start(Operation::ReleaseRecord, 2, true).unwrap();
        assert!(!damaged.owns_controller());
        assert!(!damaged.path().exists());
        let resolved = folder.canonicalize().unwrap();
        assert!(resolved.starts_with(&base) && resolved != base);
        std::fs::remove_dir_all(resolved).unwrap();
    }
}
