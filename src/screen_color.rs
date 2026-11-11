//! Low-rate, opt-in desktop color sampling. GDI work stays off the HID input thread.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::{EnumDisplayMonitors, GetDC, GetPixel, HDC, HMONITOR, ReleaseDC},
    UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
};

/// Desktop rectangle `(left, top, width, height)` of each monitor, primary first.
pub fn monitors() -> Vec<(i32, i32, i32, i32)> {
    unsafe extern "system" fn collect(_: HMONITOR, _: HDC, rect: *mut RECT, data: LPARAM) -> i32 {
        // SAFETY: `data` is the address of the Vec passed below and `rect` is valid per the API.
        let list = unsafe { &mut *(data as *mut Vec<(i32, i32, i32, i32)>) };
        let r = unsafe { *rect };
        list.push((r.left, r.top, r.right - r.left, r.bottom - r.top));
        1
    }
    let mut list: Vec<(i32, i32, i32, i32)> = Vec::new();
    // SAFETY: the callback only runs during this call and `list` outlives it.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut list as *mut _ as LPARAM,
        );
    }
    list.retain(|m| m.2 > 0 && m.3 > 0);
    // The primary monitor owns the desktop origin; keep it first so index 0 stays "primary".
    list.sort_by_key(|m| (!(m.0 == 0 && m.1 == 0), m.0, m.1));
    list
}

type Rect = (i32, i32, i32, i32);
const MONITOR_REFRESH: Duration = Duration::from_secs(3);
static MONITOR_CACHE: Mutex<Option<(Instant, Vec<Rect>)>> = Mutex::new(None);
static FALLBACK_REPORTED: AtomicBool = AtomicBool::new(false);

/// `monitors()` re-enumerated at most every few seconds; the sample tick runs every 400 ms.
fn cached_monitors() -> Vec<Rect> {
    let mut cache = MONITOR_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, list)) = cache.as_ref()
        && at.elapsed() < MONITOR_REFRESH
    {
        return list.clone();
    }
    let list = monitors();
    *cache = Some((Instant::now(), list.clone()));
    list
}

/// Rectangle of monitor `index`; `None` when it is not (or no longer) connected.
fn pick_monitor(list: &[Rect], index: u32) -> Option<Rect> {
    list.get(index as usize).copied()
}

struct Shared {
    enabled: AtomicBool,
    stop: AtomicBool,
    generation: AtomicU64,
    /// 0 is the primary monitor; higher numbers follow `monitors()` order.
    monitor: AtomicU32,
    latest: Mutex<Option<Result<[u8; 3], String>>>,
}

pub struct ScreenSampler {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl ScreenSampler {
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            enabled: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            monitor: AtomicU32::new(0),
            latest: Mutex::new(None),
        });
        let worker_shared = shared.clone();
        let worker = thread::spawn(move || {
            while !worker_shared.stop.load(Ordering::Relaxed) {
                if worker_shared.enabled.load(Ordering::Relaxed) {
                    let generation = worker_shared.generation.load(Ordering::Relaxed);
                    let result = sample_screen(worker_shared.monitor.load(Ordering::Relaxed));
                    if worker_shared.enabled.load(Ordering::Relaxed)
                        && worker_shared.generation.load(Ordering::Relaxed) == generation
                        && let Ok(mut latest) = worker_shared.latest.lock()
                    {
                        *latest = Some(result);
                    }
                    thread::sleep(Duration::from_millis(400));
                } else {
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });
        Self {
            shared,
            worker: Some(worker),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
        self.shared.enabled.store(enabled, Ordering::Relaxed);
        if let Ok(mut latest) = self.shared.latest.lock() {
            *latest = None;
        }
    }

    /// Chooses which monitor is sampled (0 = primary). Out-of-range indexes fall back to primary.
    #[allow(dead_code)]
    pub fn set_monitor(&self, index: u32) {
        if self.shared.monitor.swap(index, Ordering::Relaxed) != index {
            self.shared.generation.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut latest) = self.shared.latest.lock() {
                *latest = None;
            }
        }
    }

    pub fn take(&self) -> Option<Result<[u8; 3], String>> {
        self.shared.latest.lock().ok()?.take()
    }
}

impl Drop for ScreenSampler {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[allow(dead_code)]
fn sample_primary_screen() -> Result<[u8; 3], String> {
    sample_screen(0)
}

fn sample_screen(monitor: u32) -> Result<[u8; 3], String> {
    // SAFETY: The returned desktop DC is released on every path below. Read-only GDI calls
    // use coordinates inside the chosen monitor's bounds.
    unsafe {
        let (left, top, width, height) = if monitor == 0 {
            (
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        } else {
            match pick_monitor(&cached_monitors(), monitor) {
                Some(rect) => {
                    FALLBACK_REPORTED.store(false, Ordering::Relaxed);
                    rect
                }
                None => {
                    if !FALLBACK_REPORTED.swap(true, Ordering::Relaxed) {
                        crate::app_log::warn(&format!(
                            "Screen color monitor {monitor} is not connected; sampling the primary display."
                        ));
                    }
                    (
                        0,
                        0,
                        GetSystemMetrics(SM_CXSCREEN),
                        GetSystemMetrics(SM_CYSCREEN),
                    )
                }
            }
        };
        if width <= 0 || height <= 0 {
            return Err("Primary display is unavailable for screen color.".into());
        }
        let dc = GetDC(std::ptr::null_mut());
        if dc.is_null() {
            return Err("Windows could not sample the primary display.".into());
        }
        let mut colors = Vec::with_capacity(15);
        for row in 0..3 {
            for col in 0..5 {
                let x = left + (width * (col * 2 + 1) / 10).clamp(0, width - 1);
                let y = top + (height * (row * 2 + 1) / 6).clamp(0, height - 1);
                let pixel = GetPixel(dc, x, y);
                if pixel != 0xffff_ffff {
                    colors.push([
                        (pixel & 0xff) as u8,
                        ((pixel >> 8) & 0xff) as u8,
                        ((pixel >> 16) & 0xff) as u8,
                    ]);
                }
            }
        }
        ReleaseDC(std::ptr::null_mut(), dc);
        mix_colors(&colors)
            .ok_or_else(|| "The primary display did not return readable pixels.".into())
    }
}

fn mix_colors(colors: &[[u8; 3]]) -> Option<[u8; 3]> {
    if colors.is_empty() {
        return None;
    }
    let mut channels = [0u32; 3];
    let mut total = 0u32;
    for color in colors {
        let chroma = color.iter().max()? - color.iter().min()?;
        let weight = 1 + u32::from(chroma) / 32;
        for (sum, value) in channels.iter_mut().zip(color) {
            *sum += u32::from(*value) * weight;
        }
        total += weight;
    }
    let mut mixed = channels.map(|value| (value / total) as u8);
    let peak = *mixed.iter().max()?;
    if peak > 180 {
        for value in &mut mixed {
            *value = ((u16::from(*value) * 180) / u16::from(peak)) as u8;
        }
    }
    Some(mixed)
}

pub fn smooth(previous: Option<[u8; 3]>, current: [u8; 3]) -> [u8; 3] {
    let Some(previous) = previous else {
        return current;
    };
    std::array::from_fn(|index| {
        ((u16::from(previous[index]) * 3 + u16::from(current[index])) / 4) as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_palette_prefers_color_and_fades_changes() {
        assert_eq!(mix_colors(&[]), None);
        let mixed = mix_colors(&[[255, 255, 255], [200, 0, 0]]).unwrap();
        assert!(mixed[0] > mixed[1]);
        assert!(mixed.iter().all(|channel| *channel <= 180));
        assert_eq!(smooth(Some([0, 0, 0]), [200, 100, 40]), [50, 25, 10]);
    }

    #[test]
    fn missing_monitor_is_none_instead_of_panicking() {
        let list = [(0, 0, 1920, 1080), (1920, 0, 1280, 720)];
        assert_eq!(pick_monitor(&list, 1), Some((1920, 0, 1280, 720)));
        assert_eq!(pick_monitor(&list, 2), None);
        assert_eq!(pick_monitor(&[], 0), None);
    }

    #[test]
    #[ignore = "needs an interactive Windows desktop"]
    fn samples_primary_screen() {
        assert!(sample_primary_screen().is_ok());
        let sampler = ScreenSampler::new();
        sampler.set_enabled(true);
        let mut sampled = false;
        for _ in 0..20 {
            if let Some(Ok(_)) = sampler.take() {
                sampled = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(sampled);
        sampler.set_enabled(false);
    }
}
