//! Read Steam's local catalog. No game process or memory access is needed.
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    app_id: String,
    name: String,
    install_dir: String,
    executables: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Vdf {
    Text(String),
    Object(BTreeMap<String, Vdf>),
}
impl Vdf {
    fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }
}

fn parse(source: &str) -> Result<Vdf, &'static str> {
    if source.len() > 2_000_000 {
        return Err("Steam manifest is too large");
    }
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for ch in chars.by_ref() {
                if ch == '\n' {
                    break;
                }
            }
            continue;
        }
        if matches!(c, '{' | '}') {
            tokens.push(c.to_string());
        } else if c == '"' {
            let mut value = String::new();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == '"' {
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    let escaped = chars.next().ok_or("Incomplete Steam escape")?;
                    if !matches!(escaped, '\\' | '"') {
                        value.push('\\');
                    }
                    value.push(escaped);
                } else {
                    value.push(ch);
                }
            }
            if !closed {
                return Err("Unclosed Steam string");
            }
            // Prefix strings to distinguish a quoted brace from syntax.
            tokens.push(format!("s{value}"));
        } else {
            return Err("Unsupported Steam manifest token");
        }
        if tokens.len() > 100_000 {
            return Err("Too many Steam manifest entries");
        }
    }
    fn object(tokens: &[String], at: &mut usize, depth: u8) -> Result<Vdf, &'static str> {
        if depth > 12 {
            return Err("Steam manifest nesting is too deep");
        }
        let mut map = BTreeMap::new();
        while let Some(key) = tokens.get(*at) {
            if key == "}" {
                if depth == 0 {
                    return Err("Unexpected closing brace");
                }
                *at += 1;
                return Ok(Vdf::Object(map));
            }
            let key = key
                .strip_prefix('s')
                .ok_or("Missing Steam manifest key")?
                .to_string();
            *at += 1;
            let value = match tokens
                .get(*at)
                .ok_or("Missing Steam manifest value")?
                .as_str()
            {
                "{" => {
                    *at += 1;
                    object(tokens, at, depth + 1)?
                }
                s => {
                    let value = s.strip_prefix('s').ok_or("Invalid Steam manifest value")?;
                    *at += 1;
                    Vdf::Text(value.to_string())
                }
            };
            map.insert(key, value);
        }
        if depth > 0 {
            return Err("Unclosed Steam object");
        }
        Ok(Vdf::Object(map))
    }
    object(&tokens, &mut 0, 0)
}

fn read(path: &Path) -> Option<Vdf> {
    if fs::metadata(path).ok()?.len() > 2_000_000 {
        return None;
    }
    parse(&fs::read_to_string(path).ok()?).ok()
}

fn executables(folder: &Path, depth: u8, budget: &mut usize, found: &mut Vec<String>) {
    if depth > 3 || *budget == 0 || found.len() >= 64 {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 || found.len() >= 64 {
            break;
        }
        *budget -= 1;
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            executables(&entry.path(), depth + 1, budget, found);
        } else if kind.is_file() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let is_exe = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
            #[cfg(unix)]
            let is_exe = is_exe || {
                use std::os::unix::fs::PermissionsExt;
                entry
                    .metadata()
                    .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            };
            if is_exe
                && !["unins", "setup", "crash", "redist", "vcredist"]
                    .iter()
                    .any(|n| name.contains(n))
            {
                found.push(path.to_string_lossy().into_owned());
            }
        }
    }
}

fn scan(roots: Vec<PathBuf>) -> Vec<Game> {
    let mut libraries = roots.clone();
    for root in roots {
        if let Some(Vdf::Object(folders)) = read(&root.join("steamapps/libraryfolders.vdf"))
            .and_then(|v| v.get("libraryfolders").cloned())
        {
            for value in folders.values() {
                if let Some(path) = value
                    .get("path")
                    .and_then(Vdf::text)
                    .or_else(|| value.text())
                {
                    libraries.push(PathBuf::from(path));
                }
            }
        }
    }
    libraries.sort();
    libraries.dedup();
    let mut games = BTreeMap::new();
    for library in libraries.into_iter().take(32) {
        let Ok(entries) = fs::read_dir(library.join("steamapps")) else {
            continue;
        };
        for entry in entries.take(4096).flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                continue;
            }
            let Some(root) = read(&entry.path()) else {
                continue;
            };
            let Some(app) = root.get("AppState") else {
                continue;
            };
            let (Some(id), Some(name), Some(dir)) = (
                app.get("appid").and_then(Vdf::text),
                app.get("name").and_then(Vdf::text),
                app.get("installdir").and_then(Vdf::text),
            ) else {
                continue;
            };
            if id.is_empty()
                || !id.bytes().all(|b| b.is_ascii_digit())
                || name.len() > 200
                || dir.is_empty()
                || Path::new(dir)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                continue;
            }
            let folder = library.join("steamapps/common").join(dir);
            if !folder.is_dir() {
                continue;
            }
            let mut candidates = Vec::new();
            executables(&folder, 0, &mut 4096, &mut candidates);
            candidates.sort();
            games.insert(
                id.to_string(),
                Game {
                    app_id: id.to_string(),
                    name: name.to_string(),
                    install_dir: folder.to_string_lossy().into_owned(),
                    executables: candidates,
                },
            );
        }
    }
    let mut games: Vec<_> = games.into_values().collect();
    games.sort_by_key(|g| g.name.to_lowercase());
    games
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("STEAM_PATH") {
        roots.push(PathBuf::from(path));
    }
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let home = PathBuf::from(home);
        roots.extend([
            home.join(".steam/steam"),
            home.join(".local/share/Steam"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            home.join("Library/Application Support/Steam"),
        ]);
    }
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(path) = std::env::var_os(key) {
            roots.push(PathBuf::from(path).join("Steam"));
        }
    }
    roots
}

pub fn installed() -> Vec<Game> {
    scan(steam_roots())
}

/// Steam Input state relevant to a DualSense. Only read-only file/process inspection is used.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InputStatus {
    /// A Steam install folder with a config.vdf was found.
    pub installed: bool,
    /// Steam client process is running; `None` when this platform cannot tell.
    pub running: Option<bool>,
    /// Value of Steam's global "PlayStation Configuration Support"; `None` when unset or unknown.
    pub playstation_support: Option<bool>,
    /// True when Steam is running with PlayStation support on, so it can grab the controller.
    pub may_conflict: bool,
    pub note: String,
}

fn find_key<'a>(vdf: &'a Vdf, key: &str, depth: u8) -> Option<&'a str> {
    if depth > 8 {
        return None;
    }
    let Vdf::Object(map) = vdf else {
        return None;
    };
    for (name, value) in map {
        if name.eq_ignore_ascii_case(key)
            && let Some(text) = value.text()
        {
            return Some(text);
        }
    }
    map.values().find_map(|v| find_key(v, key, depth + 1))
}

fn playstation_support_in(config: &Vdf) -> Option<bool> {
    match find_key(config, "SteamController_PSSupport", 0)?.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn steam_running() -> Option<bool> {
    #[cfg(windows)]
    {
        use std::mem::size_of;
        use windows_sys::Win32::{
            Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
            System::Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
        };
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while has_entry && !found {
            let end = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            found =
                String::from_utf16_lossy(&entry.szExeFile[..end]).eq_ignore_ascii_case("steam.exe");
            has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        Some(found)
    }
    #[cfg(target_os = "linux")]
    {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let pid = fs::read_to_string(home.join(".steam/steam.pid")).ok()?;
        let pid: u32 = pid.trim().parse().ok()?;
        Some(Path::new(&format!("/proc/{pid}")).exists())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}

pub fn input_status() -> InputStatus {
    let config = steam_roots()
        .into_iter()
        .map(|root| root.join("config").join("config.vdf"))
        .find_map(|path| read(&path));
    let running = steam_running();
    let playstation_support = config.as_ref().and_then(playstation_support_in);
    let may_conflict = running == Some(true) && playstation_support == Some(true);
    let note = match (config.is_some(), running, playstation_support) {
        (false, _, _) => "Steam was not found.".into(),
        (_, _, Some(true)) if running == Some(true) => "Steam is running with PlayStation Configuration Support on. Steam Input can take over the controller and compete with CONTROLLER; turn the option off in Steam > Settings > Controller, or use per-game Steam Input off.".into(),
        (_, _, Some(true)) => "Steam has PlayStation Configuration Support on. It only matters while Steam is running.".into(),
        (_, _, Some(false)) => "Steam Input PlayStation support is off.".into(),
        _ => "Steam's PlayStation Configuration Support is not set in config.vdf (per-game or per-controller overrides are not inspected).".into(),
    };
    InputStatus {
        installed: config.is_some(),
        running,
        playstation_support,
        may_conflict,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_playstation_support_flag_case_insensitively() {
        let on = parse("\"InstallConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"steamcontroller_pssupport\" \"1\" } } } }").unwrap();
        assert_eq!(playstation_support_in(&on), Some(true));
        let off = parse("\"InstallConfigStore\" { \"SteamController_PSSupport\" \"0\" }").unwrap();
        assert_eq!(playstation_support_in(&off), Some(false));
        let unset = parse("\"InstallConfigStore\" { \"Other\" \"1\" }").unwrap();
        assert_eq!(playstation_support_in(&unset), None);
    }
    #[test]
    fn parses_steam_escapes_and_rejects_truncated_documents() {
        let doc = parse(
            r#"// comment
"libraryfolders" { "0" { "path" "C:\\Steam" } }"#,
        )
        .unwrap();
        assert_eq!(
            doc.get("libraryfolders")
                .unwrap()
                .get("0")
                .unwrap()
                .get("path")
                .unwrap()
                .text(),
            Some("C:\\Steam")
        );
        assert!(parse("\"x\" { \"name\" \"unfinished").is_err());
        assert!(parse("}").is_err());
    }
    #[test]
    fn finds_only_installed_manifests_and_skips_traversal() {
        let root = std::env::temp_dir().join(format!("controller-steam-{}", std::process::id()));
        let apps = root.join("steamapps");
        fs::create_dir_all(apps.join("common/Test/Binaries")).unwrap();
        fs::write(apps.join("common/Test/Binaries/Game.exe"), b"test").unwrap();
        fs::write(
            apps.join("appmanifest_1.acf"),
            r#""AppState" { "appid" "1" "name" "Test game" "installdir" "Test" }"#,
        )
        .unwrap();
        fs::write(
            apps.join("appmanifest_2.acf"),
            r#""AppState" { "appid" "2" "name" "Invalid" "installdir" "../elsewhere" }"#,
        )
        .unwrap();
        let games = scan(vec![root]);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Test game");
        assert_eq!(games[0].executables.len(), 1);
        fs::remove_file(apps.join("appmanifest_1.acf")).unwrap();
        fs::remove_file(apps.join("appmanifest_2.acf")).unwrap();
        fs::remove_file(apps.join("common/Test/Binaries/Game.exe")).unwrap();
    }
}
