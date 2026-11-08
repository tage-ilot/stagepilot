//! Safety net for the song position: if Playback's heartbeat keeps disagreeing with the
//! position StagePilot's timer is running at, StagePilot follows Playback.
//!
//! The normal paths (resume, seek, start) carry the position from the event. This catches
//! whatever they miss, for any reason (a jump made while paused whose play outran the
//! heartbeat, a dropped message, a clamped seek): the timer, ProPresenter and the lights would
//! otherwise stay on the wrong position for the rest of the song, because the heartbeat that
//! confirms "playing" was never compared with the position.

use chrono::{DateTime, Utc};
use stagepilot_core::state::{TimerState, TimerStatus};

/// A heartbeat this far from the timer's position counts as a disagreement (seconds).
/// Normal playback drifts well under a second.
pub const DISAGREEMENT_SECONDS: f64 = 3.0;
/// Disagreeing heartbeats in a row before StagePilot re-syncs.
pub const HEARTBEATS_IN_A_ROW: u32 = 2;

/// Where a running timer says the song is now. `None` unless it is counting down from a known
/// song position.
pub fn timer_position(timer: &TimerState, now: DateTime<Utc>) -> Option<f64> {
    if timer.status != TimerStatus::Running {
        return None;
    }
    let started = timer.started_at?;
    let base = timer.position_seconds?;
    let elapsed = (now - started).num_milliseconds() as f64 / 1000.0;
    (base.is_finite() && elapsed >= 0.0).then_some(base + elapsed)
}

#[derive(Debug, Default, Clone)]
pub struct DriftWatch {
    in_a_row: u32,
}

impl DriftWatch {
    /// Forget the count (not playing, a different song, nothing to compare with).
    pub fn reset(&mut self) {
        self.in_a_row = 0;
    }

    /// Feed one heartbeat of a playing song. True when StagePilot should re-sync to
    /// `heartbeat_position` now.
    pub fn check(&mut self, heartbeat_position: f64, timer_position: f64) -> bool {
        if (heartbeat_position - timer_position).abs() <= DISAGREEMENT_SECONDS {
            self.in_a_row = 0;
            return false;
        }
        self.in_a_row += 1;
        if self.in_a_row >= HEARTBEATS_IN_A_ROW {
            self.in_a_row = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_119_second_disagreement_is_corrected_on_the_second_heartbeat() {
        let mut w = DriftWatch::default();
        assert!(!w.check(114.6, 233.9));
        assert!(w.check(115.6, 234.9));
    }

    #[test]
    fn normal_drift_never_resyncs() {
        let mut w = DriftWatch::default();
        for i in 0..100 {
            assert!(!w.check(100.0 + i as f64, 100.8 + i as f64));
        }
    }

    #[test]
    fn one_odd_heartbeat_is_ignored_and_the_count_restarts() {
        let mut w = DriftWatch::default();
        assert!(!w.check(50.0, 90.0));
        assert!(!w.check(51.0, 51.2)); // back in agreement
        assert!(!w.check(52.0, 92.0));
        assert!(w.check(53.0, 93.0));
        assert!(!w.check(54.0, 94.0)); // starts counting again after a re-sync
    }

    #[test]
    fn timer_position_counts_from_the_start_position() {
        let now = Utc::now();
        let timer = TimerState {
            status: TimerStatus::Running,
            started_at: Some(now - chrono::Duration::milliseconds(2500)),
            position_seconds: Some(100.0),
            ..Default::default()
        };
        let p = timer_position(&timer, now).unwrap();
        assert!((p - 102.5).abs() < 1e-6);
        let stopped = TimerState {
            status: TimerStatus::Stopped,
            ..timer
        };
        assert!(timer_position(&stopped, now).is_none());
    }
}
