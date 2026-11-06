//! Room loudness while a song is recorded: level maths, the per-slot recorder on the song clock,
//! and how repeat recordings average. Only levels are ever kept; samples are measured and dropped
//! the moment they arrive, never stored, written or sent anywhere.
//!
//! Byte scale: `0` = no data (a pause or a missed slot), `1..=255` = -60..0 dBFS. dBFS is a
//! digital level, not sound pressure (SPL).

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;
use stagepilot_core::config::{SongLoudness, MAX_LOUDNESS_TAKES, MAX_LOUDNESS_VALUES};

/// One slot of the envelope.
pub const STEP_SECONDS: f64 = 0.25;
/// Levels at or below this read as the quietest byte.
pub const FLOOR_DB: f64 = -60.0;

/// The level of a block of samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WindowLevel {
    pub rms_db: f64,
    pub peak_db: f64,
}

fn to_db(linear: f64) -> f64 {
    if linear <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

/// RMS and peak of `samples` (full scale is 1.0) in dBFS. Silence is negative infinity.
pub fn window_level(samples: &[f32]) -> WindowLevel {
    let mut acc = Acc::default();
    acc.add(samples);
    acc.level()
}

/// dBFS -> byte: clamp to -60..0, then `1..=255`. Silence and anything below -60 is `1`.
pub fn level_byte(db: f64) -> u8 {
    if db.is_nan() {
        return 1;
    }
    let clamped = db.clamp(FLOOR_DB, 0.0);
    let scaled = (clamped - FLOOR_DB) / -FLOOR_DB * 254.0;
    // Bounded 0..=254 by the clamp above.
    1 + scaled.round() as u8
}

/// byte -> dBFS; `None` for the no-data byte.
pub fn byte_db(value: u8) -> Option<f64> {
    (value != 0).then(|| FLOOR_DB + f64::from(value - 1) / 254.0 * -FLOOR_DB)
}

/// Averages interleaved channels down to one.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

#[derive(Debug, Default, Clone, Copy)]
struct Acc {
    sum_sq: f64,
    count: u64,
    peak: f64,
}

impl Acc {
    fn add(&mut self, samples: &[f32]) {
        for &s in samples {
            let v = f64::from(s);
            self.sum_sq += v * v;
            self.peak = self.peak.max(v.abs());
        }
        self.count += samples.len() as u64;
    }

    fn level(&self) -> WindowLevel {
        let rms = if self.count == 0 {
            0.0
        } else {
            (self.sum_sq / self.count as f64).sqrt()
        };
        WindowLevel {
            rms_db: to_db(rms),
            peak_db: to_db(self.peak),
        }
    }
}

/// Collects windows on the song clock. `started_at` is the wall-clock second at which song time 0
/// happened (it moves when the song resumes); a block ending at wall time `t` is placed by that.
#[derive(Debug)]
pub struct LoudnessRecorder {
    started_at: f64,
    running: bool,
    /// Per slot: the stretch that heard it (see [`next_stretch`](Self::next_stretch)) and its level.
    slots: BTreeMap<usize, (u32, Acc)>,
    stretch: u32,
}

impl LoudnessRecorder {
    pub fn new(started_at: f64) -> Self {
        Self {
            started_at,
            running: true,
            slots: BTreeMap::new(),
            stretch: 0,
        }
    }

    /// The song was moved (a seek or restart): what is heard from here on replaces anything heard
    /// earlier in the same slots, so the latest recording of a stretch wins. A pause that resumes
    /// in place is not a new stretch. Slots nobody heard stay empty (a gap).
    pub fn next_stretch(&mut self) {
        self.stretch += 1;
    }

    /// Song time 0 moved (the song resumed).
    pub fn set_started_at(&mut self, started_at: f64) {
        self.started_at = started_at;
    }

    /// A paused song leaves a gap: nothing is collected until it runs again.
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }

    /// Mono `samples` whose last sample was heard at wall time `wall_end`.
    pub fn push(&mut self, wall_end: f64, sample_rate: u32, samples: &[f32]) {
        if !self.running || samples.is_empty() || sample_rate == 0 {
            return;
        }
        let rate = f64::from(sample_rate);
        let first = wall_end - samples.len() as f64 / rate - self.started_at;
        let mut index = 0usize;
        while index < samples.len() {
            let t = first + index as f64 / rate;
            if t < 0.0 {
                index += 1;
                continue;
            }
            let slot = (t / STEP_SECONDS).floor() as usize;
            if slot >= MAX_LOUDNESS_VALUES {
                break;
            }
            // Samples left in this slot, at least one so the loop always moves.
            let slot_end = (slot + 1) as f64 * STEP_SECONDS;
            let take = (((slot_end - t) * rate).ceil() as usize).clamp(1, samples.len() - index);
            let entry = self
                .slots
                .entry(slot)
                .or_insert_with(|| (self.stretch, Acc::default()));
            if entry.0 != self.stretch {
                *entry = (self.stretch, Acc::default());
            }
            entry.1.add(&samples[index..index + take]);
            index += take;
        }
    }

    /// The envelope for a song of `duration` seconds, or `None` when nothing was heard.
    pub fn finish(&self, duration: f64) -> Option<SongLoudness> {
        let wanted = (duration / STEP_SECONDS).ceil().max(0.0) as usize;
        let len = wanted.min(MAX_LOUDNESS_VALUES);
        let values: Vec<u8> = (0..len)
            .map(|slot| {
                self.slots
                    .get(&slot)
                    .map_or(0, |(_, acc)| level_byte(acc.level().rms_db))
            })
            .collect();
        values.iter().any(|v| *v != 0).then_some(SongLoudness {
            step_seconds: STEP_SECONDS,
            values,
            takes: 1,
        })
    }
}

/// Folds a new recording into the song's saved envelope. Per slot the two levels average in the
/// linear power domain, weighted by their `takes`; a slot only one of them has is kept as is.
/// `takes` adds up but stops at 8 so the envelope can still adapt. A different step replaces.
pub fn merge_loudness(existing: Option<&SongLoudness>, mut new: SongLoudness) -> SongLoudness {
    new.values.truncate(MAX_LOUDNESS_VALUES);
    new.takes = new.takes.clamp(1, MAX_LOUDNESS_TAKES);
    let Some(old) = existing.filter(|old| (old.step_seconds - new.step_seconds).abs() < 1e-9)
    else {
        return new;
    };
    let (w_old, w_new) = (
        f64::from(old.takes.clamp(1, MAX_LOUDNESS_TAKES)),
        f64::from(new.takes.clamp(1, MAX_LOUDNESS_TAKES)),
    );
    let len = old
        .values
        .len()
        .max(new.values.len())
        .min(MAX_LOUDNESS_VALUES);
    let values = (0..len)
        .map(|i| {
            let a = old.values.get(i).copied().unwrap_or(0);
            let b = new.values.get(i).copied().unwrap_or(0);
            match (byte_db(a), byte_db(b)) {
                (Some(da), Some(db)) => {
                    let power = |d: f64| 10f64.powf(d / 10.0);
                    let mean = (w_old * power(da) + w_new * power(db)) / (w_old + w_new);
                    level_byte(10.0 * mean.log10())
                }
                (Some(_), None) => a,
                (None, Some(_)) => b,
                (None, None) => 0,
            }
        })
        .collect();
    SongLoudness {
        step_seconds: new.step_seconds,
        values,
        takes: (old.takes + new.takes).min(MAX_LOUDNESS_TAKES),
    }
}

/// The song's own loudest slot as `(seconds, byte)`, for the picture's text alternative.
pub fn peak_slot(loudness: &SongLoudness) -> Option<(f64, u8)> {
    loudness
        .values
        .iter()
        .enumerate()
        .filter(|(_, v)| **v != 0)
        .max_by_key(|(i, v)| (**v, std::cmp::Reverse(*i)))
        .map(|(i, v)| (i as f64 * loudness.step_seconds, *v))
}

// ------------------------------------------------------------------------------ live meter

/// What the picker's meter shows: the loudest and average level heard since it was last read.
#[derive(Debug, Default)]
pub struct LevelMeter {
    inner: Mutex<MeterState>,
}

#[derive(Debug, Default)]
struct MeterState {
    acc: Acc,
    last: Option<WindowLevel>,
}

impl LevelMeter {
    pub fn feed(&self, samples: &[f32]) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .acc
            .add(samples);
    }

    /// Level since the last call; the previous one again if nothing arrived meanwhile.
    pub fn read(&self) -> Option<WindowLevel> {
        let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if state.acc.count > 0 {
            state.last = Some(state.acc.level());
            state.acc = Acc::default();
        }
        state.last
    }

    pub fn clear(&self) {
        *self.inner.lock().unwrap_or_else(|p| p.into_inner()) = MeterState::default();
    }
}
