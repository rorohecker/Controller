//! Size-capped rotating log files plus a `log` facade backend.
//!
//! The desktop shell writes `controller.log`; the optional background core process writes its
//! stderr to `core-service.log`. Both live in the per-user local data folder and are rotated
//! (`.1` ... `.3`) when they pass one mebibyte.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 1024 * 1024;
const KEEP: usize = 3;
const GUI_LOG: &str = "controller.log";
const SERVICE_LOG: &str = "core-service.log";

struct Sink {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max: u64,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

/// Per-user, machine-local state (not roamed with the profile).
pub fn local_base() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(Ok)
            .unwrap_or_else(crate::profiles::config_base)
    }
    #[cfg(target_os = "macos")]
    {
        crate::profiles::config_base()
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(state) = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            return Ok(state);
        }
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".local/state"))
            .ok_or_else(|| "HOME is unavailable".to_string())
    }
}

pub fn log_dir() -> Option<PathBuf> {
    Some(local_base().ok()?.join("CONTROLLER").join("logs"))
}

fn numbered(path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{index}", path.display()))
}

/// Shifts `x.log` to `x.log.1`, keeping `KEEP` older files. Failures are ignored: a log that
/// another process still has open on Windows simply keeps growing until the next attempt.
fn rotate(path: &Path) {
    for index in (1..KEEP).rev() {
        let from = numbered(path, index);
        if from.exists() {
            let _ = fs::rename(&from, numbered(path, index + 1));
        }
    }
    let _ = fs::rename(path, numbered(path, 1));
}

fn open_append(path: &Path, max: u64) -> Option<(File, u64)> {
    if fs::metadata(path).is_ok_and(|m| m.len() >= max) {
        rotate(path);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    Some((file, size))
}

impl Sink {
    fn new(path: PathBuf, max: u64) -> Self {
        let (file, size) = match open_append(&path, max) {
            Some((file, size)) => (Some(file), size),
            None => (None, 0),
        };
        Self {
            path,
            file,
            size,
            max,
        }
    }
    fn write(&mut self, line: &str) {
        if self.size >= self.max {
            self.file = None;
            rotate(&self.path);
            self.size = 0;
        }
        if self.file.is_none() {
            self.file = open_append(&self.path, u64::MAX).map(|(file, size)| {
                self.size = size;
                file
            });
        }
        if let Some(file) = self.file.as_mut() {
            if file.write_all(line.as_bytes()).is_ok() {
                self.size += line.len() as u64;
            } else {
                self.file = None;
            }
        }
    }
}

fn format_time(secs: i64, millis: u32) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_time(now.as_secs() as i64, now.subsec_millis())
}

fn emit(level: &str, source: &str, message: &str) {
    let mut line = format!("{} {level:<5} [{source}] ", timestamp());
    // One record per line, whatever the message contains.
    line.push_str(&message.replace(['\r', '\n'], " "));
    line.push('\n');
    match SINK.get().and_then(|sink| sink.lock().ok()) {
        Some(mut sink) => sink.write(&line),
        None => {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
    }
}

pub fn info(message: &str) {
    emit("INFO", "app", message);
}
pub fn warn(message: &str) {
    emit("WARN", "app", message);
}
pub fn error(message: &str) {
    emit("ERROR", "app", message);
}
/// A line the background/child core wrote to its stderr.
pub fn core_line(message: &str) {
    emit("INFO", "core", clip(message, MAX_LINE_CHARS));
}

const MAX_LINE_CHARS: usize = 2000;

/// `text` cut to at most `max` bytes on a character boundary.
fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.level() <= log::Level::Info && metadata.target().starts_with("controller"))
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            emit(
                &record.level().to_string(),
                record.target(),
                &record.args().to_string(),
            );
        }
    }
    fn flush(&self) {}
}
static LOGGER: Logger = Logger;

fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// Desktop shell: log to `controller.log`, route `log` records and panics there.
pub fn init_gui() {
    if let Some(dir) = log_dir()
        && fs::create_dir_all(&dir).is_ok()
    {
        let _ = SINK.set(Mutex::new(Sink::new(dir.join(GUI_LOG), MAX_BYTES)));
    }
    install_logger();
    std::panic::set_hook(Box::new(|info| {
        error(&format!("panic: {info}"));
    }));
    info(&format!(
        "CONTROLLER {} starting ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
}

/// Core / service / client processes: `log` records go to stderr (which the parent captures).
pub fn init_stderr() {
    install_logger();
}

/// The long-running `--core-service` process: it owns a rotating `core-service.log` sink itself
/// (a handle inherited from the parent could never be rotated on Windows).
pub fn init_service() {
    if let Some(dir) = log_dir()
        && fs::create_dir_all(&dir).is_ok()
    {
        let _ = SINK.set(Mutex::new(Sink::new(dir.join(SERVICE_LOG), MAX_BYTES)));
    }
    install_logger();
    std::panic::set_hook(Box::new(|info| {
        error(&format!("panic: {info}"));
    }));
}

/// Where the background core's stderr should go: nowhere, the service logs to its own file.
pub fn service_stdio() -> Stdio {
    Stdio::null()
}

fn tail(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = length.saturating_sub(256 * 1024);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err()
        || file.take(256 * 1024).read_to_end(&mut bytes).is_err()
    {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // probably cut mid-line
    }
    let skip = lines.len().saturating_sub(max_lines);
    lines.split_off(skip)
}

/// Recent lines of the shell and background-core logs, oldest first.
pub fn recent_lines(max_lines: usize) -> Vec<String> {
    let Some(dir) = log_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in [GUI_LOG, SERVICE_LOG] {
        let lines = tail(&dir.join(name), max_lines);
        if !lines.is_empty() {
            out.push(format!("== {name} =="));
            out.extend(lines);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "controller-log-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(format_time(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_time(1_700_000_000, 42), "2023-11-14T22:13:20.042Z");
        assert_eq!(format_time(1_709_164_800, 0), "2024-02-29T00:00:00.000Z");
    }

    #[test]
    fn rotation_is_capped_and_keeps_numbered_history() {
        let dir = temp("rotate");
        let path = dir.join("x.log");
        let mut sink = Sink::new(path.clone(), 64);
        for i in 0..40 {
            sink.write(&format!("line number {i} with padding padding\n"));
        }
        assert!(path.exists());
        assert!(numbered(&path, 1).exists());
        assert!(numbered(&path, 2).exists());
        assert!(!numbered(&path, KEEP + 1).exists());
        assert!(fs::metadata(&path).unwrap().len() <= 64 + 64);
        assert_eq!(tail(&path, 1).len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn clip_respects_char_boundaries() {
        assert_eq!(clip("abc", 10), "abc");
        assert_eq!(clip("abcdef", 3), "abc");
        assert_eq!(clip("a\u{e9}b", 2), "a");
        assert_eq!(clip("\u{e9}", 0), "");
    }

    #[test]
    fn tail_returns_only_the_last_lines() {
        let dir = temp("tail");
        let path = dir.join("t.log");
        fs::write(&path, "a\nb\nc\nd\n").unwrap();
        assert_eq!(tail(&path, 2), vec!["c".to_string(), "d".to_string()]);
        assert!(tail(&dir.join("missing.log"), 5).is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
