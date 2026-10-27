#[derive(Clone, Default)]
pub struct ProcessState {
    pub conflicts: Vec<&'static str>,
    pub foreground_exe: Option<String>,
    pub foreground_title: Option<String>,
    pub foreground_path: Option<String>,
}

pub fn inspect() -> ProcessState {
    // Keep external process inspection away from the controller read loop.
    static STATUS: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<ProcessState>>> =
        std::sync::OnceLock::new();
    let status = STATUS.get_or_init(|| {
        let status = std::sync::Arc::new(std::sync::Mutex::new(ProcessState::default()));
        let worker = status.clone();
        std::thread::spawn(move || {
            loop {
                let next = inspect_now();
                if let Ok(mut current) = worker.lock() {
                    *current = next;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
        status
    });
    status
        .lock()
        .map(|current| current.clone())
        .unwrap_or_default()
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
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
