#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_haptics;
mod desktop_access;
mod device;
mod diagnostics;
mod game_actions;
mod game_link;
mod haptics;
mod hardware;
mod hid_trace;
mod input_map;
mod integrations;
mod macros;
#[cfg(not(windows))]
mod native_input;
mod output_calibration;
#[cfg(windows)]
mod processes;
#[cfg(not(windows))]
#[path = "processes_portable.rs"]
mod processes;
mod profiles;
mod recognition;
mod runtime_diagnostics;
#[cfg(windows)]
mod screen_color;
#[cfg(not(windows))]
#[path = "screen_color_portable.rs"]
mod screen_color;
mod script;
mod steam;
mod telemetry;
mod updates;
mod xinput_bridge;

use std::{
    io::{self, BufRead, BufReader, BufWriter, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tauri::{Emitter, Manager, State};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    close_to_tray: bool,
    start_on_login: bool,
}

struct TraySettings {
    close_to_tray: AtomicBool,
}

fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("settings.json"))
}

fn read_close_to_tray(app: &tauri::AppHandle) -> bool {
    settings_path(app)
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|json| json.get("closeToTray")?.as_bool())
        .unwrap_or(true)
}

#[tauri::command]
fn get_app_settings(app: tauri::AppHandle, tray: State<'_, TraySettings>) -> AppSettings {
    AppSettings {
        close_to_tray: tray.close_to_tray.load(Ordering::SeqCst),
        start_on_login: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[tauri::command]
fn set_close_to_tray(
    enabled: bool,
    app: tauri::AppHandle,
    tray: State<'_, TraySettings>,
) -> Result<(), String> {
    let path = settings_path(&app)?;
    std::fs::create_dir_all(path.parent().ok_or("Settings folder missing")?)
        .map_err(|e| e.to_string())?;
    profiles::write_archive(
        &path,
        &serde_json::to_vec(&json!({"closeToTray": enabled})).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    tray.close_to_tray.store(enabled, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn set_start_on_login(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn platform_capabilities() -> Value {
    json!({"os": std::env::consts::OS, "desktopInput":integrations::environment()["desktopCandidate"], "nativeAudioFiles":true,"processAudioCapture":cfg!(windows)&&integrations::windows_build()>=20348, "supportedKeys":(1..=254u16).filter(|k|input_map::supported_key(*k)).collect::<Vec<_>>(), "xinput": cfg!(windows), "screenColor": cfg!(windows), "systemAudioBridge": cfg!(windows), "updates": cfg!(windows)})
}

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;

struct Bridge {
    writer: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    handle: tauri::AppHandle,
    stopping: AtomicBool,
}

impl Bridge {
    fn spawn_child(app: &tauri::AppHandle) -> io::Result<(Child, ChildStdin)> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--core")
            .env("CONTROLLER_GUI_PID", std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = command.spawn()?;
        let writer = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Core input pipe missing"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Core output pipe missing"))?;
        let errors = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Core error pipe missing"))?;
        let event_handle = app.clone();
        thread::Builder::new()
            .name("controller-core-events".into())
            .spawn(move || {
                for line in BufReader::new(output).lines() {
                    match line {
                        Ok(line) => {
                            if let Ok(event) = serde_json::from_str::<Value>(&line) {
                                let _ = event_handle.emit("core-event", event);
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = event_handle.emit(
                    "core-event",
                    json!({"type":"device","connected":false,"message":"Core stopped"}),
                );
                let _ = event_handle.emit("core-event", json!({"type":"core","status":"stopped"}));
            })?;
        thread::Builder::new()
            .name("controller-core-errors".into())
            .spawn(move || {
                for line in BufReader::new(errors).lines().map_while(Result::ok) {
                    eprintln!("core: {line}");
                }
            })?;
        Ok((child, writer))
    }

    fn spawn(handle: tauri::AppHandle) -> io::Result<Arc<Self>> {
        let (child, writer) = Self::spawn_child(&handle)?;
        let bridge = Arc::new(Self {
            writer: Mutex::new(Some(writer)),
            child: Mutex::new(Some(child)),
            handle,
            stopping: AtomicBool::new(false),
        });
        let watcher = Arc::clone(&bridge);
        thread::Builder::new()
            .name("controller-core-watch".into())
            .spawn(move || watcher.watch_and_respawn())
            .map_err(io::Error::other)?;
        Ok(bridge)
    }

    fn watch_and_respawn(self: Arc<Self>) {
        loop {
            thread::sleep(Duration::from_millis(400));
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            let dead = self
                .child
                .lock()
                .ok()
                .and_then(|mut guard| {
                    guard.as_mut().map(|child| match child.try_wait() {
                        Ok(Some(_)) => true,
                        Ok(None) => false,
                        Err(_) => true,
                    })
                })
                .unwrap_or(true);
            if !dead {
                continue;
            }
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            let _ = self.handle.emit(
                "core-event",
                json!({"type":"core","status":"restarting","message":"Restarting core…"}),
            );
            thread::sleep(Duration::from_millis(350));
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            match Self::spawn_child(&self.handle) {
                Ok((child, writer)) => {
                    if let Ok(mut guard) = self.writer.lock() {
                        *guard = Some(writer);
                    }
                    if let Ok(mut guard) = self.child.lock() {
                        *guard = Some(child);
                    }
                    let _ = self.handle.emit(
                        "core-event",
                        json!({"type":"core","status":"ready","message":"Core restarted"}),
                    );
                    let _ = self.handle.emit(
                        "core-event",
                        json!({"type":"notice","message":"Core restarted"}),
                    );
                }
                Err(error) => {
                    let _ = self.handle.emit(
                        "core-event",
                        json!({"type":"error","message":format!("Core restart failed: {error}")}),
                    );
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    }

    fn send(&self, value: &Value) -> Result<(), String> {
        let mut guard = self.writer.lock().map_err(|_| "Core pipe lock failed")?;
        let writer = guard.as_mut().ok_or("Device core is restarting…")?;
        serde_json::to_writer(&mut *writer, value).map_err(|e| e.to_string())?;
        writer
            .write_all(b"\n")
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())
    }

    fn shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.writer.lock()
            && let Some(mut writer) = guard.take()
        {
            let _ = writer.write_all(b"{\"action\":\"shutdown\"}\n");
            let _ = writer.flush();
        }
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
        {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                thread::sleep(Duration::from_millis(25));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tauri::command]
fn send_command(command: Value, bridge: State<'_, Arc<Bridge>>) -> Result<(), String> {
    let parsed: device::DeviceCommand =
        serde_json::from_value(command.clone()).map_err(|e| e.to_string())?;
    if matches!(parsed, device::DeviceCommand::Shutdown) {
        return Err("Shutdown is reserved for the desktop shell".into());
    }
    bridge.send(&command)
}

#[tauri::command]
fn preview_authoring(
    clip: Option<audio_haptics::authoring::EffectClip>,
    trigger: Option<aftertouch_protocol::TriggerSetting>,
    personal: Option<audio_haptics::authoring::PersonalSettings>,
) -> Result<Value, String> {
    use audio_haptics::authoring::{inspect_clip, levels, limit_trigger};
    let personal = personal.unwrap_or_default();
    personal.validate().map_err(str::to_string)?;
    if let Some(clip) = clip {
        return inspect_clip(&clip, &personal);
    }
    let setting = trigger.ok_or("Choose a clip or trigger to inspect")?;
    let block = limit_trigger(
        device::encode_live_trigger(&setting).map_err(str::to_string)?,
        personal.cap(),
    );
    Ok(
        serde_json::json!({"requested":setting,"packet":block,"levels":levels(&block),"triggerLimit":personal.cap(),"frequencyHz":if block[0]==0x26{Some(block[9])}else{None},"note":if matches!(setting.mode.as_str(),"bow"|"weapon"){"Initial packet. Live pull/release rules can change it."}else{"Encoded force steps; not measured force."}}),
    )
}

#[tauri::command]
async fn choose_audio_file(
    app: tauri::AppHandle,
) -> Result<Option<audio_haptics::file::AudioFileInfo>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("Choose audio")
            .add_filter(
                "Audio",
                &["wav", "mp3", "flac", "ogg", "m4a", "aiff", "aif"],
            )
            .blocking_pick_file();
        let Some(selected) = selected else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|e| e.to_string())?;
        audio_haptics::file::inspect_file(&path).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(windows)]
fn shell_open(target: &OsStr, verb: &str) -> Result<(), String> {
    let verb: Vec<u16> = OsStr::new(verb).encode_wide().chain([0]).collect();
    let target: Vec<u16> = target.encode_wide().chain([0]).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if (result as usize) <= 32 {
        Err(format!(
            "Windows could not open driver setup (code {}).",
            result as usize
        ))
    } else {
        Ok(())
    }
}

#[tauri::command]
fn open_driver_setup(kind: &str, app: tauri::AppHandle) -> Result<String, String> {
    #[cfg(windows)]
    {
        let (url, label) = match kind {
            "vigem" => (
                "https://github.com/nefarius/ViGEmBus/releases/tag/v1.22.0",
                "ViGEmBus",
            ),
            "hidhide" => ("https://github.com/nefarius/HidHide/releases", "HidHide"),
            _ => return Err("Unknown driver setup".into()),
        };
        if kind == "vigem" {
            let bundled = app
                .path()
                .resource_dir()
                .map_err(|e| e.to_string())?
                .join("resources")
                .join("ViGEmBus_1.22.0_x64_x86_arm64.exe");
            if bundled.is_file() {
                shell_open(bundled.as_os_str(), "runas")?;
                return Ok("Signed ViGEmBus setup launched".into());
            }
        }
        shell_open(OsStr::new(url), "open")?;
        Ok(format!("Opened official {label} download page"))
    }
    #[cfg(not(windows))]
    {
        let _ = (kind, app);
        Err("Driver setup is available on Windows only".into())
    }
}

#[tauri::command]
fn load_profiles() -> Result<Option<Value>, String> {
    profiles::load()
}

#[tauri::command]
fn save_profiles(data: Value) -> Result<(), String> {
    profiles::save(data)
}

#[tauri::command]
fn export_profiles(data: Value) -> Result<String, String> {
    profiles::export(data)
}

#[tauri::command]
fn profile_backups(create: bool) -> Result<Vec<profiles::Backup>, String> {
    profiles::backups(create)
}
#[tauri::command]
fn restore_profile_backup(id: String) -> Result<Value, String> {
    profiles::restore(&id)
}
#[tauri::command]
async fn installed_steam_games() -> Result<Vec<steam::Game>, String> {
    tauri::async_runtime::spawn_blocking(steam::installed)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn platform_diagnostics() -> Value {
    diagnostics::inspect()
}
#[tauri::command]
fn send_game_packet(port: u16, format: String, packet: String) -> Result<(), String> {
    game_link::LinkSettings {
        enabled: true,
        port,
        format: format.clone(),
        ..game_link::LinkSettings::default()
    }
    .validate()
    .map_err(str::to_string)?;
    game_link::decode(packet.as_bytes(), &format).map_err(str::to_string)?;
    std::net::UdpSocket::bind(("127.0.0.1", 0))
        .and_then(|s| s.send_to(packet.as_bytes(), ("127.0.0.1", port)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn send_demo_telemetry(port: u16) -> Result<(), String> {
    if port < 1024 {
        return Err("Choose a telemetry port from 1024 to 65535".into());
    }
    let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    socket
        .send_to(
            br#"{"rpm":6800,"maxRpm":8000,"speedKmh":112,"gear":4,"brake":160,"slip":1.4,"frontSlip":1.3,"kerb":[true,false,false,false]}"#,
            ("127.0.0.1", port),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn run_core() {
    // A 1 MiB trace is JSON-escaped inside its command envelope.
    const MAX_CORE_COMMAND_BYTES: usize = 2 * 1024 * 1024 + 4096;
    let (commands_tx, commands_rx) = mpsc::channel();
    let (events_tx, events_rx) = mpsc::channel();
    let command_errors = events_tx.clone();
    thread::Builder::new()
        .name("controller-core-input".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(line) if line.len() <= MAX_CORE_COMMAND_BYTES => {
                        match serde_json::from_str::<device::DeviceCommand>(&line) {
                            Ok(command) => {
                                if commands_tx.send(command).is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                let _ = command_errors.send(json!({
                                    "type": "error",
                                    "message": format!("Ignored malformed core command: {error}")
                                }));
                            }
                        }
                    }
                    Ok(_) => {
                        let _ = command_errors.send(json!({
                            "type": "error",
                            "message": "Ignored oversized core command"
                        }));
                    }
                    Err(_) => break,
                }
            }
            let _ = commands_tx.send(device::DeviceCommand::Shutdown);
        })
        .expect("core command reader");
    let output_stop = std::sync::Arc::new(AtomicBool::new(false));
    let writer_stop = output_stop.clone();
    let output = thread::Builder::new()
        .name("controller-core-output".into())
        .spawn(move || {
            let mut writer = BufWriter::new(io::stdout().lock());
            let mut drained = 0;
            loop {
                if writer_stop.load(Ordering::Acquire) && drained >= 256 {
                    break;
                }
                let event = match events_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(event) => event,
                    Err(mpsc::RecvTimeoutError::Timeout)
                        if !writer_stop.load(Ordering::Acquire) =>
                    {
                        continue;
                    }
                    Err(_) => break,
                };
                if writer_stop.load(Ordering::Acquire) {
                    drained += 1;
                }
                if serde_json::to_writer(&mut writer, &event).is_err()
                    || writer.write_all(b"\n").is_err()
                    || writer.flush().is_err()
                {
                    break;
                }
            }
        })
        .expect("core event writer");
    let disable_hid = std::env::args().any(|arg| arg == "--no-hid");
    if disable_hid {
        hidapi::HidApi::disable_device_discovery();
    }
    device::run(commands_rx, events_tx, disable_hid);
    output_stop.store(true, Ordering::Release);
    let _ = output.join();
}

fn main() {
    if std::env::args().any(|arg| arg == "--core") {
        run_core();
        return;
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let bridge = Bridge::spawn(app.handle().clone())?;
            app.manage(bridge);
            app.manage(updates::PendingUpdate::default());
            app.manage(TraySettings {
                close_to_tray: AtomicBool::new(read_close_to_tray(app.handle())),
            });
            let show = MenuItem::with_id(app, "show", "Show CONTROLLER", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit CONTROLLER", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let icon = app.default_window_icon().ok_or("App icon missing")?.clone();
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("CONTROLLER")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    ) && let Some(window) = tray.app_handle().get_webview_window("main")
                    {
                        let _ = window.unminimize();
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_command,
            preview_authoring,
            choose_audio_file,
            open_driver_setup,
            load_profiles,
            hardware::controller_library,
            hardware::controller_audio_endpoints,
            hardware::save_controller_library,
            hardware::inspect_edge_snapshot,
            hardware::edit_edge_snapshot,
            hardware::store_edge_snapshot,
            hardware::edge_snapshots,
            hardware::delete_edge_snapshot,
            output_calibration::save_output_calibration,
            save_profiles,
            export_profiles,
            profile_backups,
            restore_profile_backup,
            installed_steam_games,
            platform_diagnostics,
            send_game_packet,
            send_demo_telemetry,
            get_app_settings,
            set_close_to_tray,
            set_start_on_login,
            platform_capabilities,
            integrations::integration_info,
            integrations::inspect_native_report,
            desktop_access::desktop_access_status,
            desktop_access::request_desktop_access,
            desktop_access::open_desktop_access_settings,
            desktop_access::record_shortcut,
            desktop_access::cancel_shortcut_recording,
            updates::check_update,
            updates::install_update
        ])
        .build(tauri::generate_context!())
        .expect("CONTROLLER desktop shell");
    app.run(|handle, event| match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main"
            && handle
                .try_state::<TraySettings>()
                .is_some_and(|s| s.close_to_tray.load(Ordering::SeqCst)) =>
        {
            api.prevent_close();
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.hide();
            }
        }
        tauri::RunEvent::Exit => {
            if let Some(bridge) = handle.try_state::<Arc<Bridge>>() {
                bridge.shutdown();
            }
        }
        _ => {}
    });
}
