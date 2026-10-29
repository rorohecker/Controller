//! WASAPI process-tree capture. No microphone, output-file recording or implicit system fallback.
use super::super::{CaptureDsp, Shared, push_capture_frame};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};
use std::time::Duration;
use windows::Win32::Media::Audio::{AUDCLNT_BUFFERFLAGS_SILENT, IAudioCaptureClient};
static WORKERS: AtomicUsize = AtomicUsize::new(0);
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
            _ => "stopped",
        }
    }
    pub(in crate::audio_haptics) fn start(shared: Arc<Shared>, pid: u32) -> Result<Self, String> {
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                (v < 2).then_some(v + 1)
            })
            .map_err(|_| {
                "Previous process capture is still stopping. Try again shortly.".to_string()
            })?;
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let state = Arc::new(AtomicU8::new(0));
        let status = state.clone();
        let generation = shared.capture_generation.load(Ordering::Acquire);
        let result = std::thread::Builder::new()
            .name("game-audio-capture".into())
            .spawn(move || {
                let _count = WorkerCount;
                if let Err(error) = capture(&shared, pid, &signal, &status, generation)
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
    let result = (|| {
        while !stop.load(Ordering::Acquire) {
            if unsafe { WaitForSingleObject(process.0, 0) } != WAIT_TIMEOUT {
                return Err("Selected process ended or is unavailable; capture stopped. Select it again to restart.".into());
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
                    dsp.gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));
                    dsp.set_preset(shared.preset.load(Ordering::Relaxed));
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
                        push_capture_frame(shared, &mut dsp, frame);
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
        let worker = ProcessCapture::start(shared.clone(), std::process::id()).unwrap();
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
