//! Safety net for the song position: if Playback's heartbeat keeps disagreeing with the
//! position StagePilot's timer is running at, StagePilot follows Playback.
//!
//! The normal paths (resume, seek, start) carry the position from the event. This catches
//! whatever they miss, for any reason (a jump made while paused whose play outran the
//! heartbeat, a dropped message, a clamped seek): the timer, ProPresenter and the lights would
//! otherwise stay on the wrong position for the rest of the song, because the heartbeat that
//! confirms "playing" was never compared with the position.
//!
//! It must NOT act on a disagreement Playback itself caused while playing: a section loop or a
//! section jump moves Playback's position (the real 25-minute capture loops a 22 s section
//! 24 times), and those stay monitor-only. So a playing -> playing position jump (or a
//! section event) suspends the net for that song until a start, resume, seek, snapshot or
//! song change event re-arms it. The original fault (paused -> jump -> play) is never a
//! playing -> playing jump, so it is still corrected.

use crate::normalizer::{EventType, PlaybackEvent};
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
    /// The song Playback moved by itself (loop or jump) while playing; the net is off for it.
    jumped_song: Option<i64>,
}

impl DriftWatch {
    /// Forget the count (not playing, a different song, nothing to compare with).
    pub fn reset(&mut self) {
        self.in_a_row = 0;
    }

    /// Learn from the events of one message batch, in order. A position jump or section
    /// event of a playing song suspends the net for that song; a start, resume, seek,
    /// snapshot or song change re-arms it.
    pub fn note_events(&mut self, events: &[PlaybackEvent]) {
        for e in events {
            match e.kind {
                EventType::PositionJump | EventType::SectionJump | EventType::SectionLoop => {
                    if let Some(song) = e.song_id {
                        self.jumped_song = Some(song);
                    }
                    self.in_a_row = 0;
                }
                EventType::SongStarted
                | EventType::SongResumed
                | EventType::Seek
                | EventType::StateSnapshot
                | EventType::SongSelected
                | EventType::SongChanged => {
                    self.jumped_song = None;
                    self.in_a_row = 0;
                }
                _ => {}
            }
        }
    }

    /// True while Playback's own loop or jump has the net off for `song_id`.
    pub fn is_suspended_for(&self, song_id: i64) -> bool {
        self.jumped_song == Some(song_id)
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

    fn ev(kind: EventType, song: i64) -> PlaybackEvent {
        PlaybackEvent {
            kind,
            timestamp: 0.0,
            song_id: Some(song),
            position: Some(0.0),
            previous_song_id: None,
            continues_playing: false,
            playing: None,
            pad: None,
            setlist_version: None,
            previous_version: None,
            section_id: None,
            active: None,
            reason: None,
            message_kind: None,
            provisional: false,
        }
    }

    #[test]
    fn a_playback_jump_suspends_the_net_until_a_seek_resume_or_start() {
        for rearm in [
            EventType::Seek,
            EventType::SongResumed,
            EventType::SongStarted,
            EventType::SongChanged,
        ] {
            let mut w = DriftWatch::default();
            w.note_events(&[ev(EventType::PositionJump, 7)]);
            assert!(w.is_suspended_for(7));
            assert!(!w.is_suspended_for(8));
            w.note_events(&[ev(EventType::SongPaused, 7)]);
            assert!(w.is_suspended_for(7), "a pause alone does not re-arm");
            w.note_events(&[ev(rearm, 7)]);
            assert!(!w.is_suspended_for(7), "{rearm:?} re-arms");
        }
        let mut w = DriftWatch::default();
        w.note_events(&[ev(EventType::SectionLoop, 7)]);
        assert!(w.is_suspended_for(7));
    }

    #[test]
    fn after_a_loop_pause_seek_play_is_corrected_again() {
        let mut w = DriftWatch::default();
        w.note_events(&[ev(EventType::PositionJump, 7)]);
        assert!(w.is_suspended_for(7));
        w.note_events(&[ev(EventType::SongPaused, 7)]);
        w.note_events(&[ev(EventType::Seek, 7)]);
        w.note_events(&[ev(EventType::SongResumed, 7)]);
        assert!(!w.is_suspended_for(7));
        assert!(!w.check(114.6, 233.9));
        assert!(w.check(115.6, 234.9));
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
