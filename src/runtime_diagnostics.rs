//! Software service times only; these are not controller-to-screen latency.
use serde_json::{Value, json};
use std::time::Instant;

const WINDOW: usize = 512;
pub struct Rolling {
    samples: [u64; WINDOW],
    cursor: usize,
    count: usize,
}
impl Default for Rolling {
    fn default() -> Self {
        Self {
            samples: [0; WINDOW],
            cursor: 0,
            count: 0,
        }
    }
}
impl Rolling {
    pub fn record(&mut self, us: u64) {
        self.samples[self.cursor] = us;
        self.cursor = (self.cursor + 1) % WINDOW;
        self.count = (self.count + 1).min(WINDOW);
    }
    pub fn report(&self) -> Value {
        if self.count == 0 {
            return json!({"count":0,"p50Us":null,"p95Us":null,"maxUs":null});
        }
        let mut samples = self.samples;
        let values = &mut samples[..self.count];
        values.sort_unstable();
        json!({"count":self.count,"p50Us":values[(self.count-1)/2],"p95Us":values[(self.count*95).div_ceil(100)-1],"maxUs":values[self.count-1]})
    }
}
#[derive(Default)]
pub struct Metrics {
    pub read_wait: Rolling,
    pub decode: Rolling,
    pub dispatch: Rolling,
    pub loop_period: Rolling,
}
impl Metrics {
    pub fn report(&self) -> Value {
        json!({"readWait":self.read_wait.report(),"decode":self.decode.report(),"dispatch":self.dispatch.report(),"loopPeriod":self.loop_period.report()})
    }
}
pub fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

pub fn resources() -> Value {
    let (cpu_us, rss_bytes) = resources_native();
    static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let sampled = ORIGIN.get_or_init(Instant::now).elapsed().as_micros() as u64;
    json!({"sampledAtUs":sampled,"pid":std::process::id(),"cpuTimeUs":cpu_us,"residentBytes":rss_bytes})
}
#[cfg(windows)]
fn resources_native() -> (Option<u64>, Option<u64>) {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{GetCurrentProcess, GetProcessTimes},
        },
    };
    let mut created: FILETIME = unsafe { std::mem::zeroed() };
    let mut ended = created;
    let mut kernel = created;
    let mut user = created;
    let process = unsafe { GetCurrentProcess() };
    let ok =
        unsafe { GetProcessTimes(process, &mut created, &mut ended, &mut kernel, &mut user) } != 0;
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    let cpu = ok.then(|| (ticks(kernel) + ticks(user)) / 10);
    let mut memory: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    memory.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    let rss = (unsafe { K32GetProcessMemoryInfo(process, &mut memory, memory.cb) } != 0)
        .then_some(memory.WorkingSetSize as u64);
    (cpu, rss)
}
#[cfg(unix)]
fn resources_native() -> (Option<u64>, Option<u64>) {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let cpu = (unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0).then(|| {
        let us = |t: libc::timeval| {
            (t.tv_sec as u64)
                .saturating_mul(1_000_000)
                .saturating_add(t.tv_usec as u64)
        };
        us(usage.ru_utime).saturating_add(us(usage.ru_stime))
    });
    #[cfg(target_os = "linux")]
    let rss = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .and_then(|pages| {
            let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            u64::try_from(size)
                .ok()
                .and_then(|size| pages.checked_mul(size))
        });
    #[cfg(not(target_os = "linux"))]
    let rss = None;
    (cpu, rss)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_window_quantiles_ignore_overwritten_samples() {
        let mut stats = Rolling::default();
        assert!(stats.report()["p50Us"].is_null());
        stats.record(u64::MAX);
        for n in 1..=512 {
            stats.record(n);
        }
        let report = stats.report();
        assert_eq!(report["count"], 512);
        assert_eq!(report["p50Us"], 256);
        assert_eq!(report["p95Us"], 487);
        assert_eq!(report["maxUs"], 512);
    }
    #[test]
    fn process_resources_have_no_child_or_system_wide_claims() {
        let report = resources();
        assert_eq!(report["pid"], std::process::id());
        assert!(report["cpuTimeUs"].as_u64().is_some());
    }
}
