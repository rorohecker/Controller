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
                    let mut unchanged = 0u32;
                    loop {
                        let started = Instant::now();
                        let state = inspect_now();
                        if state == worker.load().state {
                            unchanged = unchanged.saturating_add(1);
                        } else {
                            unchanged = 0;
                        }
                        worker.store(Arc::new(Snapshot {
                            state,
                            sampled: Some(Instant::now()),
                            duration_us: started.elapsed().as_micros() as u64,
                        }));
                        // Windows polls are cheap. Portable platforms spawn osascript/xprop, so back
                        // off once nothing has changed for a while. The 4 s ceiling stays below the
                        // 6 s staleness limit in `inspect`.
                        #[cfg(windows)]
                        let period = Duration::from_secs(2);
                        #[cfg(not(windows))]
                        let period = if unchanged >= 5 {
                            Duration::from_secs(4)
                        } else {
                            Duration::from_secs(2)
                        };
                        let _ = unchanged;
                        // The sample's age is the sleep plus the next inspection, so the sleep
                        // only covers what the inspection did not already spend.
                        std::thread::sleep(sleep_for(period, started.elapsed()));
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

/// Remaining sleep so the sampling period is measured start to start, never below 100 ms.
fn sleep_for(period: Duration, elapsed: Duration) -> Duration {
    period
        .saturating_sub(elapsed)
        .max(Duration::from_millis(100))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_inspections_shorten_the_sleep() {
        let p = Duration::from_secs(4);
        assert_eq!(sleep_for(p, Duration::from_secs(1)), Duration::from_secs(3));
        assert_eq!(
            sleep_for(p, Duration::from_secs(9)),
            Duration::from_millis(100)
        );
    }
}
