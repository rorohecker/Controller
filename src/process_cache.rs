//! One discovery worker; readers only load an immutable snapshot.
use super::{ProcessState, inspect_now};
use arc_swap::ArcSwap;
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Snapshot {
    state: ProcessState,
    sampled: Option<Instant>,
    duration_us: u64,
}
fn current() -> Arc<Snapshot> {
    static STATUS: OnceLock<Arc<ArcSwap<Snapshot>>> = OnceLock::new();
    STATUS
        .get_or_init(|| {
            let status = Arc::new(ArcSwap::from_pointee(Snapshot::default()));
            let worker = status.clone();
            let _ = std::thread::Builder::new()
                .name("controller-process-discovery".into())
                .spawn(move || {
                    loop {
                        let started = Instant::now();
                        let state = inspect_now();
                        worker.store(Arc::new(Snapshot {
                            state,
                            sampled: Some(Instant::now()),
                            duration_us: started.elapsed().as_micros() as u64,
                        }));
                        #[cfg(windows)]
                        let period = Duration::from_secs(2);
                        #[cfg(not(windows))]
                        let period = Duration::from_secs(2);
                        std::thread::sleep(period);
                    }
                });
            status
        })
        .load_full()
}
pub(super) fn inspect() -> ProcessState {
    let snapshot = current();
    if snapshot
        .sampled
        .is_none_or(|at| at.elapsed() > Duration::from_secs(6))
    {
        ProcessState::default()
    } else {
        snapshot.state.clone()
    }
}
pub(super) fn diagnostics() -> serde_json::Value {
    let snapshot = current();
    serde_json::json!({"ready":snapshot.sampled.is_some(),"durationUs":snapshot.duration_us,
        "ageMs":snapshot.sampled.map(|at|at.elapsed().as_millis() as u64),"workerLimit":1,"stale":snapshot.sampled.is_none_or(|at|at.elapsed()>Duration::from_secs(6))})
}
