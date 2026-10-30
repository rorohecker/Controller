//! OS audio open/close calls belong to workers, never the HID reader.
use super::*;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

static OUTPUT_WORKERS: AtomicU32 = AtomicU32::new(0);
static CAPTURE_WORKERS: AtomicU32 = AtomicU32::new(0);
const START_TIMEOUT: Duration = Duration::from_secs(4);

struct Permit(&'static AtomicU32);
impl Permit {
    fn acquire(counter: &'static AtomicU32) -> Result<Self, String> {
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .map(|_| Self(counter))
            .map_err(|_| "Audio is still closing. Try again in a moment.".into())
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct OutputWorker {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    ready: mpsc::Receiver<Result<String, String>>,
    started: Instant,
    settled: bool,
}
impl OutputWorker {
    pub fn start(
        shared: Arc<Shared>,
        endpoint: Option<String>,
        retired: Vec<Arc<AtomicBool>>,
    ) -> Result<Self, String> {
        let permit = Permit::acquire(&OUTPUT_WORKERS)?;
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let completion = Completion(done.clone());
        let cancel = stop.clone();
        let (ready, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("controller-audio-output".into())
            .spawn(move || {
                let _completion = completion;
                let _permit = permit;
                // A quick replay waits for this engine's previous stream to close.
                // Waiting and OS teardown both remain outside the HID thread.
                if !wait_retired(&retired, &cancel) {
                    return;
                }
                #[cfg(windows)]
                let result =
                    run_exclusive_output(shared.clone(), cancel.clone(), ready.clone(), endpoint);
                #[cfg(not(windows))]
                let result =
                    run_portable_output(shared.clone(), cancel.clone(), ready.clone(), endpoint);
                if let Err(error) = result {
                    let _ = ready.try_send(Err(error.clone()));
                    if !cancel.load(Ordering::Acquire)
                        && let Ok(mut slot) = shared.error.lock()
                        && !cancel.load(Ordering::Acquire)
                    {
                        *slot = Some(format!("Controller audio stopped: {error}"));
                    }
                }
            })
            .map_err(|e| format!("Could not start audio worker: {e}"))?;
        Ok(Self {
            stop,
            done,
            ready: receiver,
            started: Instant::now(),
            settled: false,
        })
    }
    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
    pub fn completion(&self) -> Arc<AtomicBool> {
        self.done.clone()
    }
    pub fn poll(&mut self) -> Option<Result<String, String>> {
        if self.settled {
            return None;
        }
        // The deadline wins even if an endpoint completed just before this poll.
        let result = if self.started.elapsed() >= START_TIMEOUT {
            self.stop.store(true, Ordering::Release);
            Some(Err(
                "Controller audio did not open within four seconds.".into()
            ))
        } else {
            match self.ready.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Audio worker stopped during startup.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            }
        };
        if result.is_some() {
            self.settled = true;
        }
        result
    }
}
struct Completion(Arc<AtomicBool>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn wait_retired(retired: &[Arc<AtomicBool>], stop: &AtomicBool) -> bool {
    while retired.iter().any(|done| !done.load(Ordering::Acquire)) {
        if stop.load(Ordering::Acquire) {
            return false;
        }
        thread::sleep(Duration::from_millis(3));
    }
    !stop.load(Ordering::Acquire)
}
impl Drop for OutputWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

pub(super) struct CaptureWorker {
    stop: Arc<AtomicBool>,
    state: Arc<AtomicU32>,
    started: Instant,
}
impl CaptureWorker {
    pub fn start(
        shared: Arc<Shared>,
        source: Option<String>,
        sink_name: Option<String>,
        sink_id: Option<String>,
    ) -> Result<Self, String> {
        let permit = Permit::acquire(&CAPTURE_WORKERS)?;
        let generation = shared.capture_generation.load(Ordering::Acquire);
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let state = Arc::new(AtomicU32::new(0));
        let worker_state = state.clone();
        thread::Builder::new()
            .name("controller-audio-capture".into())
            .spawn(move || {
                let _permit = permit;
                match build_loopback(
                    shared.clone(),
                    source.as_deref(),
                    sink_name.as_deref(),
                    sink_id.as_deref(),
                ) {
                    Ok(stream) => {
                        // The stream is created and destroyed on its owner thread (also on macOS).
                        let _stream = stream;
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        worker_state.store(1, Ordering::Release);
                        while !cancel.load(Ordering::Acquire) {
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                    Err(error) => {
                        worker_state.store(2, Ordering::Release);
                        if !cancel.load(Ordering::Acquire)
                            && shared.capture_generation.load(Ordering::Acquire) == generation
                            && let Ok(mut slot) = shared.capture_error.lock()
                            && !cancel.load(Ordering::Acquire)
                            && shared.capture_generation.load(Ordering::Acquire) == generation
                        {
                            *slot = Some(error);
                        }
                    }
                }
            })
            .map_err(|e| format!("Could not start capture worker: {e}"))?;
        Ok(Self {
            stop,
            state,
            started: Instant::now(),
        })
    }
    pub fn state(&self) -> &'static str {
        match self.state.load(Ordering::Acquire) {
            1 => "running",
            2 => "failed",
            _ => "starting",
        }
    }
    pub fn timed_out(&self) -> bool {
        self.state() == "starting" && self.started.elapsed() >= START_TIMEOUT
    }
}
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_audio_poll_and_drop_never_wait_for_worker() {
        let stop = Arc::new(AtomicBool::new(false));
        let (_sender, ready) = mpsc::sync_channel(1);
        let mut worker = OutputWorker {
            done: Arc::new(AtomicBool::new(false)),
            stop: stop.clone(),
            ready,
            started: Instant::now(),
            settled: false,
        };
        for _ in 0..100 {
            assert!(worker.poll().is_none());
        }
        drop(worker);
        assert!(stop.load(Ordering::Acquire));
    }
    #[test]
    fn startup_deadline_cancels_late_success() {
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, ready) = mpsc::sync_channel(1);
        sender.send(Ok("late device".into())).unwrap();
        let mut worker = OutputWorker {
            done: Arc::new(AtomicBool::new(false)),
            stop: stop.clone(),
            ready,
            started: Instant::now() - START_TIMEOUT,
            settled: false,
        };
        assert!(worker.poll().unwrap().is_err());
        assert!(stop.load(Ordering::Acquire));
        assert!(worker.poll().is_none());
    }
    #[test]
    fn cancel_while_waiting_for_old_stream_close_does_not_wait_or_open() {
        let old = Arc::new(AtomicBool::new(false));
        assert!(!wait_retired(
            std::slice::from_ref(&old),
            &AtomicBool::new(true)
        ));
        old.store(true, Ordering::Release);
        assert!(wait_retired(&[old], &AtomicBool::new(false)));
    }
    #[test]
    fn stuck_workers_have_a_hard_limit_and_release_their_slot() {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let first = Permit::acquire(&COUNT).unwrap();
        let second = Permit::acquire(&COUNT).unwrap();
        assert!(Permit::acquire(&COUNT).is_err());
        drop(first);
        drop(second);
        assert_eq!(COUNT.load(Ordering::Acquire), 0);
    }
}
