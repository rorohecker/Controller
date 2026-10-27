pub struct ProcessState {
    pub conflicts: Vec<&'static str>,
    pub foreground_exe: Option<String>,
    pub foreground_title: Option<String>,
    pub foreground_path: Option<String>,
}

pub fn inspect() -> ProcessState {
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
