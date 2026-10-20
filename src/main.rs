#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod device;
mod processes;
mod profiles;
mod touch_mouse;

use std::{
    io::{self, BufRead, BufReader, BufWriter, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Mutex, mpsc},
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
}

impl Bridge {
    fn spawn(handle: tauri::AppHandle) -> io::Result<Self> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--core")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW for the background HID process.
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
        let event_handle = handle.clone();
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
                    json!({"type":"device","connected":false,"message":"Device core stopped"}),
                );
            })?;
        thread::Builder::new()
            .name("aftertouch-core-errors".into())
            .spawn(move || {
                for line in BufReader::new(errors).lines().map_while(Result::ok) {
                    eprintln!("core: {line}");
                }
            })?;
        Ok(Self {
            writer: Mutex::new(Some(writer)),
            child: Mutex::new(Some(child)),
        })
    }

    fn send(&self, value: &Value) -> Result<(), String> {
        let mut guard = self.writer.lock().map_err(|_| "Core pipe lock failed")?;
        let writer = guard.as_mut().ok_or("Device core has stopped")?;
        serde_json::to_writer(&mut *writer, value).map_err(|e| e.to_string())?;
        writer
            .write_all(b"\n")
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())
    }

    fn shutdown(&self) {
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
fn send_command(command: Value, bridge: State<'_, Bridge>) -> Result<(), String> {
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
    thread::Builder::new()
        .name("aftertouch-core-input".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(line) if line.len() <= 65_536 => {
                        if let Ok(command) = serde_json::from_str::<device::DeviceCommand>(&line)
                            && commands_tx.send(command).is_err()
                        {
                            break;
                        }
                    }
                    Ok(_) => {}
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
            app.manage(Bridge::spawn(app.handle().clone())?);
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
            && let Some(bridge) = handle.try_state::<Bridge>()
        {
            bridge.shutdown();
        }
    });
}
