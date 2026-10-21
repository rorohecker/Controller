use std::mem::size_of;

use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
};

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
    let (foreground, window) = unsafe {
        let window = GetForegroundWindow();
        let mut pid = 0;
        if !window.is_null() {
            GetWindowThreadProcessId(window, &mut pid);
        }
        (pid, window)
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
        if entry.th32ProcessID == foreground
            && !matches!(exe.as_str(), "controller.exe" | "aftertouch.exe")
        {
            state.foreground_exe = Some(exe.clone());
            let mut title = [0u16; 512];
            let title_len =
                unsafe { GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32) };
            if title_len > 0 {
                state.foreground_title =
                    Some(String::from_utf16_lossy(&title[..title_len as usize]));
            }
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, foreground) };
            if !process.is_null() {
                let mut path = [0u16; 32768];
                let mut size = path.len() as u32;
                if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size) }
                    != 0
                {
                    state.foreground_path = Some(String::from_utf16_lossy(&path[..size as usize]));
                }
                unsafe { CloseHandle(process) };
            }
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
