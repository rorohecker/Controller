//! ScreenCaptureKit / desktop-portal capture. Frames never leave memory.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
#[path = "screen_pixels.rs"]
mod pixels;
struct Shared {
    enabled: AtomicBool,
    stop: AtomicBool,
    generation: AtomicU64,
    latest: Mutex<Option<Result<[u8; 3], String>>>,
}
pub struct ScreenSampler {
    shared: Arc<Shared>,
}

impl ScreenSampler {
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            enabled: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            latest: Mutex::new(None),
        });
        let worker = shared.clone();
        if let Err(e) = thread::Builder::new().name("screen-color-capture".into()).spawn(move || {
            while !worker.stop.load(Ordering::Acquire) {
                if !worker.enabled.load(Ordering::Acquire) { thread::sleep(Duration::from_millis(80)); continue; }
                let generation = worker.generation.load(Ordering::Acquire);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture_session(&worker, generation)))
                    .unwrap_or_else(|_| Err("Screen capture could not start. Check screen-sharing permission and the desktop portal, then try again.".into()));
                if let Err(e) = result { publish(&worker, generation, Err(e)); }
                // A denied or failed portal stays closed until the user retries.
                while current(&worker, generation) { thread::sleep(Duration::from_millis(80)); }
            }
        }) { *shared.latest.lock().unwrap() = Some(Err(e.to_string())); }
        Self { shared }
    }
    pub fn set_enabled(&self, enabled: bool) {
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        self.shared.enabled.store(enabled, Ordering::Release);
        if let Ok(mut latest) = self.shared.latest.lock() {
            *latest = None;
        }
    }
    pub fn take(&self) -> Option<Result<[u8; 3], String>> {
        self.shared.latest.lock().ok()?.take()
    }
}
impl Drop for ScreenSampler {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.enabled.store(false, Ordering::Release);
    }
}
fn current(shared: &Shared, generation: u64) -> bool {
    !shared.stop.load(Ordering::Acquire)
        && shared.enabled.load(Ordering::Acquire)
        && shared.generation.load(Ordering::Acquire) == generation
}
fn publish(shared: &Shared, generation: u64, result: Result<[u8; 3], String>) {
    if let Ok(mut latest) = shared.latest.lock()
        && current(shared, generation)
    {
        *latest = Some(result);
    }
}
fn capture_session(shared: &Shared, generation: u64) -> Result<(), String> {
    use scap::{
        capturer::{Options, Resolution, engine::Engine},
        frame::{Frame, FrameType, VideoFrame},
    };
    if !scap::is_supported() {
        return Err("Screen capture is unavailable in this desktop session.".into());
    }
    if !scap::has_permission() {
        scap::request_permission();
        return Err("Allow Screen Recording in macOS Privacy & Security, then restart CONTROLLER and enable screen color again.".into());
    }
    let options = Options {
        fps: 4,
        show_cursor: false,
        show_highlight: false,
        output_type: FrameType::BGR0,
        output_resolution: Resolution::_480p,
        captures_audio: false,
        ..Options::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut engine = Engine::new(&options, tx);
    if !current(shared, generation) {
        return Ok(());
    }
    engine.start();
    let mut last_frame = Instant::now();
    let result = (|| {
        while current(shared, generation) {
            let item =
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(item) => item,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        if last_frame.elapsed() < Duration::from_secs(10) =>
                    {
                        continue;
                    }
                    Err(_) => return Err(
                        "Screen sharing stopped. Select a display again to resume screen color."
                            .into(),
                    ),
                };
            let Some(Frame::Video(frame)) = engine.process_channel_item(item) else {
                continue;
            };
            let color = match frame {
                VideoFrame::RGB(f) => pixels::average(&f.data, f.width, f.height, 3, [0, 1, 2]),
                VideoFrame::RGBx(f) => pixels::average(&f.data, f.width, f.height, 4, [0, 1, 2]),
                VideoFrame::XBGR(f) => pixels::average(&f.data, f.width, f.height, 4, [3, 2, 1]),
                VideoFrame::BGRx(f) => pixels::average(&f.data, f.width, f.height, 4, [2, 1, 0]),
                VideoFrame::BGR0(f) => pixels::average(&f.data, f.width, f.height, 4, [2, 1, 0]),
                VideoFrame::BGRA(f) => pixels::average(&f.data, f.width, f.height, 4, [2, 1, 0]),
                VideoFrame::YUVFrame(_) => {
                    Err("The display returned an unsupported pixel format.".into())
                }
            }?;
            last_frame = Instant::now();
            publish(shared, generation, Ok(color));
        }
        Ok(())
    })();
    engine.stop();
    result
}

pub fn smooth(previous: Option<[u8; 3]>, current: [u8; 3]) -> [u8; 3] {
    let Some(previous) = previous else {
        return current;
    };
    std::array::from_fn(|index| {
        ((u16::from(previous[index]) * 3 + u16::from(current[index])) / 4) as u8
    })
}
