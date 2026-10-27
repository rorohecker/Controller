use serde_json::{Value, json};

pub fn inspect() -> Value {
    let mut checks = vec![
        json!({"name":"Platform", "detail":format!("{} / {}",std::env::consts::OS,std::env::consts::ARCH),"ok":true}),
    ];
    #[cfg(target_os = "linux")]
    {
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
        checks.push(json!({"name":"Desktop session","detail":format!("{session}. Wayland may require desktop-control approval; X11 uses the current display."),"ok":true}));
        let mut controllers = 0;
        let mut accessible = 0;
        if let Ok(entries) = std::fs::read_dir("/sys/class/hidraw") {
            for entry in entries.flatten() {
                if std::fs::read_to_string(entry.path().join("device/uevent"))
                    .is_ok_and(|s| s.contains("0000054C"))
                {
                    controllers += 1;
                    let path = std::path::Path::new("/dev").join(entry.file_name());
                    if std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(path)
                        .is_ok()
                    {
                        accessible += 1;
                    }
                }
            }
        }
        checks.push(json!({"name":"Controller permissions","detail":if controllers==0 {"No Sony hidraw device found.".to_string()} else {format!("{accessible} of {controllers} Sony devices allow read/write access. Install the bundled udev rule if access is denied, then reconnect.")},"ok":controllers==0||accessible==controllers}));
        let rule = [
            "/etc/udev/rules.d/70-controller-dualsense.rules",
            "/usr/lib/udev/rules.d/70-controller-dualsense.rules",
            "/etc/udev/rules.d/70-dualsense.rules",
        ]
        .iter()
        .any(|p| std::path::Path::new(p).exists());
        checks.push(json!({"name":"udev rule","detail":if rule {"A CONTROLLER rule is installed."} else {"No app-specific rule found. Existing system rules may already provide access."},"ok":rule||accessible==controllers}));
    }
    #[cfg(target_os="macos")]
    checks.push(json!({"name":"Desktop permissions","detail":"Allow CONTROLLER in System Settings → Privacy & Security → Accessibility for PC controls and gyro aim. Errors appear here when an input is blocked.","ok":true}));
    #[cfg(windows)]
    checks.push(json!({"name":"Desktop input","detail":"PC controls target normal desktop windows. Windows blocks input sent to apps with higher privileges.","ok":true}));
    checks.push(json!({"name":"Game receiver","detail":"JSON and DSX adapters listen on this PC only. Weapon detection requires a game or mod that sends supported events.","ok":true}));
    json!({"checks":checks})
}
