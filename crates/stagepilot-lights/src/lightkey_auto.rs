//! Auto capture/replay: which songs capture and which replay (pure logic, no I/O, no clock).
//!
//! The Lighting link decides once per song, when its timer starts: capture a song that is not
//! fully captured yet, replay one that is, and replay when there is no lighting connection. The
//! decision never changes mid-song. A capture is judged by how much of the song was recorded in
//! total ([`Coverage`]); gaps anywhere (a link drop, a seek, a pause, a late start) are fine as
//! long as no more than [`COMPLETE_MARGIN_SECONDS`] of the song are missing.

use serde::{Deserialize, Serialize};
use stagepilot_core::config::{LightkeyMode, SongLightingCueMap};
use ts_rs::TS;

/// A link that was usable this recently still counts as usable when a song starts and the mode
/// is Auto (a short drop never flips the decision to replay).
pub const GRACE_SECONDS: f64 = 60.0;
/// A pass is a complete capture when no more than this many seconds of the song are missing.
pub const COMPLETE_MARGIN_SECONDS: f64 = 10.0;

/// What a song's cue map says about capturing, as far as the decision cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongCapture {
    /// No captured rows (nothing, or only hand-built rows).
    NotCaptured,
    /// Captured rows from a partial pass (or an older file with no status).
    Incomplete,
    Complete,
}

pub fn song_capture(map: Option<&SongLightingCueMap>) -> SongCapture {
    match map {
        Some(map) if map.is_fully_captured() => SongCapture::Complete,
        Some(map) if map.has_captured_rows() => SongCapture::Incomplete,
        _ => SongCapture::NotCaptured,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Action {
    Capture,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    AutoCapturing,
    AutoFullyCaptured,
    AutoNoConnection,
    PinnedReplay,
    PinnedRecord,
}

impl Reason {
    pub fn text(self) -> &'static str {
        match self {
            Self::AutoCapturing => "Auto: capturing this song",
            Self::AutoFullyCaptured => "Auto: replaying (fully captured)",
            Self::AutoNoConnection => "Auto: replaying (no connection)",
            Self::PinnedReplay => "Replay: replaying every song",
            Self::PinnedRecord => "Record: capturing every song",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub action: Action,
    pub reason: Reason,
}

/// The per-song decision. `connected` already includes the grace period (`Availability::usable`).
pub fn decide(mode: LightkeyMode, connected: bool, song: SongCapture) -> Decision {
    let (action, reason) = match mode {
        LightkeyMode::Replay => (Action::Replay, Reason::PinnedReplay),
        LightkeyMode::Record => (Action::Capture, Reason::PinnedRecord),
        LightkeyMode::Auto if !connected => (Action::Replay, Reason::AutoNoConnection),
        LightkeyMode::Auto if song == SongCapture::Complete => {
            (Action::Replay, Reason::AutoFullyCaptured)
        }
        LightkeyMode::Auto => (Action::Capture, Reason::AutoCapturing),
    };
    Decision { action, reason }
}

/// Watches whether the lighting connection (API linked, Lightkey running, fixture stream live,
/// Lighting on) is usable, on a caller-supplied monotonic clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct Availability {
    /// The connection has been usable at least once.
    seen: bool,
    /// When the current drop began.
    down_since: Option<f64>,
}

impl Availability {
    /// Record what the connection is like at `now` (monotonic seconds).
    pub fn observe(&mut self, now: f64, available: bool) {
        if available {
            self.seen = true;
            self.down_since = None;
        } else if self.seen && self.down_since.is_none() {
            self.down_since = Some(now);
        }
    }

    /// Available now, or dropped less than the grace period ago.
    pub fn usable(&self, now: f64, available_now: bool) -> bool {
        available_now
            || (self.seen
                && self
                    .down_since
                    .is_some_and(|since| now - since < GRACE_SECONDS))
    }

    pub fn down_since(&self) -> Option<f64> {
        self.down_since
    }
}

/// The stretches of song time that were really recorded, as a set of disjoint spans in seconds.
/// A stretch is recorded only while the timer runs and the lighting connection is live; the
/// union is what counts, so a stretch recorded twice counts once.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coverage {
    /// Sorted, disjoint, non-empty `(start, end)` spans.
    spans: Vec<(f64, f64)>,
}

impl Coverage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the stretch `start..end` (song seconds). Empty, reversed and non-finite stretches are
    /// ignored; negative starts are cut to 0.
    pub fn add(&mut self, start: f64, end: f64) {
        if !start.is_finite() || !end.is_finite() {
            return;
        }
        let start = start.max(0.0);
        if end <= start {
            return;
        }
        let (mut lo, mut hi) = (start, end);
        let mut spans = Vec::with_capacity(self.spans.len() + 1);
        for &(a, b) in &self.spans {
            if b < lo || a > hi {
                spans.push((a, b));
            } else {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        spans.push((lo, hi));
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        self.spans = spans;
    }

    /// Seconds recorded, counting only `0..duration` (a take past the end is not coverage).
    /// A `duration` of 0 or less (unknown) counts everything.
    pub fn total(&self, duration: f64) -> f64 {
        self.spans
            .iter()
            .map(|&(a, b)| {
                if duration > 0.0 {
                    (b.min(duration) - a.min(duration)).max(0.0)
                } else {
                    b - a
                }
            })
            .sum()
    }

    /// Seconds of a `duration`-second song that were not recorded.
    pub fn missing(&self, duration: f64) -> f64 {
        (duration - self.total(duration)).max(0.0)
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

/// What a finished pass is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassVerdict {
    /// Recorded all of the song but at most [`COMPLETE_MARGIN_SECONDS`], wherever those seconds are.
    Complete,
    /// Has at least one change but is not complete: saved as "Incomplete capture", however short.
    Partial,
    /// Nothing changed after the starting state: the song stays "not captured".
    NoChanges,
    /// The song's length is unknown, so the pass cannot be judged (nothing is saved).
    UnknownLength,
}

/// `covered` seconds in total of a `duration`-second song were recorded (see [`Coverage`]).
/// Where the missing seconds are does not matter: a link drop, a seek, a pause or a late start
/// all just leave a gap.
pub fn judge(duration: f64, covered: f64, has_changes: bool) -> PassVerdict {
    if duration <= 0.0 {
        return PassVerdict::UnknownLength;
    }
    if !has_changes {
        return PassVerdict::NoChanges;
    }
    // A song shorter than twice the margin needs at least half of it, so a few seconds never
    // count as complete.
    let margin = COMPLETE_MARGIN_SECONDS.min(duration / 2.0);
    // The tolerance only absorbs float error in summing spans (exactly 10 s missing is complete).
    if covered.min(duration) >= duration - margin - 1e-6 {
        PassVerdict::Complete
    } else {
        PassVerdict::Partial
    }
}
