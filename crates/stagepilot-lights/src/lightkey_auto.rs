//! Auto capture/replay: which songs capture and which replay (pure logic, no I/O, no clock).
//!
//! The Lighting link decides once per song, when its timer starts: capture a song that is not
//! fully captured yet, replay one that is, and replay when there is no lighting connection. The
//! decision never changes mid-song. A link drop of up to a minute is covered ("grace"); a longer
//! one fails the capture, which is then saved as incomplete if it qualifies.

use serde::{Deserialize, Serialize};
use stagepilot_core::config::{LightkeyMode, SongLightingCueMap};
use ts_rs::TS;

/// A link that was usable this recently still counts as usable; a drop this long fails a capture.
pub const GRACE_SECONDS: f64 = 60.0;
/// A pass is a complete capture when it reached within this many seconds of the song's length.
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

    /// The connection has been down for the grace period or longer.
    pub fn dropped_too_long(&self, now: f64) -> bool {
        self.down_since
            .is_some_and(|since| now - since >= GRACE_SECONDS)
    }

    pub fn down_since(&self) -> Option<f64> {
        self.down_since
    }
}

/// What a finished pass is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassVerdict {
    /// Covered the song from the start to within [`COMPLETE_MARGIN_SECONDS`] of its end.
    Complete,
    /// Has at least one change but is not complete: saved as "Incomplete capture", however short.
    Partial,
    /// Nothing changed after the starting state: the song stays "not captured".
    NoChanges,
    /// The song's length is unknown, so the pass cannot be judged (nothing is saved).
    UnknownLength,
}

/// `covered` seconds of a `duration`-second song were recorded; `link_failed`: the connection was
/// down for a minute or more. (A seek or reset never gets here: it discards the pass.)
pub fn judge(duration: f64, covered: f64, has_changes: bool, link_failed: bool) -> PassVerdict {
    if duration <= 0.0 {
        return PassVerdict::UnknownLength;
    }
    if !has_changes {
        return PassVerdict::NoChanges;
    }
    // A song shorter than twice the margin needs at least half of it, so a few seconds never
    // count as complete.
    let margin = COMPLETE_MARGIN_SECONDS.min(duration / 2.0);
    if !link_failed && covered.min(duration) >= duration - margin {
        PassVerdict::Complete
    } else {
        PassVerdict::Partial
    }
}
