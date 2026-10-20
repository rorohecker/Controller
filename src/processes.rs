use std::mem::size_of;

use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

pub struct ProcessState {
    pub conflicts: Vec<&'static str>,
    pub foreground_exe: Option<String>,
}

pub fn inspect() -> ProcessState {
    let mut state = ProcessState {
        conflicts: Vec::new(),
        foreground_exe: None,
    };
    let foreground = unsafe {
        let window = GetForegroundWindow();
        let mut pid = 0;
        if !window.is_null() {
            GetWindowThreadProcessId(window, &mut pid);
        }
        pid
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return state;
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        let end = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe = String::from_utf16_lossy(&entry.szExeFile[..end]).to_lowercase();
        if entry.th32ProcessID == foreground && exe != "aftertouch.exe" {
            state.foreground_exe = Some(exe.clone());
        }
        let tool = match exe.as_str() {
            "steam.exe" => Some("Steam is running; Steam Input may also write to this controller."),
            "ds4windows.exe" => Some("DS4Windows is running and may write controller effects."),
            "dsx.exe" | "dsx_pro.exe" => Some("DSX is running and may write controller effects."),
            "rewasd.exe" => Some("reWASD is running and may write controller effects."),
            "dualsensex.exe" => Some("DualSenseX is running and may write controller effects."),
            _ => None,
        };
        if let Some(tool) = tool
            && !state.conflicts.contains(&tool)
        {
            state.conflicts.push(tool);
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe {
        CloseHandle(snapshot);
    }
    state
}
