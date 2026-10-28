//! Bounded PCM transport and immutable mixer settings. The render path never
//! waits on a decoder or a mutex and never advances a starved file's playhead.
use super::{RATE, SPEAKER_FILE_GAIN};
use crossbeam_queue::ArrayQueue;
use serde::{Deserialize, Serialize};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub(super) const FILE_CAPACITY: usize = RATE as usize * 4;
pub(super) const FILE_WATERMARK: usize = RATE as usize; // native decode stays one second ahead

pub(super) struct PcmStream {
    pub track_id: u64,
    pub speaker: bool,
    pub grips: bool,
    pub native: bool,
    pub queue: ArrayQueue<[f32; 4]>,
    pub consumed: AtomicU64,
    pub total: AtomicU64,
    pub tail_frames: AtomicU64,
    pub paused: AtomicBool,
    pub ready: AtomicBool,
    pub eof: AtomicBool,
    pub canceled: AtomicBool,
    pub error: Mutex<Option<String>>,
}

impl PcmStream {
    pub fn new(track_id: u64, route: &str, native: bool, total: u64) -> Self {
        Self {
            track_id,
            speaker: route != "grips",
            grips: route != "speaker",
            native,
            queue: ArrayQueue::new(if native {
                FILE_WATERMARK
            } else {
                FILE_CAPACITY
            }),
            consumed: AtomicU64::new(0),
            total: AtomicU64::new(total),
            tail_frames: AtomicU64::new(RATE as u64 / 10),
            paused: AtomicBool::new(false),
            ready: AtomicBool::new(!native),
            eof: AtomicBool::new(!native),
            canceled: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }
    pub fn mode(&self) -> &'static str {
        if self.canceled.load(Ordering::Acquire) {
            "stopped"
        } else if self.paused.load(Ordering::Acquire) {
            "paused"
        } else if self.queue.is_empty()
            && self.eof.load(Ordering::Acquire)
            && self.tail_frames.load(Ordering::Relaxed) == 0
        {
            "finished"
        } else if !self.ready.load(Ordering::Acquire) {
            if self.consumed.load(Ordering::Relaxed) == 0 {
                "starting"
            } else {
                "buffering"
            }
        } else if self.queue.is_empty() && !self.eof.load(Ordering::Acquire) {
            "buffering"
        } else {
            "playing"
        }
    }
    pub fn position(&self) -> f64 {
        self.consumed.load(Ordering::Relaxed) as f64 / RATE as f64
    }
    pub fn remaining(&self) -> f32 {
        self.total
            .load(Ordering::Relaxed)
            .saturating_sub(self.consumed.load(Ordering::Relaxed)) as f32
            / RATE as f32
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct MixerSettings {
    pub file_gain: f32,
    pub cue_gain: f32,
    pub game_gain: f32,
    pub bridge_gain: f32,
    pub speaker_gain: f32,
    pub ducking: f32,
    pub solo: String,
}
impl Default for MixerSettings {
    fn default() -> Self {
        Self {
            file_gain: 1.0,
            cue_gain: 1.0,
            game_gain: 1.0,
            bridge_gain: 1.0,
            speaker_gain: 1.0,
            ducking: 0.65,
            solo: "stereo".into(),
        }
    }
}
impl MixerSettings {
    pub fn validate(&self) -> Result<(), String> {
        if ![
            self.file_gain,
            self.cue_gain,
            self.game_gain,
            self.bridge_gain,
            self.speaker_gain,
            self.ducking,
        ]
        .iter()
        .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
        {
            return Err("Audio levels must be between 0 and 100%.".into());
        }
        if !matches!(self.solo.as_str(), "stereo" | "left" | "right") {
            return Err("Choose both grips, left, or right.".into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct RenderState {
    pub duty: [f32; 2],
    pub duck: f32,
}
impl RenderState {
    pub fn mix_grips(
        &mut self,
        cue: [f32; 2],
        background: [f32; 2],
        settings: &MixerSettings,
        cue_active: bool,
    ) -> [f32; 2] {
        let target = if cue_active { settings.ducking } else { 0.0 };
        let speed = if target > self.duck { 0.01 } else { 0.0002 };
        self.duck += (target - self.duck) * speed;
        let mut result = [0.0; 2];
        for i in 0..2 {
            result[i] = cue[i] * settings.cue_gain + background[i] * (1.0 - self.duck);
        }
        if settings.solo == "left" {
            result[1] = 0.0;
        }
        if settings.solo == "right" {
            result[0] = 0.0;
        }
        result
    }
}

pub(super) fn routed_frame(input: [f32; 2], grip: [f32; 2], stream: &PcmStream) -> [f32; 4] {
    let speaker = if stream.speaker {
        (input[0] + input[1]) * 0.5 * SPEAKER_FILE_GAIN
    } else {
        0.0
    };
    [
        speaker,
        speaker,
        if stream.grips { grip[0] } else { 0.0 },
        if stream.grips { grip[1] } else { 0.0 },
    ]
}
