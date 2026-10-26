pub struct ProcessState {
    pub conflicts: Vec<&'static str>,
    pub foreground_exe: Option<String>,
    pub foreground_title: Option<String>,
    pub foreground_path: Option<String>,
}

pub fn gui_window_focused(_gui_pid: u32) -> bool {
    true
}

pub fn inspect() -> ProcessState {
    ProcessState {
        conflicts: Vec::new(),
        foreground_exe: None,
        foreground_title: None,
        foreground_path: None,
    }
}
