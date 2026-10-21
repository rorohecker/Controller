#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_haptics;
mod device;
mod haptics;
mod input_map;
mod processes;
mod profiles;
mod screen_color;
mod script;
mod telemetry;
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

#[cfg(windows)]
use std::os::windows::process::CommandExt;

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
            .name("aftertouch-core-events".into())
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
            .name("aftertouch-core-errors".into())
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
            .name("aftertouch-core-watch".into())
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

fn run_core() {
    let (commands_tx, commands_rx) = mpsc::channel();
    let (events_tx, events_rx) = mpsc::channel();
    let command_errors = events_tx.clone();
    thread::Builder::new()
        .name("aftertouch-core-input".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(line) if line.len() <= 65_536 => {
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
    let output = thread::Builder::new()
        .name("aftertouch-core-output".into())
        .spawn(move || {
            let mut writer = BufWriter::new(io::stdout().lock());
            for event in events_rx {
                if serde_json::to_writer(&mut writer, &event).is_err()
                    || writer.write_all(b"\n").is_err()
                    || writer.flush().is_err()
                {
                    break;
                }
            }
        })
        .expect("core event writer");
    device::run(commands_rx, events_tx);
    let _ = output.join();
}

fn main() {
    if std::env::args().any(|arg| arg == "--core") {
        run_core();
        return;
    }
    let app = tauri::Builder::default()
        .setup(|app| {
            let bridge = Bridge::spawn(app.handle().clone())?;
            app.manage(bridge);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_command,
            load_profiles,
            save_profiles,
            export_profiles
        ])
        .build(tauri::generate_context!())
        .expect("Aftertouch desktop shell");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(bridge) = handle.try_state::<Arc<Bridge>>()
        {
            bridge.shutdown();
        }
    });
}
