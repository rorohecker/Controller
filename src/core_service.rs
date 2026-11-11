//! Per-user background core, authenticated loopback JSONL, bounded clients and queues.
use crate::{
    core_io::{Line, Lines, MAX_LINE, Outbox},
    device::{self, DeviceCommand},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
const BUILD: &str = env!("CONTROLLER_CORE_BUILD");
/// Authenticated GUI attachments.
const MAX_CLIENTS: usize = 3;
/// Sockets still in their handshake; separate so unauthenticated peers cannot lock the GUI out.
const MAX_PENDING: usize = 8;
const HANDSHAKE: Duration = Duration::from_secs(2);
const QUEUE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REMEMBERED_OWNERS: usize = 8;
/// Settings replayed to a reconnecting controller while no GUI is attached.
const REMEMBERED: &[&str] = &[
    "setPersonalSettings",
    "setAudioMixer",
    "setHapticOutputLevel",
    "setControlProfile",
    "setGyroAim",
    "setRemap",
    "setPcMode",
    "setPcBindings",
    "setTouchMouse",
    "setGestures",
    "setGameActions",
    "setGameLink",
    "setInputEffects",
    "applyTriggers",
    "setScript",
    "setBridgeLatency",
    "setBridgeCustom",
    "setBridgeOptions",
    "setRemapTiming",
    "setPadExtras",
    "setAutoBias",
];
/// Scripts and key injection are replayed only when the command carries `"persistent": true`.
const RISKY: &[&str] = &["setScript", "setGameActions", "setPcBindings"];

pub fn build() -> &'static str {
    BUILD
}

#[derive(Clone)]
struct Config {
    folder: PathBuf,
    no_hid: bool,
    /// Pre-0.23 location under roaming app data; stopped on a best-effort basis.
    legacy: Option<PathBuf>,
}
impl Config {
    fn current(no_hid: bool) -> io::Result<Self> {
        let name = if no_hid { "service-no-hid" } else { "service" };
        let override_dir = if no_hid {
            std::env::var_os("CONTROLLER_SERVICE_STATE_DIR").map(PathBuf::from)
        } else {
            None
        };
        if let Some(folder) = override_dir {
            return Ok(Self {
                folder,
                no_hid,
                legacy: None,
            });
        }
        // Runtime state (endpoint + token + lock) is machine-local and user-private by the
        // inherited ACL of the local app-data folder; it must not roam.
        let folder = crate::app_log::local_base()
            .map_err(io::Error::other)?
            .join("CONTROLLER")
            .join(name);
        let legacy = crate::hardware::root().ok().map(|root| root.join(name));
        Ok(Self {
            folder,
            no_hid,
            legacy,
        })
    }
    fn endpoint(&self) -> PathBuf {
        self.folder.join("endpoint.json")
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Endpoint {
    protocol: u8,
    port: u16,
    pid: u32,
    build: String,
    token: String,
    no_hid: bool,
}
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |n, (a, b)| n | (a ^ b)) == 0
}
fn private_open(path: &Path) -> io::Result<File> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && (!meta.is_file() || meta.file_type().is_symlink())
    {
        return Err(io::Error::other("Service file is not an ordinary file"));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn read_endpoint(config: &Config) -> io::Result<Endpoint> {
    let path = config.endpoint();
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 2048 {
        return Err(io::Error::other("Invalid service endpoint file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::other(
                "Service endpoint permissions must be user-only",
            ));
        }
    }
    let endpoint: Endpoint = serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
    if endpoint.protocol != 1
        || endpoint.port == 0
        || endpoint.no_hid != config.no_hid
        || endpoint.token.len() != 64
        || !endpoint.token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(io::Error::other("Unsupported service endpoint"));
    }
    Ok(endpoint)
}
fn publish_endpoint(config: &Config, endpoint: &Endpoint) -> io::Result<()> {
    let temp = config
        .folder
        .join(format!("endpoint-{}.tmp", std::process::id()));
    let mut file = private_open(&temp)?;
    file.set_len(0)?;
    serde_json::to_writer(&mut file, endpoint).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    // std's rename replaces an existing destination atomically on every platform (MoveFileEx
    // with REPLACE_EXISTING on Windows), so there is no remove-then-rename gap. A reader that
    // briefly holds the destination can still make Windows refuse, so retry a few times.
    let mut last = Ok(());
    for attempt in 0..5 {
        last = fs::rename(&temp, config.endpoint());
        if last.is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20 * (attempt + 1)));
    }
    let _ = fs::remove_file(&temp);
    last
}
fn address(port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}
fn connect(config: &Config, stop: bool) -> io::Result<TcpStream> {
    connect_with(config, stop, BUILD)
}
fn connect_with(config: &Config, stop: bool, build: &str) -> io::Result<TcpStream> {
    let endpoint = read_endpoint(config)?;
    let mut stream =
        TcpStream::connect_timeout(&address(endpoint.port), Duration::from_millis(500))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let pid = std::env::var("CONTROLLER_GUI_PID")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(std::process::id());
    serde_json::to_writer(&mut stream,&json!({"protocol":1,"token":endpoint.token,"build":build,"mode":if stop{"stop"}else{"attach"},"pid":pid})).map_err(io::Error::other)?;
    stream.write_all(b"\n")?;
    // Read exactly the acknowledgement. A buffered reader could consume replay events here.
    let mut response = Vec::new();
    for _ in 0..2048 {
        let mut byte = [0];
        stream.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            break;
        }
        response.push(byte[0]);
    }
    let reply: Value = serde_json::from_slice(&response).map_err(io::Error::other)?;
    if reply["ok"] != true {
        return Err(io::Error::other(if reply["reason"] == "buildMismatch" {
            "background core build mismatch"
        } else {
            "background core authentication failed"
        }));
    }
    stream.set_read_timeout(None)?;
    Ok(stream)
}
fn connect_or_start(config: &Config) -> io::Result<TcpStream> {
    match connect(config, false) {
        Ok(stream) => return Ok(stream),
        Err(e) if e.to_string().contains("build mismatch") => {
            // A service from another build must be gone before a new one can take the lock.
            stop_and_wait_in(config)?;
        }
        Err(_) => {}
    }
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--core-service")
        .env_remove("CONTROLLER_GUI_PID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(crate::app_log::service_stdio());
    if config.no_hid {
        command.arg("--no-hid");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(stream) = connect(config, false) {
            return Ok(stream);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(
                "Background core did not accept an authenticated connection",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}
/// Writes all of `data`, retrying write timeouts until `patience` has elapsed.
fn write_retry(out: &mut TcpStream, data: &[u8], patience: Duration) -> io::Result<()> {
    let deadline = Instant::now() + patience;
    let mut written = 0;
    while written < data.len() {
        match out.write(&data[written..]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => written += n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) && Instant::now() < deadline => {}
            Err(e) => return Err(e),
        }
    }
    out.flush()
}
pub fn proxy(no_hid: bool) -> io::Result<()> {
    let config = Config::current(no_hid)?;
    let mut input = connect_or_start(&config)?;
    let mut output = input.try_clone()?;
    let outbox = Arc::new(Outbox::new(512));
    let finishing = Arc::new(AtomicBool::new(false));
    let (writer_box, writer_done) = (outbox.clone(), finishing.clone());
    thread::Builder::new()
        .name("core-client-output".into())
        .spawn(move || {
            loop {
                match writer_box.pop_wait(Duration::from_millis(250)) {
                    Some((_, line)) => {
                        // A slow core gets a bounded retry window instead of dropping the link.
                        if write_retry(&mut output, &line, Duration::from_secs(10)).is_err() {
                            break;
                        }
                    }
                    None if writer_done.load(Ordering::Acquire) => break,
                    None => {}
                }
            }
            let _ = output.shutdown(Shutdown::Both);
        })?;
    thread::Builder::new()
        .name("core-client-input".into())
        .spawn(move || {
            let mut lines = Lines::new(io::stdin().lock());
            let mut bytes = Vec::new();
            let mut dropped_at: Option<Instant> = None;
            while let Ok(Line::Data) = lines.next(&mut bytes, MAX_LINE) {
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                if value["action"] == "shutdown" {
                    outbox.clear();
                    break;
                }
                if value["action"] == "shutdownService" {
                    outbox.push_force(b"{\"action\":\"shutdown\"}\n".to_vec());
                    break;
                }
                let mut line = bytes.clone();
                line.push(b'\n');
                if outbox
                    .push(crate::core_io::coalesce_key(&value), line)
                    .is_err()
                {
                    if let Some(id) = value.get("requestId") {
                        let reply = json!({"type":"error","requestId":id,"message":"The background core is busy; the command was not sent."});
                        let mut out = io::stdout().lock();
                        let _ = writeln!(out, "{reply}");
                        let _ = out.flush();
                    }
                    if dropped_at.is_none_or(|t: Instant| t.elapsed() > Duration::from_secs(5)) {
                        crate::app_log::warn("Core client queue is full; dropped a command");
                        dropped_at = Some(Instant::now());
                    }
                }
            }
            finishing.store(true, Ordering::Release);
        })?;
    io::copy(&mut input, &mut io::stdout().lock())?;
    Ok(())
}
#[allow(dead_code)] // non-waiting variant of stop_and_wait
pub fn stop(no_hid: bool) -> io::Result<()> {
    stop_in(&Config::current(no_hid)?)
}
fn stop_in(config: &Config) -> io::Result<()> {
    if !config.endpoint().exists() {
        return Ok(());
    }
    let _ = connect(config, true)?;
    Ok(())
}
pub fn stop_and_wait(no_hid: bool) -> io::Result<()> {
    stop_and_wait_in(&Config::current(no_hid)?)
}
fn stop_and_wait_in(config: &Config) -> io::Result<()> {
    if let Some(legacy) = &config.legacy {
        let old = Config {
            folder: legacy.clone(),
            no_hid: config.no_hid,
            legacy: None,
        };
        // Best effort: an older build may still run from the roaming location.
        let _ = wait_stopped(&old);
    }
    wait_stopped(config)
}
fn wait_stopped(config: &Config) -> io::Result<()> {
    if !config.folder.exists() {
        return Ok(());
    }
    let lock = private_open(&config.folder.join("owner.lock"))?;
    if lock.try_lock().is_ok() {
        if config.endpoint().exists() {
            fs::remove_file(config.endpoint())?;
        }
        return Ok(());
    }
    stop_in(config)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if lock.try_lock().is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::other(
        "Background core is still stopping. Wait before restarting or installing an update.",
    ))
}
struct Client {
    sender: mpsc::SyncSender<Arc<Vec<u8>>>,
    queued: Arc<AtomicUsize>,
    socket: TcpStream,
    closed: Arc<AtomicBool>,
}
/// Per-controller remembered settings in least-recently-used owner order. Within an owner the
/// commands keep the order of their most recent send.
#[derive(Default)]
struct Remembered {
    owners: VecDeque<(String, Vec<(String, Value)>)>,
}
impl Remembered {
    fn touch(&mut self, owner: &str) -> &mut Vec<(String, Value)> {
        let at = match self.owners.iter().position(|(o, _)| o == owner) {
            Some(at) => at,
            None => {
                if self.owners.len() >= MAX_REMEMBERED_OWNERS {
                    self.owners.pop_front();
                }
                self.owners.push_back((owner.to_owned(), Vec::new()));
                self.owners.len() - 1
            }
        };
        let entry = self.owners.remove(at).expect("position is in range");
        self.owners.push_back(entry);
        &mut self.owners.back_mut().expect("just pushed").1
    }
    fn store(&mut self, owner: &str, action: &str, value: &Value, keep: bool) {
        let commands = self.touch(owner);
        commands.retain(|(a, _)| a != action);
        if keep {
            commands.push((action.to_owned(), value.clone()));
        }
    }
    fn replay(&mut self, owner: &str) -> Vec<Value> {
        if !self.owners.iter().any(|(o, _)| o == owner) {
            return Vec::new();
        }
        self.touch(owner).iter().map(|(_, v)| v.clone()).collect()
    }
}
struct Hub {
    clients: Mutex<Vec<Client>>,
    cache: Mutex<std::collections::BTreeMap<String, Value>>,
    commands: mpsc::SyncSender<DeviceCommand>,
    stop: Arc<AtomicBool>,
    wake: OnceLock<SocketAddr>,
    connections: AtomicUsize,
    pending: AtomicUsize,
    gui_pid: AtomicU32,
    owner: Mutex<Option<String>>,
    settings: Mutex<Remembered>,
}
impl Hub {
    fn new(commands: mpsc::SyncSender<DeviceCommand>, stop: Arc<AtomicBool>) -> Self {
        Self {
            clients: Mutex::new(Vec::new()),
            cache: Mutex::new(Default::default()),
            commands,
            stop,
            wake: OnceLock::new(),
            connections: AtomicUsize::new(0),
            pending: AtomicUsize::new(0),
            gui_pid: AtomicU32::new(0),
            owner: Mutex::new(None),
            settings: Mutex::new(Remembered::default()),
        }
    }
    /// Asks the accept loop to finish and wakes it with a throwaway connection.
    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(addr) = self.wake.get() {
            let _ = TcpStream::connect_timeout(addr, Duration::from_millis(200));
        }
    }
    /// Queues a command without blocking forever; a command that cannot be queued within
    /// `patience` is dropped (the caller keeps running). Returns false once the core is gone.
    fn submit(&self, command: DeviceCommand, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        let mut command = command;
        loop {
            match self.commands.try_send(command) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
                Err(mpsc::TrySendError::Full(back)) => {
                    if Instant::now() >= deadline {
                        crate::app_log::warn("Device core queue stayed full; dropped a command");
                        return true;
                    }
                    command = back;
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
    fn bytes(value: &Value) -> io::Result<Arc<Vec<u8>>> {
        let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
        if bytes.len() > MAX_LINE {
            return Err(io::Error::other("Core event exceeds its frame limit"));
        }
        bytes.push(b'\n');
        Ok(Arc::new(bytes))
    }
    fn send(client: &Client, bytes: &Arc<Vec<u8>>) -> bool {
        let before = client.queued.fetch_add(bytes.len(), Ordering::AcqRel);
        if before.saturating_add(bytes.len()) > QUEUE_BYTES
            || client.sender.try_send(bytes.clone()).is_err()
        {
            client.queued.fetch_sub(bytes.len(), Ordering::AcqRel);
            client.closed.store(true, Ordering::Release);
            let _ = client.socket.shutdown(Shutdown::Both);
            return false;
        }
        true
    }
    fn event(&self, value: Value) {
        let kind = value["type"].as_str().unwrap_or("");
        if kind == "device" {
            let owner = value["controllerId"]
                .as_str()
                .filter(|_| value["connected"] == true)
                .map(str::to_owned);
            if let Ok(mut current) = self.owner.lock() {
                *current = owner.clone();
            }
            if self.connections.load(Ordering::Acquire) == 0
                && let Some(owner) = owner
                && let Ok(mut settings) = self.settings.lock()
            {
                for value in settings.replay(&owner) {
                    if let Ok(command) = serde_json::from_value(value) {
                        let _ = self.commands.try_send(command);
                    }
                }
            }
        }
        if kind == "safetyReset"
            && let Ok(mut settings) = self.settings.lock()
        {
            *settings = Remembered::default();
        }
        if matches!(
            kind,
            "device"
                | "devices"
                | "audioMeters"
                | "audioStatus"
                | "speakerStatus"
                | "xinput"
                | "xinputStatus"
                | "pcMode"
                | "mouseStatus"
                | "microphoneStatus"
                | "lightMode"
                | "nativeHandoff"
                | "nativeProviderStatus"
                | "outputCalibration"
                | "outputInputHealth"
                | "gyroAim"
                | "controlProfile"
        ) && let Ok(mut cache) = self.cache.lock()
        {
            cache.insert(kind.into(), value.clone());
        }
        if let Ok(bytes) = Self::bytes(&value)
            && let Ok(mut clients) = self.clients.lock()
        {
            clients.retain(|client| {
                !client.closed.load(Ordering::Acquire) && Self::send(client, &bytes)
            });
        }
    }
    fn remember(&self, value: &Value) {
        let action = value["action"].as_str().unwrap_or("");
        if !REMEMBERED.contains(&action) {
            return;
        }
        if serde_json::to_vec(value).is_ok_and(|b| b.len() > 64 * 1024) {
            return;
        }
        // Scripts and key injection must not silently come back on a headless reconnect unless
        // the user marked them persistent. A newer non-persistent version clears the old one.
        let keep = !RISKY.contains(&action) || value["persistent"] == true;
        if let Ok(owner) = self.owner.lock()
            && let Some(owner) = owner.as_ref()
            && let Ok(mut settings) = self.settings.lock()
        {
            settings.store(owner, action, value, keep);
        }
    }
}
fn serve_client(mut socket: TcpStream, hub: Arc<Hub>, token: Arc<String>) {
    struct Pending(Arc<Hub>, bool);
    impl Drop for Pending {
        fn drop(&mut self) {
            if self.1 {
                self.0.pending.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
    struct Permit(Arc<Hub>);
    impl Drop for Permit {
        fn drop(&mut self) {
            self.0.connections.fetch_sub(1, Ordering::AcqRel);
        }
    }
    // The accept loop already counted this socket as pending.
    let mut pending = Pending(hub.clone(), true);
    let mut permit: Option<Permit> = None;
    let result = (|| -> io::Result<()> {
        socket.set_nodelay(true)?;
        socket.set_read_timeout(Some(Duration::from_millis(250)))?;
        socket.set_write_timeout(Some(Duration::from_millis(500)))?;
        let mut lines = Lines::new(BufReader::new(socket.try_clone()?));
        let mut bytes = Vec::new();
        // One absolute deadline for the whole hello, so a trickling peer cannot hold a slot.
        let deadline = Instant::now() + HANDSHAKE;
        loop {
            match lines.next(&mut bytes, 2048) {
                Ok(Line::Data) => break,
                Ok(_) => return Ok(()),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) && Instant::now() < deadline => {}
                Err(e) => return Err(e),
            }
        }
        let hello: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if hello["protocol"] != 1 || !same_token(hello["token"].as_str().unwrap_or(""), &token) {
            return Ok(());
        }
        let stopping = hello["mode"] == "stop";
        if !stopping && hello["build"] != BUILD {
            socket.write_all(b"{\"ok\":false,\"reason\":\"buildMismatch\"}\n")?;
            return Ok(());
        }
        if !stopping {
            // Only authenticated attachments count against the cap.
            if hub.connections.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                hub.connections.fetch_sub(1, Ordering::AcqRel);
                socket.write_all(b"{\"ok\":false,\"reason\":\"tooManyClients\"}\n")?;
                return Ok(());
            }
            permit = Some(Permit(hub.clone()));
        }
        if std::mem::replace(&mut pending.1, false) {
            hub.pending.fetch_sub(1, Ordering::AcqRel);
        }
        socket.write_all(b"{\"ok\":true}\n")?;
        if stopping {
            hub.submit(DeviceCommand::Shutdown, Duration::from_secs(5));
            hub.request_stop();
            return Ok(());
        }
        if hello["mode"] != "attach" {
            return Ok(());
        }
        let pid = hello["pid"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        hub.gui_pid.store(pid, Ordering::Release);
        device::set_gui_pid(pid);
        let closed = Arc::new(AtomicBool::new(false));
        let queued = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel::<Arc<Vec<u8>>>(32);
        let mut writer = socket.try_clone()?;
        let signal = closed.clone();
        let queue_count = queued.clone();
        let stop = hub.stop.clone();
        thread::Builder::new()
            .name("core-service-client-output".into())
            .spawn(move || {
                while !signal.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
                    match receiver.recv_timeout(Duration::from_millis(250)) {
                        Ok(bytes) => {
                            let result = writer.write_all(&bytes);
                            queue_count.fetch_sub(bytes.len(), Ordering::AcqRel);
                            if result.is_err() {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    }
                }
                signal.store(true, Ordering::Release);
                let _ = writer.shutdown(Shutdown::Both);
            })?;
        let client = Client {
            sender,
            queued,
            socket: socket.try_clone()?,
            closed: closed.clone(),
        };
        if let Ok(cache) = hub.cache.lock() {
            if let Some(device) = cache.get("device") {
                let mut replay = device.clone();
                replay["serviceReplay"] = json!(true);
                let _ = Hub::send(&client, &Hub::bytes(&replay)?);
            }
            for event in cache
                .iter()
                .filter(|(k, _)| k.as_str() != "device")
                .map(|(_, e)| e)
            {
                let _ = Hub::send(&client, &Hub::bytes(event)?);
            }
        }
        let _ = Hub::send(
            &client,
            &Hub::bytes(
                &json!({"type":"core","status":"ready","background":true,"servicePid":std::process::id(),"build":BUILD}),
            )?,
        );
        if let Ok(mut clients) = hub.clients.lock() {
            clients.push(client);
        }
        socket.set_read_timeout(Some(Duration::from_millis(250)))?;
        while !closed.load(Ordering::Acquire) && !hub.stop.load(Ordering::Acquire) {
            match lines.next(&mut bytes, MAX_LINE) {
                Ok(Line::Data) => {
                    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                        continue;
                    };
                    let Ok(command) = serde_json::from_value::<DeviceCommand>(value.clone()) else {
                        continue;
                    };
                    hub.remember(&value);
                    let stopping = matches!(command, DeviceCommand::Shutdown);
                    if !hub.submit(command, Duration::from_secs(if stopping { 5 } else { 2 })) {
                        break;
                    }
                    if stopping {
                        hub.request_stop();
                        break;
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) && bytes.is_empty() =>
                {
                    continue;
                }
                _ => break,
            }
        }
        closed.store(true, Ordering::Release);
        if hub
            .gui_pid
            .compare_exchange(pid, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            device::set_gui_pid(0);
            hub.submit(
                DeviceCommand::SetUiFocused { focused: false },
                Duration::from_millis(200),
            );
        }
        Ok(())
    })();
    let _ = result;
    let _ = socket.shutdown(Shutdown::Both);
    drop(permit);
    drop(pending);
}
/// Removes the published endpoint on every exit path of `serve_with`.
struct EndpointGuard(PathBuf);
impl Drop for EndpointGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub fn serve(no_hid: bool) -> io::Result<()> {
    let config = Config::current(no_hid)?;
    serve_with(config, move |receiver, events| {
        if no_hid {
            hidapi::HidApi::disable_device_discovery();
        }
        device::run(receiver, events, no_hid);
    })
}
fn serve_with<F>(config: Config, actor_body: F) -> io::Result<()>
where
    F: FnOnce(mpsc::Receiver<DeviceCommand>, mpsc::Sender<Value>) + Send + 'static,
{
    fs::create_dir_all(&config.folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&config.folder, fs::Permissions::from_mode(0o700))?;
    }
    let lock = private_open(&config.folder.join("owner.lock"))?;
    lock.try_lock().map_err(io::Error::other)?;
    // Blocking accept: no idle polling. `Hub::request_stop` wakes it with a self-connection.
    let listener = TcpListener::bind(address(0))?;
    let mut entropy = [0; 32];
    getrandom::fill(&mut entropy).map_err(|error| io::Error::other(error.to_string()))?;
    let token = Arc::new(
        entropy
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    let local = listener.local_addr()?;
    let endpoint = Endpoint {
        protocol: 1,
        port: local.port(),
        pid: std::process::id(),
        build: BUILD.into(),
        token: (*token).clone(),
        no_hid: config.no_hid,
    };
    publish_endpoint(&config, &endpoint)?;
    let guard = EndpointGuard(config.endpoint());
    let stop = Arc::new(AtomicBool::new(false));
    let (commands, receiver) = mpsc::sync_channel(256);
    let (events, outputs) = mpsc::channel();
    let hub = Arc::new(Hub::new(commands.clone(), stop.clone()));
    let _ = hub.wake.set(address(local.port()));
    struct Done(Arc<Hub>);
    impl Drop for Done {
        fn drop(&mut self) {
            self.0.request_stop();
        }
    }
    let done = Done(hub.clone());
    let actor = thread::Builder::new()
        .name("background-device-core".into())
        .spawn(move || {
            let _done = done;
            actor_body(receiver, events);
        })?;
    let event_hub = hub.clone();
    if let Err(error) = thread::Builder::new()
        .name("core-service-events".into())
        .spawn(move || {
            // Ends when the device actor drops its event sender.
            for event in outputs {
                event_hub.event(event);
            }
        })
    {
        crate::app_log::error(&format!(
            "Background core could not start its event thread: {error}"
        ));
        hub.request_stop();
    }
    let renew_hub = hub.clone();
    let _ = thread::Builder::new()
        .name("core-service-renew".into())
        .spawn(move || {
            while !renew_hub.stop.load(Ordering::Acquire) {
                thread::sleep(Duration::from_secs(2));
                if renew_hub.connections.load(Ordering::Acquire) == 0
                    && !renew_hub.stop.load(Ordering::Acquire)
                {
                    let _ = renew_hub.commands.try_send(DeviceCommand::RenewTriggers);
                }
            }
        });
    let mut failures = 0u32;
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((socket, _)) => {
                failures = 0;
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let count = hub.pending.fetch_add(1, Ordering::AcqRel);
                if count >= MAX_PENDING {
                    hub.pending.fetch_sub(1, Ordering::AcqRel);
                    let _ = socket.shutdown(Shutdown::Both);
                } else {
                    let client_hub = hub.clone();
                    let token = token.clone();
                    if thread::Builder::new()
                        .name("core-service-client".into())
                        .spawn(move || serve_client(socket, client_hub, token))
                        .is_err()
                    {
                        hub.pending.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            }
            Err(e) => {
                // Transient accept failures (aborted handshakes, descriptor pressure) must not
                // end the service and skip its cleanup.
                failures += 1;
                crate::app_log::warn(&format!("Background core accept failed ({failures}): {e}"));
                if failures >= 100 {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    hub.stop.store(true, Ordering::Release);
    hub.submit(DeviceCommand::Shutdown, Duration::from_secs(5));
    if let Ok(mut clients) = hub.clients.lock() {
        for client in clients.drain(..) {
            client.closed.store(true, Ordering::Release);
            let _ = client.socket.shutdown(Shutdown::Both);
        }
    }
    let _ = actor.join();
    drop(guard);
    drop(lock);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32 as Counter;

    static NEXT: Counter = Counter::new(0);

    fn test_config() -> Config {
        let folder = std::env::temp_dir().join(format!(
            "controller-service-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&folder);
        Config {
            folder,
            no_hid: true,
            legacy: None,
        }
    }

    /// A stand-in device actor: acknowledges commands and leaves on Shutdown.
    fn fake_actor(receiver: mpsc::Receiver<DeviceCommand>, events: mpsc::Sender<Value>) {
        let _ = events.send(json!({"type":"device","connected":true,"controllerId":"pad-1"}));
        while let Ok(command) = receiver.recv() {
            if matches!(command, DeviceCommand::Shutdown) {
                break;
            }
        }
    }

    fn start(config: &Config) -> thread::JoinHandle<io::Result<()>> {
        let served = config.clone();
        let handle = thread::spawn(move || serve_with(served, fake_actor));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if read_endpoint(config).is_ok() {
                return handle;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("service did not publish an endpoint");
    }

    #[test]
    fn token_comparison_rejects_prefixes_and_changed_bytes() {
        assert!(same_token("abcdef", "abcdef"));
        assert!(!same_token("abcdef", "abcdeg"));
        assert!(!same_token("abcdef", "abc"));
        assert!(!same_token("abcdef", ""));
    }

    #[test]
    fn handshake_build_mismatch_and_stop_and_wait() {
        let config = test_config();
        let handle = start(&config);
        // Matching build attaches and receives the ready event.
        let mut stream = connect(&config, false).expect("attach");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut seen = String::new();
        let mut byte = [0u8];
        while !seen.contains("\"ready\"") {
            stream.read_exact(&mut byte).unwrap();
            seen.push(byte[0] as char);
        }
        drop(stream);
        // Another build is refused with a specific reason.
        let error = connect_with(&config, false, "not-this-build").unwrap_err();
        assert!(error.to_string().contains("build mismatch"));
        // A wrong token is refused without an acknowledgement.
        let endpoint = read_endpoint(&config).unwrap();
        let mut bad = TcpStream::connect(address(endpoint.port)).unwrap();
        bad.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        bad.write_all(
            format!(
                "{{\"protocol\":1,\"token\":\"{}\",\"build\":\"{BUILD}\",\"mode\":\"attach\",\"pid\":1}}\n",
                "0".repeat(64)
            )
            .as_bytes(),
        )
        .unwrap();
        let mut reply = Vec::new();
        let _ = bad.read_to_end(&mut reply);
        assert!(reply.is_empty());
        // stop_and_wait returns only after the service released its lock and endpoint.
        stop_and_wait_in(&config).expect("stop");
        handle.join().unwrap().unwrap();
        assert!(!config.endpoint().exists());
        let lock = private_open(&config.folder.join("owner.lock")).unwrap();
        assert!(lock.try_lock().is_ok());
        drop(lock);
        let _ = fs::remove_dir_all(&config.folder);
    }

    #[test]
    fn silent_unauthenticated_sockets_do_not_lock_out_the_gui() {
        let config = test_config();
        let handle = start(&config);
        let endpoint = read_endpoint(&config).unwrap();
        let idle: Vec<TcpStream> = (0..MAX_CLIENTS + 2)
            .map(|_| TcpStream::connect(address(endpoint.port)).unwrap())
            .collect();
        let started = Instant::now();
        let stream = connect(&config, false).expect("attach despite idle sockets");
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(stream);
        // Idle peers are dropped after the absolute handshake deadline.
        let mut idle = idle;
        idle[0]
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut sink = Vec::new();
        let began = Instant::now();
        let _ = idle[0].read_to_end(&mut sink);
        assert!(began.elapsed() < Duration::from_secs(4));
        stop_and_wait_in(&config).expect("stop");
        handle.join().unwrap().unwrap();
        let _ = fs::remove_dir_all(&config.folder);
    }

    #[test]
    fn stop_and_wait_clears_a_stale_endpoint() {
        let config = test_config();
        fs::create_dir_all(&config.folder).unwrap();
        fs::write(config.endpoint(), b"stale").unwrap();
        stop_and_wait_in(&config).unwrap();
        assert!(!config.endpoint().exists());
        let _ = fs::remove_dir_all(&config.folder);
    }

    fn hub() -> (Hub, mpsc::Receiver<DeviceCommand>) {
        let (tx, rx) = mpsc::sync_channel(64);
        (Hub::new(tx, Arc::new(AtomicBool::new(false))), rx)
    }

    #[test]
    fn replay_keeps_send_order_and_skips_scripts_unless_persistent() {
        let (hub, rx) = hub();
        hub.event(json!({"type":"device","connected":true,"controllerId":"pad-1"}));
        hub.remember(&json!({"action":"setAudioMixer","settings":{}}));
        hub.remember(&json!({"action":"setScript","source":"x"}));
        hub.remember(&json!({"action":"setPcBindings","bindings":[]}));
        hub.remember(&json!({"action":"setGameActions","actions":[]}));
        hub.remember(&json!({"action":"setLightbar","rgb":[1,2,3]})); // not remembered
        hub.remember(&json!({"action":"applyTriggers","left":null,"right":null}));
        hub.remember(&json!({"action":"setAudioMixer","settings":{"again":1}}));
        let replay = hub.settings.lock().unwrap().replay("pad-1");
        let actions: Vec<_> = replay
            .iter()
            .map(|v| v["action"].as_str().unwrap())
            .collect();
        assert_eq!(actions, ["applyTriggers", "setAudioMixer"]);
        // A persistent script is kept and a later non-persistent one clears it.
        hub.remember(&json!({"action":"setScript","source":"y","persistent":true}));
        assert!(
            hub.settings
                .lock()
                .unwrap()
                .replay("pad-1")
                .iter()
                .any(|v| v["action"] == "setScript")
        );
        hub.remember(&json!({"action":"setScript","source":""}));
        assert!(
            !hub.settings
                .lock()
                .unwrap()
                .replay("pad-1")
                .iter()
                .any(|v| v["action"] == "setScript")
        );
        // Reconnecting with no GUI attached replays to the actor queue.
        hub.remember(&json!({"action":"setTouchMouse","enabled":true}));
        hub.event(json!({"type":"device","connected":true,"controllerId":"pad-1"}));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn least_recently_used_owner_is_evicted() {
        let mut remembered = Remembered::default();
        let value = json!({"action":"setAudioMixer"});
        for n in 0..MAX_REMEMBERED_OWNERS {
            remembered.store(&format!("pad-{n}"), "setAudioMixer", &value, true);
        }
        // Touch pad-0 so pad-1 is now the least recently used.
        remembered.replay("pad-0");
        remembered.store("new", "setAudioMixer", &value, true);
        assert!(!remembered.replay("pad-1").iter().any(|_| true));
        assert!(!remembered.replay("pad-0").is_empty());
        assert!(!remembered.replay("new").is_empty());
    }

    #[test]
    fn client_queue_overflow_closes_the_slow_client() {
        let listener = TcpListener::bind(address(0)).unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (sender, _receiver) = mpsc::sync_channel(1);
        let client = Client {
            sender,
            queued: Arc::new(AtomicUsize::new(0)),
            socket,
            closed: Arc::new(AtomicBool::new(false)),
        };
        let bytes = Arc::new(b"{}\n".to_vec());
        assert!(Hub::send(&client, &bytes));
        // The one-slot channel is now full.
        assert!(!Hub::send(&client, &bytes));
        assert!(client.closed.load(Ordering::Acquire));
        // Byte budget overflow also closes.
        let big = Arc::new(vec![0u8; QUEUE_BYTES + 1]);
        let (sender, _receiver) = mpsc::sync_channel(4);
        let client = Client {
            sender,
            queued: Arc::new(AtomicUsize::new(0)),
            socket: TcpStream::connect(listener.local_addr().unwrap()).unwrap(),
            closed: Arc::new(AtomicBool::new(false)),
        };
        assert!(!Hub::send(&client, &big));
        assert_eq!(client.queued.load(Ordering::Acquire), 0);
    }

    #[test]
    fn submit_drops_instead_of_blocking_when_the_actor_is_stuck() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let hub = Hub::new(tx, Arc::new(AtomicBool::new(false)));
        assert!(hub.submit(DeviceCommand::Scan, Duration::from_millis(20)));
        let started = Instant::now();
        assert!(hub.submit(DeviceCommand::Scan, Duration::from_millis(30)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
