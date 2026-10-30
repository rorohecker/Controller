#[path = "process_cache.rs"]
mod cache;
pub fn inspect() -> ProcessState {
    cache::inspect()
}
pub fn diagnostics() -> serde_json::Value {
    cache::diagnostics()
}
#[derive(Clone, Default)]
pub struct ProcessState {
    pub conflicts: Vec<&'static str>,
    pub foreground_exe: Option<String>,
    pub foreground_title: Option<String>,
    pub foreground_path: Option<String>,
}

fn inspect_now() -> ProcessState {
    let mut state = ProcessState {
        conflicts: Vec::new(),
        foreground_exe: None,
        foreground_title: None,
        foreground_path: None,
    };
    #[cfg(target_os = "macos")]
    if let Some(name) = output(
        "osascript",
        &[
            "-e",
            "tell application \"System Events\" to get name of first application process whose frontmost is true",
        ],
    ) && !name.is_empty()
        && !name.eq_ignore_ascii_case("controller")
    {
        state.foreground_title = Some(name.clone());
        state.foreground_exe = Some(name);
    }
    #[cfg(target_os = "linux")]
    if let Some(window) = output("xprop", &["-root", "_NET_ACTIVE_WINDOW"])
        .and_then(|value| value.split_whitespace().last().map(str::to_owned))
        .filter(|id| id.starts_with("0x") && id[2..].chars().all(|c| c.is_ascii_hexdigit()))
        && let Some(properties) = output("xprop", &["-id", &window, "_NET_WM_PID", "_NET_WM_NAME"])
    {
        let pid = properties
            .lines()
            .find(|line| line.starts_with("_NET_WM_PID"))
            .and_then(|line| line.split('=').nth(1))
            .and_then(|pid| pid.trim().parse::<u32>().ok());
        if let Some(pid) = pid
            && let Ok(path) = std::fs::read_link(format!("/proc/{pid}/exe"))
        {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            if name
                .as_deref()
                .is_some_and(|n| !n.eq_ignore_ascii_case("controller"))
            {
                state.foreground_exe = name;
                state.foreground_path = Some(path.to_string_lossy().into_owned());
                if let Ok(command_line) = std::fs::read(format!("/proc/{pid}/cmdline"))
                    && let Some((name, path)) = proton_executable(&command_line)
                {
                    state.foreground_exe = Some(name);
                    state.foreground_path = Some(path);
                }
                state.foreground_title = properties
                    .lines()
                    .find(|line| line.starts_with("_NET_WM_NAME"))
                    .and_then(|line| line.split_once('='))
                    .map(|(_, title)| title.trim().trim_matches('"').to_owned());
            }
        }
    }
    state
}

#[cfg(target_os = "linux")]
fn proton_executable(command_line: &[u8]) -> Option<(String, String)> {
    if command_line.len() > 65536 {
        return None;
    }
    command_line
        .split(|b| *b == 0)
        .filter_map(|s| std::str::from_utf8(s).ok())
        .find_map(|arg| {
            let path = arg.trim_matches('"').replace('\\', "/");
            let name = path.rsplit('/').next()?;
            if !name.to_lowercase().ends_with(".exe")
                || name.len() > 80
                || [
                    "wine",
                    "services.exe",
                    "explorer.exe",
                    "winedevice.exe",
                    "rundll32.exe",
                ]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n))
                || path.starts_with('-')
            {
                return None;
            }
            let name = name.to_owned();
            let path = if path.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("z:/")) {
                path[2..].to_owned()
            } else {
                path
            };
            Some((name, path))
        })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::proton_executable;
    #[test]
    fn identifies_game_in_proton_command_line_without_guessing_helper_names() {
        assert_eq!(
            proton_executable(b"wine64\0Z:\\games\\example\\game.exe\0--launch\0"),
            Some(("game.exe".into(), "/games/example/game.exe".into()))
        );
        assert!(proton_executable(b"wine64\0C:\\windows\\services.exe\0").is_none());
        assert!(proton_executable(b"wine64\0--game.exe\0").is_none());
    }
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < Duration::from_millis(600) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    if !success {
        return None;
    }
    let mut text = String::new();
    child
        .stdout
        .take()?
        .take(65_537)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() <= 65_536).then(|| text.trim().to_owned())
}
