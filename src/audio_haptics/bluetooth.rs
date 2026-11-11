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
                let mut state = RenderState::with_duty(shared.duty_load());
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
                        pcm: converter.convert(&data, state.ceiling),
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
    pub fn dropped_total(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
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
/// Knee-input at which each level's full-strength peak (0.928 x its ceiling) lands, as a
/// multiple of `ceiling`: the quantiser reference is `ceiling * BT_REFERENCE[level]`.
/// Soft and Normal reproduce the shipped mapping exactly (reference 0.32). Strong and
/// Max only have the last few counts of the 6-bit +/-31 range above Normal, so they step
/// peak by a smaller amount than on USB; they are folded in with the same soft knee.
const BT_REFERENCE: [f32; 4] = [2.0, 1.0, 0.889, 0.715];
/// Maps grip drive to the +/-31 PCM range relative to the selected output `ceiling`
/// (value / reference, soft knee, x 31). Never exceeds +/-31.
fn quantize(value: f64, ceiling: f32) -> i8 {
    if !value.is_finite() {
        return 0;
    }
    let reference = LEVEL_CEILINGS
        .iter()
        .position(|c| (*c - ceiling).abs() < 1e-6)
        .map_or(ceiling, |i| ceiling * BT_REFERENCE[i])
        .max(0.01);
    let x = (value / f64::from(reference)) as f32;
    (f64::from(knee_curve(x).clamp(-1.0, 1.0)) * 31.0)
        .round()
        .clamp(-31.0, 31.0) as i8
}
#[derive(Default)]
struct Converter {
    low: [[f32; 2]; 2],
    previous: [f32; 2],
    high: [f32; 2],
}
impl Converter {
    fn convert(&mut self, data: &[f32; 512 * 4], ceiling: f32) -> [i8; 64] {
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
                result[index * 2 + side] = quantize(sum[side] / 16.0, ceiling);
            }
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Full-strength drive peaks at 0.928 x ceiling (knee of 1.0) for every level.
    fn full_peak(level: usize) -> i8 {
        quantize(
            f64::from(knee_curve(1.0) * LEVEL_CEILINGS[level]),
            LEVEL_CEILINGS[level],
        )
    }
    #[test]
    fn each_level_maps_its_full_strength_peak_to_a_stepped_pcm_value() {
        // Soft and Normal are the shipped mapping; Strong and Max step above Normal.
        assert_eq!(
            [full_peak(0), full_peak(1), full_peak(2), full_peak(3)],
            [14, 28, 29, 31]
        );
        // Below the knee the mapping is exactly the old linear one against 0.32.
        assert_eq!(quantize(f64::from(MAX_AMPLITUDE) / 2.0, MAX_AMPLITUDE), 16);
        assert_eq!(quantize(0.08, LEVEL_CEILINGS[0]), 8);
        assert_eq!(quantize(-0.08, LEVEL_CEILINGS[0]), -8);
        assert_eq!(quantize(0.0, MAX_AMPLITUDE), 0);
        assert_eq!(quantize(5.0, MAX_AMPLITUDE), 31);
        assert_eq!(quantize(f64::NAN, MAX_AMPLITUDE), 0);
    }
    #[test]
    fn quantize_is_monotonic_bounded_and_covers_every_ceiling() {
        for ceiling in LEVEL_CEILINGS {
            let mut last = i8::MIN;
            for step in 0..=1200 {
                let value = f64::from(step) / 1000.0 - 0.6;
                let q = quantize(value, ceiling);
                assert!((-31..=31).contains(&q) && q >= last);
                last = q;
            }
            assert!(quantize(f64::from(ceiling), ceiling) <= 31);
            assert!(quantize(-f64::from(ceiling), ceiling) >= -31);
        }
        // The same absolute drive is quieter at a higher level's reference.
        assert!(quantize(0.2, 0.32) > quantize(0.2, 0.75));
    }
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
        let out = convert.convert(&data, MAX_AMPLITUDE);
        assert!(
            out.as_chunks::<2>()
                .0
                .iter()
                .all(|p| (0..=32).contains(&p[0]) && (-32..=0).contains(&p[1]))
        );
        // A unit step decays through the 0.9974 high-pass to about 0.28 by the last block,
        // which the full-range quantiser (0.32 -> 31) maps to 27. The old test expected 30+
        // because the previous gain saturated at 0.25.
        assert!(out[62] >= 25 && out[63] <= -25);
        data.fill(f32::NAN);
        for _ in 0..4 {
            convert.convert(&data, MAX_AMPLITUDE);
        }
        assert_eq!(convert.convert(&data, MAX_AMPLITUDE), [0; 64]);
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
