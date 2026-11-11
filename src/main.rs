#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_log;
mod audio_haptics;
mod core_io;
mod core_service;
mod desktop_access;
mod device;
mod diagnostics;
mod edge_writer;
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
mod native_provider;
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
#[cfg(all(test, windows))]
#[path = "screen_pixels.rs"]
mod screen_pixels;
mod script;
mod steam;
mod telemetry;
mod ui_security;
mod updates;
mod virtual_gamepad;
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
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use tauri_plugin_notification::NotificationExt;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    close_to_tray: bool,
    start_on_login: bool,
    background_core: bool,
    background_active: bool,
    start_minimized: bool,
    /// "starting", "running" or "failed".
    core_state: String,
    core_error: Option<String>,
}

struct TraySettings {
    close_to_tray: AtomicBool,
}

const SETTINGS_LIMIT: u64 = 16 * 1024;

fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("settings.json"))
}
/// Reads the settings file. An unusable file (corrupt, oversized, symlink, not an object) yields
/// defaults and `true`; a missing or unreadable one yields defaults and `false`.
fn load_settings_file(path: &std::path::Path) -> (Value, bool) {
    try_load_settings_file(path).unwrap_or_else(|_| (json!({}), false))
}
/// Like `load_settings_file`, but a transient I/O error (locked or unreadable file) is an
/// `Err`: the file may be perfectly good and must never be treated as unusable.
fn try_load_settings_file(path: &std::path::Path) -> std::io::Result<(Value, bool)> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((json!({}), false)),
        Err(e) => return Err(e),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > SETTINGS_LIMIT {
        return Ok((json!({}), true));
    }
    let bytes = std::fs::read(path)?;
    Ok(match serde_json::from_slice::<Value>(&bytes) {
        Ok(data) if data.is_object() => (data, false),
        _ => (json!({}), true),
    })
}
/// Stores one key. A broken file is moved to a unique `settings.json.bak*` and replaced atomically.
fn store_setting_file(path: &std::path::Path, key: &str, value: Value) -> Result<(), String> {
    let (mut data, unusable) = try_load_settings_file(path).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().ok_or("Settings folder missing")?)
        .map_err(|e| e.to_string())?;
    if unusable {
        let mut backup = path.with_extension("json.bak");
        let mut n = 1u32;
        while std::fs::symlink_metadata(&backup).is_ok() {
            backup = path.with_extension(format!("json.bak{n}"));
            n += 1;
        }
        if std::fs::rename(path, &backup).is_err() {
            let _ = std::fs::remove_file(path);
        }
        app_log::warn("settings.json was unusable; defaults restored, original kept as .bak");
    }
    data[key] = value;
    profiles::write_archive(path, &serde_json::to_vec(&data).map_err(|e| e.to_string())?)
}
fn setting_bool(app: &tauri::AppHandle, key: &str, default: bool) -> bool {
    settings_path(app)
        .ok()
        .and_then(|path| load_settings_file(&path).0.get(key)?.as_bool())
        .unwrap_or(default)
}
fn save_setting(app: &tauri::AppHandle, key: &str, value: bool) -> Result<(), String> {
    store_setting_file(&settings_path(app)?, key, json!(value))
}
fn read_background_core(app: &tauri::AppHandle) -> bool {
    setting_bool(app, "backgroundCore", false)
}
fn read_close_to_tray(app: &tauri::AppHandle) -> bool {
    setting_bool(app, "closeToTray", true)
}
fn no_hid() -> bool {
    std::env::args().any(|a| a == "--no-hid")
}

#[tauri::command]
fn get_app_settings(app: tauri::AppHandle, tray: State<'_, TraySettings>) -> AppSettings {
    let bridge = app.try_state::<Arc<Bridge>>();
    AppSettings {
        close_to_tray: tray.close_to_tray.load(Ordering::SeqCst),
        start_on_login: app.autolaunch().is_enabled().unwrap_or(false),
        background_core: read_background_core(&app),
        background_active: bridge
            .as_ref()
            .is_some_and(|b| b.background_active.load(Ordering::Acquire)),
        start_minimized: setting_bool(&app, "startMinimized", true),
        core_state: bridge
            .as_ref()
            .map_or("starting", |b| b.core_state())
            .to_owned(),
        core_error: bridge.as_ref().and_then(|b| b.core_error()),
    }
}

#[tauri::command]
fn get_core_status(app: tauri::AppHandle) -> Value {
    let bridge = app.try_state::<Arc<Bridge>>();
    json!({
        "state": bridge.as_ref().map_or("starting", |b| b.core_state()),
        "error": bridge.as_ref().and_then(|b| b.core_error()),
        "backgroundActive": bridge.as_ref().is_some_and(|b| b.background_active.load(Ordering::Acquire)),
        "build": core_service::build(),
    })
}

#[tauri::command]
fn set_close_to_tray(
    enabled: bool,
    app: tauri::AppHandle,
    tray: State<'_, TraySettings>,
) -> Result<(), String> {
    save_setting(&app, "closeToTray", enabled)?;
    tray.close_to_tray.store(enabled, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
fn set_background_core(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    save_setting(&app, "backgroundCore", enabled)
}
/// Whether an autostart launch (`--minimized`) leaves the window hidden in the tray.
#[tauri::command]
fn set_start_minimized(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    save_setting(&app, "startMinimized", enabled)
}
#[tauri::command]
async fn restart_core(bridge: State<'_, Arc<Bridge>>) -> Result<(), String> {
    let bridge = Arc::clone(&bridge);
    tauri::async_runtime::spawn_blocking(move || bridge.restart())
        .await
        .map_err(|e| e.to_string())?
}

/// Registrations made before the plugin args became `--minimized` lack that flag until the user
/// toggles start-on-login. `enable()` rewrites the same registry value / launch agent, so it is
/// idempotent and cheap; do it once per args revision and only when already enabled.
fn refresh_autostart_registration(app: &tauri::AppHandle) {
    const MARKER: &str = "autostartArgsMinimizedV1";
    if setting_bool(app, MARKER, false) || !app.autolaunch().is_enabled().unwrap_or(false) {
        return;
    }
    match app.autolaunch().enable() {
        Ok(()) => {
            let _ = save_setting(app, MARKER, true);
        }
        Err(error) => app_log::warn(&format!(
            "Could not refresh the start-on-login entry: {error}"
        )),
    }
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
    json!({"os": std::env::consts::OS, "desktopInput":integrations::environment()["desktopCandidate"], "nativeAudioFiles":true,"processAudioCapture":cfg!(windows)&&integrations::windows_build()>=20348, "supportedKeys":(1..=254u16).filter(|k|input_map::supported_key(*k)).collect::<Vec<_>>(), "xinput": cfg!(any(windows,target_os="linux")), "screenColor": cfg!(any(windows,target_os="linux",target_os="macos")), "systemAudioBridge": audio_haptics::capture_supported(), "updates": updates::supported(), "notifications": true})
}

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;

// ---- tray -------------------------------------------------------------------------------

struct TrayState {
    status: MenuItem<tauri::Wry>,
    tray: TrayIcon,
    profile: Mutex<String>,
    connection: Mutex<String>,
}

fn tray_status_text(profile: &str, connection: &str) -> String {
    if profile.is_empty() {
        connection.to_owned()
    } else {
        format!("{profile} \u{b7} {connection}")
    }
}

fn update_tray(app: &tauri::AppHandle, profile: Option<&str>, connection: Option<&str>) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    if let Some(profile) = profile
        && let Ok(mut current) = state.profile.lock()
    {
        *current = profile.chars().take(60).collect();
    }
    if let Some(connection) = connection
        && let Ok(mut current) = state.connection.lock()
    {
        *current = connection.to_owned();
    }
    let profile = state.profile.lock().map(|g| g.clone()).unwrap_or_default();
    let connection = state
        .connection
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    let text = tray_status_text(&profile, &connection);
    let _ = state.status.set_text(&text);
    let _ = state.tray.set_tooltip(Some(format!("CONTROLLER\n{text}")));
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Lets the UI put the active profile name in the tray status line and tooltip.
#[tauri::command]
fn set_tray_profile(name: String, app: tauri::AppHandle) {
    update_tray(&app, Some(&name), None);
}

fn note_core_event(app: &tauri::AppHandle, event: &Value) {
    match event["type"].as_str() {
        Some("device") => {
            if let Some(connected) = event["connected"].as_bool() {
                update_tray(
                    app,
                    None,
                    Some(if connected {
                        "Controller connected"
                    } else {
                        "No controller"
                    }),
                );
            }
        }
        Some("core") => match event["status"].as_str() {
            Some("restarting") | Some("stopped") => update_tray(app, None, Some("Core restarting")),
            Some("failed") => update_tray(app, None, Some("Core failed")),
            _ => {}
        },
        _ => {}
    }
}

// ---- core bridge -------------------------------------------------------------------------

struct Bridge {
    writer: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    handle: tauri::AppHandle,
    stopping: AtomicBool,
    background_active: AtomicBool,
    /// Set by "Quit and stop background core".
    force_stop_service: AtomicBool,
    lifecycle: Mutex<()>,
    /// Commands wait here; `send` never blocks on the child's pipe.
    outbox: core_io::Outbox,
    core_error: Mutex<Option<String>>,
}

impl Bridge {
    fn spawn_child(app: &tauri::AppHandle) -> io::Result<(Child, ChildStdin, bool)> {
        let background = read_background_core(app);
        if !background && let Err(error) = core_service::stop_and_wait(no_hid()) {
            // A stale service must not keep the app from starting; run the built-in core.
            app_log::warn(&format!("Could not stop the background core: {error}"));
            let _ = app.emit(
                "core-event",
                json!({"type":"notice","message":format!("Could not stop the background core ({error}). Using the built-in core.")}),
            );
        }
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg(if background {
                "--core-client"
            } else {
                "--core"
            })
            .env("CONTROLLER_GUI_PID", std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        if no_hid() {
            command.arg("--no-hid");
        }
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
                                note_core_event(&event_handle, &event);
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
                note_core_event(&event_handle, &json!({"type":"core","status":"stopped"}));
            })?;
        thread::Builder::new()
            .name("controller-core-errors".into())
            .spawn(move || {
                for line in BufReader::new(errors).lines().map_while(Result::ok) {
                    app_log::core_line(&line);
                }
            })?;
        Ok((child, writer, background))
    }

    fn spawn(handle: tauri::AppHandle) -> io::Result<Arc<Self>> {
        let bridge = Arc::new(Self {
            writer: Mutex::new(None),
            child: Mutex::new(None),
            background_active: AtomicBool::new(read_background_core(&handle)),
            handle,
            stopping: AtomicBool::new(false),
            force_stop_service: AtomicBool::new(false),
            lifecycle: Mutex::new(()),
            outbox: core_io::Outbox::new(512),
            core_error: Mutex::new(None),
        });
        // Starting the core (which may wait for an old service to stop) happens off the
        // setup/main thread; commands sent meanwhile are queued.
        let watcher = Arc::clone(&bridge);
        thread::Builder::new()
            .name("controller-core-watch".into())
            .spawn(move || watcher.watch_and_respawn())?;
        let pump = Arc::clone(&bridge);
        thread::Builder::new()
            .name("controller-core-writer".into())
            .spawn(move || pump.pump())?;
        Ok(bridge)
    }

    fn core_error(&self) -> Option<String> {
        self.core_error.lock().ok().and_then(|e| e.clone())
    }
    fn core_state(&self) -> &'static str {
        if self.core_error().is_some() {
            "failed"
        } else if self.child.lock().is_ok_and(|c| c.is_some()) {
            "running"
        } else {
            "starting"
        }
    }

    /// Sole owner of writes to the child's stdin, so a stuck pipe never stalls a Tauri command.
    fn pump(self: Arc<Self>) {
        loop {
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            let Some((item, epoch)) = self.outbox.pop_wait_epoch(Duration::from_millis(250)) else {
                continue;
            };
            loop {
                let Ok(mut guard) = self.writer.lock() else {
                    return;
                };
                // The queue was cleared (core restart or failure) after this item was popped:
                // it was meant for the previous core and must not reach the next one.
                if self.outbox.epoch() != epoch {
                    break;
                }
                match guard.as_mut() {
                    Some(writer) => {
                        // A failed write means the child is gone; the watcher respawns it.
                        let _ = writer.write_all(&item.1).and_then(|_| writer.flush());
                        break;
                    }
                    None => {
                        drop(guard);
                        if self.stopping.load(Ordering::SeqCst) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        }
    }

    fn watch_and_respawn(self: Arc<Self>) {
        let mut first = true;
        loop {
            let initial = std::mem::replace(&mut first, false);
            if !initial {
                thread::sleep(Duration::from_millis(400));
            }
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            let Ok(lifecycle) = self.lifecycle.lock() else {
                return;
            };
            // Tray updates marshal to the main thread, which may be waiting in `shutdown` for
            // this very lock, so they are deferred until the guard is released.
            let tray: Option<&'static str>;
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
            // Commands queued for the dead core would replay stale state into the new one.
            self.outbox.clear();
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            if !initial {
                let _ = self.handle.emit(
                    "core-event",
                    json!({"type":"core","status":"restarting","message":"Restarting core..."}),
                );
                thread::sleep(Duration::from_millis(350));
                if self.stopping.load(Ordering::SeqCst) {
                    return;
                }
            }
            match Self::spawn_child(&self.handle) {
                Ok((child, writer, background)) => {
                    self.background_active.store(background, Ordering::Release);
                    if let Ok(mut error) = self.core_error.lock() {
                        *error = None;
                    }
                    if let Ok(mut guard) = self.writer.lock() {
                        *guard = Some(writer);
                    }
                    if let Ok(mut guard) = self.child.lock() {
                        *guard = Some(child);
                    }
                    // After the first start the service replay reports the real state; only a
                    // respawned core is known to have no controller yet.
                    tray = if initial { None } else { Some("No controller") };
                    if !initial {
                        let _ = self.handle.emit(
                            "core-event",
                            json!({"type":"core","status":"ready","message":"Core restarted"}),
                        );
                        let _ = self.handle.emit(
                            "core-event",
                            json!({"type":"notice","message":"Core restarted"}),
                        );
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    app_log::error(&format!("Core start failed: {message}"));
                    if let Ok(mut slot) = self.core_error.lock() {
                        *slot = Some(message.clone());
                    }
                    self.outbox.clear();
                    tray = Some("Core failed");
                    let _ = self.handle.emit(
                        "core-event",
                        json!({"type":"core","status":"failed","message":format!("Core failed to start: {message}")}),
                    );
                    let _ = self.handle.emit(
                        "core-event",
                        json!({"type":"error","message":format!("Core start failed: {message}")}),
                    );
                    drop(lifecycle);
                    if let Some(text) = tray {
                        update_tray(&self.handle, None, Some(text));
                    }
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            }
            drop(lifecycle);
            if let Some(text) = tray {
                update_tray(&self.handle, None, Some(text));
            }
        }
    }

    /// Queues a command. Never blocks: slider-style commands replace their queued predecessor,
    /// everything else is refused with an error when the queue is full.
    fn send(&self, value: &Value) -> Result<(), String> {
        if self.stopping.load(Ordering::Acquire) {
            return Err("CONTROLLER is shutting down.".into());
        }
        if let Some(error) = self.core_error() {
            return Err(format!("Device core failed to start: {error}"));
        }
        let mut line = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if line.len() >= core_io::MAX_LINE {
            return Err("Command is too large for the device core.".into());
        }
        line.push(b'\n');
        self.outbox
            .push(core_io::coalesce_key(value), line)
            .map_err(|_| "Device core is busy; try again in a moment.".to_string())
    }

    fn restart(&self) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "Core lifecycle lock failed")?;
        if self.stopping.load(Ordering::Acquire) {
            return Err("Core is stopping.".into());
        }
        self.close_child(true);
        if self.background_active.load(Ordering::Acquire) {
            core_service::stop_and_wait(no_hid()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn shutdown(&self) {
        let stop_service =
            self.force_stop_service.load(Ordering::Acquire) || !read_background_core(&self.handle);
        self.shutdown_inner(stop_service);
        if stop_service {
            self.stop_service_blocking();
        }
    }
    fn shutdown_for_update(&self) {
        self.shutdown_inner(true);
        // The installer replaces files the background core has open; wait until it is gone.
        self.stop_service_blocking();
    }
    fn stop_service_blocking(&self) {
        if let Err(error) = core_service::stop_and_wait(no_hid()) {
            app_log::warn(&format!("Background core did not stop cleanly: {error}"));
        }
    }
    fn shutdown_inner(&self, stop_service: bool) {
        self.stopping.store(true, Ordering::SeqCst);
        let Ok(_lifecycle) = self.lifecycle.lock() else {
            return;
        };
        self.close_child(stop_service);
    }
    fn close_child(&self, stop_service: bool) {
        self.outbox.clear();
        let mut writer = None;
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match self.writer.try_lock() {
                Ok(mut guard) => {
                    writer = guard.take();
                    break;
                }
                Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                    writer = poisoned.into_inner().take();
                    break;
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        // The writer thread is stuck on a full pipe; killing the child frees it.
                        if let Ok(mut guard) = self.child.lock()
                            && let Some(child) = guard.as_mut()
                        {
                            let _ = child.kill();
                        }
                        if let Ok(mut guard) = self.writer.lock() {
                            writer = guard.take();
                        }
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        if let Some(mut writer) = writer {
            let command = if stop_service && self.background_active.load(Ordering::Acquire) {
                b"{\"action\":\"shutdownService\"}\n".as_slice()
            } else {
                b"{\"action\":\"shutdown\"}\n".as_slice()
            };
            let _ = writer.write_all(command);
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

#[tauri::command]
fn profile_recovery_status() -> Result<Value, String> {
    profiles::recovery_status()
}
/// Restores the newest valid backup over an unreadable profile archive.
#[tauri::command]
fn recover_profiles() -> Result<Value, String> {
    profiles::recover()
}
/// Checks an archive (for example an imported single-profile file) without saving it.
#[tauri::command]
fn validate_profiles(data: Value) -> Result<Value, String> {
    profiles::validate_archive(data)
}
#[tauri::command]
async fn steam_input_status() -> Result<steam::InputStatus, String> {
    tauri::async_runtime::spawn_blocking(steam::input_status)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn get_telemetry_allowed_source() -> Option<String> {
    telemetry::read_allowed_source().map(|ip| ip.to_string())
}
/// Restricts LAN telemetry to one sender address; empty or null clears the restriction.
#[tauri::command]
fn set_telemetry_allowed_source(ip: Option<String>) -> Result<Option<String>, String> {
    telemetry::write_allowed_source(ip.as_deref()).map(|ip| ip.map(|ip| ip.to_string()))
}
/// Monitors available for screen colour sampling (index 0 is the primary).
#[tauri::command]
fn screen_monitors() -> Value {
    #[cfg(windows)]
    {
        json!(
            screen_color::monitors()
                .iter()
                .enumerate()
                .map(|(i, m)| json!({"index":i,"x":m.0,"y":m.1,"width":m.2,"height":m.3,"primary":i==0}))
                .collect::<Vec<_>>()
        )
    }
    #[cfg(not(windows))]
    {
        json!([])
    }
}

/// Replaces the user's home folder with `~`, ignoring case and `\` versus `/`.
fn redact_home(text: &str, home: &str) -> String {
    let fold = |s: &str| s.replace('\\', "/").to_ascii_lowercase();
    let needle = fold(home.trim_end_matches(['\\', '/']));
    if needle.is_empty() {
        return text.to_owned();
    }
    let hay = fold(text);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    while let Some(found) = hay[last..].find(&needle) {
        let start = last + found;
        out.push_str(&text[last..start]);
        out.push('~');
        last = start + needle.len();
    }
    out.push_str(&text[last..]);
    out
}
fn redact_home_in_value(value: &mut Value, home: &str) {
    match value {
        Value::String(text) => *text = redact_home(text, home),
        Value::Array(items) => items.iter_mut().for_each(|v| redact_home_in_value(v, home)),
        Value::Object(map) => map.values_mut().for_each(|v| redact_home_in_value(v, home)),
        _ => {}
    }
}
#[tauri::command]
fn export_diagnostics(save: Option<bool>, app: tauri::AppHandle) -> Result<Value, String> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|h| h.to_string_lossy().into_owned())
        .filter(|h| !h.is_empty());
    let (mut settings, _) = load_settings_file(&settings_path(&app)?);
    if let Some(map) = settings.as_object_mut() {
        for value in map.values_mut() {
            if value.is_string() {
                *value = json!("[redacted]");
            }
        }
    }
    let bridge = app.try_state::<Arc<Bridge>>();
    let mut report = json!({
        "app": "CONTROLLER",
        "version": env!("CARGO_PKG_VERSION"),
        "coreBuild": core_service::build(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "windowsBuild": integrations::windows_build(),
        "settings": settings,
        "core": {
            "state": bridge.as_ref().map_or("starting", |b| b.core_state()),
            "error": bridge.as_ref().and_then(|b| b.core_error()),
            "backgroundActive": bridge.as_ref().is_some_and(|b| b.background_active.load(Ordering::Acquire)),
        },
        "platform": diagnostics::inspect(),
        "log": app_log::recent_lines(300).into_iter().collect::<Vec<_>>(),
    });
    if let Some(home) = &home {
        redact_home_in_value(&mut report, home);
    }
    if !save.unwrap_or(false) {
        return Ok(json!({ "report": report }));
    }
    let folder = profiles::downloads_folder().ok_or("Downloads folder is unavailable")?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let path = folder.join(format!(
        "CONTROLLER-diagnostics-{stamp}-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(json!({ "path": path.to_string_lossy(), "report": report }))
}

fn run_core() {
    let (commands_tx, commands_rx) = mpsc::sync_channel(256);
    let (events_tx, events_rx) = mpsc::channel();
    let command_errors = events_tx.clone();
    thread::Builder::new()
        .name("controller-core-input".into())
        .spawn(move || {
            // A 1 MiB trace is JSON-escaped inside its command envelope.
            let mut lines=core_io::Lines::new(io::stdin().lock());let mut buffer=Vec::new();
            loop {
                match lines.next(&mut buffer,core_io::MAX_LINE) {
                    Ok(core_io::Line::Data)=>match serde_json::from_slice::<device::DeviceCommand>(&buffer) {
                        Ok(command)=>if commands_tx.send(command).is_err(){break;},
                        Err(error)=>{let _=command_errors.send(json!({"type":"error","message":format!("Ignored malformed core command: {error}")}));}
                    },
                    Ok(core_io::Line::TooLong)=>{let _=command_errors.send(json!({"type":"error","message":"Ignored oversized core command"}));},
                    Ok(core_io::Line::Eof)|Err(_)=>break,
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
    let disable_hid = no_hid();
    if disable_hid {
        hidapi::HidApi::disable_device_discovery();
    }
    device::run(commands_rx, events_tx, disable_hid);
    output_stop.store(true, Ordering::Release);
    let _ = output.join();
}

fn main() {
    let no_hid = no_hid();
    let has = |flag: &str| std::env::args().any(|arg| arg == flag);
    if has("--core-service") {
        app_log::init_service();
        if let Err(error) = core_service::serve(no_hid) {
            app_log::error(&format!("Background core: {error}"));
            std::process::exit(1);
        }
        return;
    }
    if has("--core-client") {
        app_log::init_stderr();
        if let Err(error) = core_service::proxy(no_hid) {
            eprintln!("Core connection: {error}");
            std::process::exit(1);
        }
        return;
    }
    if has("--stop-background-core") {
        // Used by the installer hooks: ask the service to exit and wait until it has.
        if let Err(error) = core_service::stop_and_wait(no_hid) {
            eprintln!("Core stop: {error}");
            std::process::exit(1);
        }
        return;
    }
    if has("--core") {
        app_log::init_stderr();
        run_core();
        return;
    }
    app_log::init_gui();
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            // An autostart relaunch (`--minimized`) must not pop the window up.
            if !args.iter().any(|a| a == "--minimized") {
                show_main(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            let minimized = std::env::args().any(|a| a == "--minimized")
                && setting_bool(app.handle(), "startMinimized", true);
            let config = app
                .config()
                .app
                .windows
                .first()
                .ok_or("Main window configuration is missing")?;
            let window = tauri::WebviewWindowBuilder::from_config(app, config)?
                .on_navigation(ui_security::allowed_navigation);
            let window = if minimized {
                window.visible(false)
            } else {
                window
            };
            window.build()?;
            app.manage(updates::PendingUpdate::default());
            app.manage(TraySettings {
                close_to_tray: AtomicBool::new(read_close_to_tray(app.handle())),
            });
            let show = MenuItem::with_id(app, "show", "Show CONTROLLER", true, None::<&str>)?;
            let status = MenuItem::with_id(app, "status", "Core starting", false, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_stop = MenuItem::with_id(
                app,
                "quit_stop",
                "Quit and stop background core",
                true,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Quit CONTROLLER", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &status, &separator, &quit_stop, &quit])?;
            let icon = app.default_window_icon().ok_or("App icon missing")?.clone();
            let tray = TrayIconBuilder::new()
                .icon(icon)
                .tooltip("CONTROLLER")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit_stop" => {
                        if let Some(bridge) = app.try_state::<Arc<Bridge>>() {
                            bridge.force_stop_service.store(true, Ordering::Release);
                        }
                        app.exit(0);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            app.manage(TrayState {
                status,
                tray,
                profile: Mutex::new(String::new()),
                connection: Mutex::new("Core starting".into()),
            });
            // Never fatal: a failing core is reported to the UI and retried by the watcher.
            let bridge = Bridge::spawn(app.handle().clone())?;
            app.manage(bridge);
            refresh_autostart_registration(app.handle());
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
            edge_writer::edge_transactions,
            edge_writer::archive_edge_transaction,
            output_calibration::save_output_calibration,
            save_profiles,
            export_profiles,
            profile_backups,
            restore_profile_backup,
            profile_recovery_status,
            recover_profiles,
            validate_profiles,
            installed_steam_games,
            steam_input_status,
            platform_diagnostics,
            export_diagnostics,
            send_game_packet,
            send_demo_telemetry,
            get_telemetry_allowed_source,
            set_telemetry_allowed_source,
            screen_monitors,
            get_app_settings,
            get_core_status,
            set_close_to_tray,
            set_background_core,
            set_start_minimized,
            set_tray_profile,
            restart_core,
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
        .build(tauri::generate_context!());
    let app = match built {
        Ok(app) => app,
        Err(error) => {
            app_log::error(&format!(
                "CONTROLLER desktop shell failed to start: {error}"
            ));
            eprintln!("CONTROLLER desktop shell failed to start: {error}");
            std::process::exit(1);
        }
    };
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
            // First time only: tell the user the app is still running in the tray.
            if !setting_bool(handle, "trayNoticeShown", false) {
                let _ = save_setting(handle, "trayNoticeShown", true);
                let message = "CONTROLLER is still running in the system tray. Use the tray icon to reopen or quit it.";
                let _ = handle.emit("trayHidden", json!({ "message": message }));
                let _ = handle
                    .notification()
                    .builder()
                    .title("CONTROLLER is still running")
                    .body(message)
                    .show();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "controller-settings-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn corrupt_settings_fall_back_and_are_replaced_with_a_backup() {
        let path = temp_file("corrupt");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(load_settings_file(&path), (json!({}), true));
        store_setting_file(&path, "closeToTray", json!(false)).unwrap();
        assert_eq!(
            load_settings_file(&path),
            (json!({"closeToTray":false}), false)
        );
        assert_eq!(
            std::fs::read(path.with_extension("json.bak")).unwrap(),
            b"{not json"
        );
        // Toggles keep working afterwards and keep earlier keys.
        store_setting_file(&path, "backgroundCore", json!(true)).unwrap();
        assert_eq!(
            load_settings_file(&path).0,
            json!({"closeToTray":false,"backgroundCore":true})
        );
    }

    #[test]
    fn home_redaction_ignores_case_and_slash_style() {
        let home = "C:\\Users\\Ann";
        assert_eq!(
            redact_home("c:/users/ann/AppData and C:\\USERS\\ANN\\x", home),
            "~/AppData and ~\\x"
        );
        let mut v = json!({"core":{"error":"fail at c:\\users\\ann\\a"},"n":[1,"C:/Users/Ann"]});
        redact_home_in_value(&mut v, home);
        assert_eq!(v["core"]["error"], "fail at ~\\a");
        assert_eq!(v["n"][1], "~");
    }

    #[test]
    fn existing_backups_are_never_overwritten() {
        let path = temp_file("unique-bak");
        std::fs::write(&path, b"{first").unwrap();
        store_setting_file(&path, "a", json!(1)).unwrap();
        std::fs::write(&path, b"{second").unwrap();
        store_setting_file(&path, "b", json!(2)).unwrap();
        assert_eq!(
            std::fs::read(path.with_extension("json.bak")).unwrap(),
            b"{first"
        );
        assert_eq!(
            std::fs::read(path.with_extension("json.bak1")).unwrap(),
            b"{second"
        );
    }

    #[test]
    fn oversized_or_non_object_settings_are_treated_as_unusable() {
        let path = temp_file("oversized");
        std::fs::write(&path, vec![b' '; (SETTINGS_LIMIT + 1) as usize]).unwrap();
        assert!(load_settings_file(&path).1);
        std::fs::write(&path, b"[1,2]").unwrap();
        assert!(load_settings_file(&path).1);
        store_setting_file(&path, "startMinimized", json!(true)).unwrap();
        assert_eq!(
            load_settings_file(&path),
            (json!({"startMinimized":true}), false)
        );
        let missing = path.with_file_name("none.json");
        assert_eq!(load_settings_file(&missing), (json!({}), false));
    }

    #[test]
    fn tray_status_line_combines_profile_and_connection() {
        assert_eq!(tray_status_text("", "No controller"), "No controller");
        assert_eq!(
            tray_status_text("FPS", "Controller connected"),
            "FPS \u{b7} Controller connected"
        );
    }
}
