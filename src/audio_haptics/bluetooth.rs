//! Experimental 3 kHz stereo grip PCM. Only the device owner writes HID.
use super::*;
use std::time::Instant;
const PERIOD: Duration = Duration::from_nanos(10_666_667);
const MAX_AGE: Duration = Duration::from_millis(32);
static WORKERS: AtomicU32 = AtomicU32::new(0);
struct Packet {
    created: Instant,
    pcm: [i8; 64],
}
pub(super) struct Output {
    queue: Arc<ArrayQueue<Packet>>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    next: Instant,
    pub dropped: Arc<AtomicU64>,
    pub underruns: u64,
}
struct Completion(Arc<AtomicBool>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Output {
    pub fn start(shared: Arc<Shared>) -> Result<Self, String> {
        let mut count = WORKERS.load(Ordering::Acquire);
        loop {
            if count >= 2 {
                return Err("Bluetooth audio is still stopping. Try again in a moment.".into());
            }
            match WORKERS.compare_exchange_weak(
                count,
                count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(n) => count = n,
            }
        }
        let queue = Arc::new(ArrayQueue::new(4));
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let (ring, cancel, complete, loss) =
            (queue.clone(), stop.clone(), done.clone(), dropped.clone());
        let result = thread::Builder::new()
            .name("bluetooth-grip-renderer".into())
            .spawn(move || {
                let _done = Completion(complete);
                let mut position = 0;
                let mut state = RenderState::default();
                let mut converter = Converter::default();
                let mut data = [0.0; 512 * 4];
                let mut next = Instant::now();
                while !cancel.load(Ordering::Acquire) {
                    let now = Instant::now();
                    if now < next {
                        thread::sleep((next - now).min(Duration::from_millis(2)));
                        continue;
                    }
                    render_output(&mut data, &shared, &mut position, &mut state);
                    let packet = Packet {
                        created: Instant::now(),
                        pcm: converter.convert(&data),
                    };
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    if ring.force_push(packet).is_some() {
                        loss.fetch_add(1, Ordering::Relaxed);
                    }
                    // Skip missed deadlines instead of emitting catch-up bursts.
                    next += PERIOD;
                    if next <= Instant::now() {
                        next = Instant::now() + PERIOD;
                    }
                }
            });
        if let Err(e) = result {
            WORKERS.fetch_sub(1, Ordering::AcqRel);
            return Err(e.to_string());
        }
        Ok(Self {
            queue,
            stop,
            done,
            next: Instant::now(),
            dropped,
            underruns: 0,
        })
    }
    pub fn take(&mut self, now: Instant) -> Result<Option<[i8; 64]>, String> {
        if self.done.load(Ordering::Acquire) {
            return Err("Bluetooth grip renderer stopped. Enable it again to retry.".into());
        }
        if now < self.next {
            return Ok(None);
        }
        self.next = if now.saturating_duration_since(self.next) > PERIOD {
            now + PERIOD
        } else {
            self.next + PERIOD
        };
        while let Some(packet) = self.queue.pop() {
            if now.saturating_duration_since(packet.created) <= MAX_AGE {
                return Ok(Some(packet.pcm));
            }
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        self.underruns += 1;
        Ok(Some([0; 64]))
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
#[derive(Default)]
struct Converter {
    low: [[f32; 2]; 2],
    previous: [f32; 2],
    high: [f32; 2],
}
impl Converter {
    fn convert(&mut self, data: &[f32; 512 * 4]) -> [i8; 64] {
        let mut result = [0; 64];
        for (index, block) in data.as_chunks::<64>().0.iter().enumerate() {
            let mut sum = [0.0f64; 2];
            for frame in block.as_chunks::<4>().0 {
                for side in 0..2 {
                    let value = if frame[side + 2].is_finite() {
                        frame[side + 2].clamp(-1.0, 1.0)
                    } else {
                        0.0
                    };
                    self.high[side] = 0.9974 * (self.high[side] + value - self.previous[side]);
                    self.previous[side] = value;
                    self.low[0][side] += 0.1 * (self.high[side] - self.low[0][side]);
                    self.low[1][side] += 0.1 * (self.low[0][side] - self.low[1][side]);
                    sum[side] += f64::from(self.low[1][side]);
                }
            }
            for side in 0..2 {
                result[index * 2 + side] = ((sum[side] / 16.0).clamp(-0.25, 0.25) * 127.0)
                    .round()
                    .clamp(-31.0, 31.0) as i8;
            }
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downsampling_preserves_sides_limits_and_finite_output() {
        let mut data = [0.0; 512 * 4];
        for frame in data.as_chunks_mut::<4>().0 {
            frame[0] = 1.0;
            frame[1] = -1.0;
            frame[2] = 1.0;
            frame[3] = -1.0;
        }
        let mut convert = Converter::default();
        let out = convert.convert(&data);
        assert!(
            out.as_chunks::<2>()
                .0
                .iter()
                .all(|p| (0..=32).contains(&p[0]) && (-32..=0).contains(&p[1]))
        );
        assert!(out[62] >= 30 && out[63] <= -30);
        data.fill(f32::NAN);
        for _ in 0..4 {
            convert.convert(&data);
        }
        assert_eq!(convert.convert(&data), [0; 64]);
    }
    #[test]
    fn pacing_drops_stale_data_without_catchup_and_underrun_sends_silence() {
        let now = Instant::now();
        let mut out = Output {
            queue: Arc::new(ArrayQueue::new(4)),
            stop: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicBool::new(false)),
            next: now,
            dropped: Arc::new(AtomicU64::new(0)),
            underruns: 0,
        };
        out.queue
            .push(Packet {
                created: now - MAX_AGE - Duration::from_millis(1),
                pcm: [10; 64],
            })
            .ok()
            .unwrap();
        out.queue
            .push(Packet {
                created: now,
                pcm: [20; 64],
            })
            .ok()
            .unwrap();
        assert_eq!(out.take(now).unwrap(), Some([20; 64]));
        assert_eq!(out.dropped.load(Ordering::Relaxed), 1);
        assert_eq!(out.take(now).unwrap(), None);
        assert_eq!(out.take(now + PERIOD * 5).unwrap(), Some([0; 64]));
        assert_eq!(out.take(now + PERIOD * 5).unwrap(), None);
        assert_eq!(out.underruns, 1);
    }
}
