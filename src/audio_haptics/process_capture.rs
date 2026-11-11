//! WASAPI process-tree capture. No microphone, output-file recording or implicit system fallback.
use super::super::{CaptureDsp, Shared, push_capture_frame};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};
use std::time::Duration;
use windows::Win32::Media::Audio::{AUDCLNT_BUFFERFLAGS_SILENT, IAudioCaptureClient};
static WORKERS: AtomicUsize = AtomicUsize::new(0);
/// Error text for a vanished process; `run` matches it to decide on re-attaching.
const ENDED: &str =
    "Selected process ended or is unavailable; capture stopped. Select it again to restart.";
const REATTACH_POLLS: u32 = 60; // 30 s at 500 ms
pub(in crate::audio_haptics) struct ProcessCapture {
    stop: Arc<AtomicBool>,
    state: Arc<AtomicU8>,
    started: std::time::Instant,
}
impl Drop for ProcessCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
struct WorkerCount;
impl Drop for WorkerCount {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

impl ProcessCapture {
    pub(in crate::audio_haptics) fn timed_out(&self) -> bool {
        self.state.load(Ordering::Acquire) == 0 && self.started.elapsed() >= Duration::from_secs(4)
    }
    pub(in crate::audio_haptics) fn state(&self) -> &'static str {
        match self.state.load(Ordering::Acquire) {
            0 => "starting",
            1 => "running",
            3 => "reattaching",
            _ => "stopped",
        }
    }
    /// With `reattach`, a captured executable that exits and starts again is
    /// picked up automatically (polled for up to 30 seconds) instead of ending capture.
    pub(in crate::audio_haptics) fn start(
        shared: Arc<Shared>,
        pid: u32,
        reattach: bool,
    ) -> Result<Self, String> {
        let mut workers = WORKERS.load(Ordering::Acquire);
        loop {
            if workers >= 2 {
                return Err(
                    "Previous process capture is still stopping. Try again shortly.".into(),
                );
            }
            match WORKERS.compare_exchange_weak(
                workers,
                workers + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => workers = actual,
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let state = Arc::new(AtomicU8::new(0));
        let status = state.clone();
        let generation = shared.capture_generation.load(Ordering::Acquire);
        let result = std::thread::Builder::new()
            .name("game-audio-capture".into())
            .spawn(move || {
                let _count = WorkerCount;
                if let Err(error) = run(&shared, pid, &signal, &status, generation, reattach)
                    && !signal.load(Ordering::Acquire)
                    && let Ok(mut slot) = shared.capture_error.lock()
                    && !signal.load(Ordering::Acquire)
                    && shared.capture_generation.load(Ordering::Acquire) == generation
                {
                    *slot = Some(error);
                }
                status.store(2, Ordering::Release);
            });
        if let Err(error) = result {
            WORKERS.fetch_sub(1, Ordering::AcqRel);
            return Err(error.to_string());
        }
        Ok(Self {
            stop,
            state,
            started: std::time::Instant::now(),
        })
    }
}
/// (pid, parent pid, executable name) for every running process.
fn process_list() -> Vec<(u32, u32, String)> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    let mut list = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return list;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let length = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            list.push((
                entry.th32ProcessID,
                entry.th32ParentProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..length]),
            ));
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    list
}
fn process_name(pid: u32) -> Option<String> {
    process_list()
        .into_iter()
        .find(|(id, ..)| *id == pid)
        .map(|(.., name)| name)
}
/// The root of a restarted process tree: same executable, parent not the same executable.
fn restarted_pid(list: &[(u32, u32, String)], name: &str, old: u32) -> Option<u32> {
    let same = |n: &str| n.eq_ignore_ascii_case(name);
    let candidates: Vec<&(u32, u32, String)> = list
        .iter()
        .filter(|(pid, _, n)| *pid != old && *pid > 4 && same(n))
        .collect();
    candidates
        .iter()
        .find(|(_, parent, _)| !candidates.iter().any(|(pid, ..)| pid == parent))
        .or(candidates.first())
        .map(|(pid, ..)| *pid)
}
fn run(
    shared: &Arc<Shared>,
    pid: u32,
    stop: &AtomicBool,
    status: &AtomicU8,
    generation: u64,
    reattach: bool,
) -> Result<(), String> {
    let name = process_name(pid);
    let mut current = pid;
    loop {
        shared.set_capture_info(
            generation,
            Some(("Process audio (WASAPI loopback)".into(), 48_000)),
            Some(match &name {
                Some(name) => {
                    format!("Capturing {name} (process {current}) and its child processes")
                }
                None => format!("Capturing process {current} and its child processes"),
            }),
        );
        let result = capture(shared, current, stop, status, generation);
        let restart = match (&result, &name) {
            (Err(error), Some(name))
                if reattach && error == ENDED && !stop.load(Ordering::Acquire) =>
            {
                Some(name.clone())
            }
            _ => None,
        };
        let Some(name) = restart else {
            return result;
        };
        status.store(3, Ordering::Release);
        shared.set_capture_info(
            generation,
            None,
            Some(format!("Waiting for {name} to start again")),
        );
        let mut next = None;
        for _ in 0..REATTACH_POLLS {
            if stop.load(Ordering::Acquire)
                || shared.capture_generation.load(Ordering::Acquire) != generation
            {
                return Ok(());
            }
            next = restarted_pid(&process_list(), &name, current);
            if next.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        match next {
            Some(pid) => current = pid,
            None => {
                return Err(format!(
                    "{name} did not start again within 30 seconds; capture stopped."
                ));
            }
        }
    }
}
fn capture(
    shared: &Arc<Shared>,
    pid: u32,
    stop: &AtomicBool,
    status: &AtomicU8,
    generation: u64,
) -> Result<(), String> {
    use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_TIMEOUT},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        },
    };
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return Err("The selected process exited or cannot be queried. Choose it again.".into());
    }
    struct ProcessHandle(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
    let process = ProcessHandle(handle);
    wasapi::initialize_mta().ok().map_err(|e| e.to_string())?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            wasapi::deinitialize();
        }
    }
    let _com = Com;
    let mut client = AudioClient::new_application_loopback_client(pid, true).map_err(|e| {
        format!("Process audio capture unavailable: {e}. Use System audio explicitly if needed.")
    })?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
    client
        .initialize_client(
            &format,
            &Direction::Capture,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 200_000,
            },
        )
        .map_err(|e| e.to_string())?;
    let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
    // Use the raw service for correct SILENT/null-buffer handling. The wrapper's
    // raw-byte reader copies unconditionally and cannot safely consume silence.
    let reader: IAudioCaptureClient =
        unsafe { client.as_iaudioclient().GetService() }.map_err(|e| e.to_string())?;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    client.start_stream().map_err(|e| e.to_string())?;
    status.store(1, Ordering::Release);
    let mut dsp = CaptureDsp::new(48_000, 0.5);
    dsp.generation = generation;
    let result = (|| {
        while !stop.load(Ordering::Acquire) {
            if unsafe { WaitForSingleObject(process.0, 0) } != WAIT_TIMEOUT {
                return Err(ENDED.into());
            }
            for _ in 0..32 {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if unsafe { reader.GetNextPacketSize() }.map_err(|e| e.to_string())? == 0 {
                    break;
                }
                let mut ptr = std::ptr::null_mut();
                let (mut frames, mut flags) = (0, 0);
                unsafe { reader.GetBuffer(&mut ptr, &mut frames, &mut flags, None, None) }
                    .map_err(|e| e.to_string())?;
                if frames == 0 {
                    let _ = unsafe { reader.ReleaseBuffer(0) };
                    break;
                }
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                let valid = frames <= 48_000 && (silent || !ptr.is_null());
                if valid {
                    dsp.apply_settings(shared);
                    let (mut peak, mut square_sum) = (0.0f32, 0.0f32);
                    for i in 0..frames as usize {
                        if stop.load(Ordering::Acquire)
                            || shared.capture_generation.load(Ordering::Acquire) != generation
                        {
                            break;
                        }
                        let frame = if silent {
                            [0.0; 2]
                        } else {
                            unsafe {
                                [
                                    std::ptr::read_unaligned(ptr.add(i * 8).cast::<f32>()),
                                    std::ptr::read_unaligned(ptr.add(i * 8 + 4).cast::<f32>()),
                                ]
                            }
                        };
                        for sample in frame {
                            if sample.is_finite() {
                                peak = peak.max(sample.abs().min(1.0));
                                square_sum += sample.clamp(-1.0, 1.0).powi(2);
                            }
                        }
                        push_capture_frame(shared, &mut dsp, frame);
                    }
                    if frames > 0 {
                        shared.record_capture_input(
                            peak,
                            (square_sum / (frames as f32 * 2.0)).sqrt(),
                        );
                    }
                }
                unsafe { reader.ReleaseBuffer(frames) }.map_err(|e| e.to_string())?;
                if !valid {
                    return Err(
                        "Process capture returned an invalid or oversized audio packet".into(),
                    );
                }
            }
            let _ = event.wait_for_event(30);
        }
        Ok(())
    })();
    let _ = client.stop_stream();
    std::thread::sleep(Duration::from_millis(1));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restarted_process_prefers_the_new_tree_root() {
        let list = vec![
            (10, 1, "game.exe".to_string()),
            (20, 1, "Game.EXE".to_string()),
            (21, 20, "game.exe".to_string()),
            (30, 1, "other.exe".to_string()),
        ];
        assert_eq!(restarted_pid(&list, "game.exe", 10), Some(20));
        assert_eq!(restarted_pid(&list, "game.exe", 20), Some(10));
        assert_eq!(restarted_pid(&list, "missing.exe", 10), None);
        assert_eq!(restarted_pid(&[], "game.exe", 10), None);
    }
    #[test]
    fn activation_timeout_only_applies_while_starting() {
        let worker = ProcessCapture {
            stop: Arc::new(AtomicBool::new(false)),
            state: Arc::new(AtomicU8::new(0)),
            started: std::time::Instant::now() - Duration::from_secs(5),
        };
        assert!(worker.timed_out());
        worker.state.store(1, Ordering::Release);
        assert!(!worker.timed_out());
        worker.state.store(2, Ordering::Release);
        assert!(!worker.timed_out());
    }
    #[test]
    #[ignore = "Requires a running Windows audio service and process-loopback support; no controller or sound output"]
    fn process_loopback_starts_without_controller() {
        let shared = Arc::new(Shared::default());
        let worker = ProcessCapture::start(shared.clone(), std::process::id(), false).unwrap();
        let state = worker.state.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        while worker.state() == "starting" && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            worker.state(),
            "running",
            "{:?}",
            shared.capture_error.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(250));
        assert!(shared.capture_error.lock().unwrap().is_none());
        drop(worker);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while state.load(Ordering::Acquire) != 2 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.load(Ordering::Acquire), 2);
    }
}
