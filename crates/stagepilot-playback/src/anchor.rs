//! Where Playback's song is right now, between its heartbeats.
//!
//! Playback sends a heartbeat about once a second and nothing else says where the song is. A reader that waits for the next beat
//! learns that a song started playing up to a second late, and a seek up to a second late. This keeps ONE anchor (song, position,
//! playing, the moment it was received) that is moved by the earliest signal there is:
//!
//! * a heartbeat: song, position and play state, anchored at the moment it was RECEIVED (not when somebody later looked at it);
//! * `transportPlay` (normalised to `song.started` / `song.resumed` / `song.paused`, with the position the player will be at;
//!   they travel the fast path, so they come before any heartbeat), `waveformSeek` (`seek`, with the new position),
//!   `transportReturnToStart` (`song.stopped`, position 0) and a refused command (`transport.reverted`);
//! * events that carry no position (a section jump or loop, a song change, a selection) are left to the next beat.
//!
//! While the player plays, the position moves on from the anchor by the time that has passed, so a reader never waits for a
//! beat to know where the song is. A beat that arrives within [`EVENT_GRACE`] of such an event and contradicts it was already in
//! flight before the command: it is ignored (the next one is believed).
//!
//! The unknown part is how long a heartbeat was on its way (Playback samples its position, then the network and the socket carry
//! it). Playback's position is then behind the true one by that delivery time; the rig measured Playback's clock against the wall
//! clock at rate 1.0001 with a residual of 7 ms, so there is no drift to correct, and the delivery time of a LAN websocket frame
//! is a few milliseconds.
//!
//! Time is given by the caller (`Instant`s), so a test drives it without waiting.

use crate::normalizer::{EventType, Heartbeat, PlaybackEvent};
use std::time::{Duration, Instant};

/// A beat that contradicts an event this young was in flight before the command that caused the event.
pub const EVENT_GRACE: Duration = Duration::from_millis(1500);
/// While playing, the position is moved on from the anchor by at most this long (a silent connection must not run away).
pub const MAX_EXTRAPOLATE_S: f64 = 5.0;
/// `waveformSeek(86400)` means "to the end": the player clamps it, so it is not a position.
const SEEK_TO_END: f64 = 86_400.0;

/// What moved the anchor last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSource {
    Heartbeat,
    /// A transport or seek event (before the beat that confirms it).
    Event,
}

#[derive(Debug, Clone, Copy)]
struct Anchor {
    song_id: i64,
    position: f64,
    playing: bool,
    at: Instant,
    source: AnchorSource,
}

/// Where the song is at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    pub song_id: i64,
    /// Seconds into the song, moved on from the anchor while it plays.
    pub position: f64,
    pub playing: bool,
    /// Seconds since the anchor (the heartbeat or event it comes from) was received.
    pub age_s: f64,
    pub source: AnchorSource,
    /// How many times a song has been started from the top since the connection began (`song.started`): moves once per start,
    /// not on pause/resume, a seek or a beat.
    pub starts: u64,
}

#[derive(Default)]
pub struct PositionTracker {
    anchor: Option<Anchor>,
    starts: u64,
    beat_serial: u64,
}

impl PositionTracker {
    /// Forgets where the song is (the connection ended or was restarted).
    pub fn clear(&mut self) {
        self.anchor = None;
        self.beat_serial = 0;
    }

    /// One published status: the heartbeat the client holds (`serial` moves with every valid beat) and the events that came with
    /// it. `now` is the moment they were received.
    pub fn observe(
        &mut self,
        now: Instant,
        connected: bool,
        beat: Option<Heartbeat>,
        serial: u64,
        events: &[PlaybackEvent],
    ) {
        let Some(beat) = beat.filter(|_| connected) else {
            self.anchor = None;
            return;
        };
        if serial != self.beat_serial {
            self.beat_serial = serial;
            if !self.beat_is_older_news(now, beat) {
                self.anchor = Some(Anchor {
                    song_id: beat.song_id,
                    position: beat.position,
                    playing: beat.playing,
                    at: now,
                    source: AnchorSource::Heartbeat,
                });
            }
        }
        for event in events {
            self.on_event(now, event);
        }
    }

    /// A beat that disagrees with an event received a moment ago: it was sent before Playback took the command.
    fn beat_is_older_news(&self, now: Instant, beat: Heartbeat) -> bool {
        let Some(a) = self
            .anchor
            .filter(|a| a.source == AnchorSource::Event && now.duration_since(a.at) < EVENT_GRACE)
        else {
            return false;
        };
        if a.song_id != beat.song_id {
            return false;
        }
        let expected = a.position
            + if a.playing {
                now.duration_since(a.at).as_secs_f64()
            } else {
                0.0
            };
        beat.playing != a.playing || (beat.position - expected).abs() > 1.0
    }

    fn on_event(&mut self, now: Instant, event: &PlaybackEvent) {
        match event.kind {
            EventType::SongStarted | EventType::SongResumed => {
                self.starts += u64::from(event.kind == EventType::SongStarted);
                // A beat-made event carries the beat's own position: the anchor was just set from it.
                if event.provisional {
                    self.anchor_from(now, event, Some(true));
                }
            }
            EventType::SongPaused | EventType::SongStopped => {
                if event.provisional || event.position == Some(0.0) {
                    self.anchor_from(now, event, Some(false));
                }
            }
            EventType::TransportReverted => {
                self.anchor_from(now, event, Some(event.playing.unwrap_or(false)));
            }
            EventType::Seek if event.position.is_some_and(|p| p < SEEK_TO_END) => {
                self.anchor_from(now, event, None);
            }
            // No position: left to the next beat.
            _ => {}
        }
    }

    fn anchor_from(&mut self, now: Instant, event: &PlaybackEvent, playing: Option<bool>) {
        let (Some(song_id), Some(position)) = (
            event.song_id.or(self.anchor.map(|a| a.song_id)),
            event.position,
        ) else {
            return;
        };
        let playing = playing.or(self.anchor.map(|a| a.playing)).unwrap_or(false);
        self.anchor = Some(Anchor {
            song_id,
            position,
            playing,
            at: now,
            source: AnchorSource::Event,
        });
    }

    /// Where the song is at `now`; `None` while nothing is known (no connection, no beat yet).
    pub fn moment(&self, now: Instant) -> Option<Moment> {
        let a = self.anchor?;
        let age = now.saturating_duration_since(a.at).as_secs_f64();
        Some(Moment {
            song_id: a.song_id,
            position: a.position
                + if a.playing {
                    age.min(MAX_EXTRAPOLATE_S)
                } else {
                    0.0
                },
            playing: a.playing,
            age_s: age,
            source: a.source,
            starts: self.starts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(song_id: i64, position: f64, playing: bool) -> Heartbeat {
        Heartbeat {
            song_id,
            position,
            playing,
            pad: false,
            setlist_version: None,
        }
    }

    fn event(
        kind: EventType,
        song: i64,
        position: Option<f64>,
        provisional: bool,
    ) -> PlaybackEvent {
        PlaybackEvent {
            kind,
            timestamp: 0.0,
            song_id: Some(song),
            position,
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
            provisional,
        }
    }

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    #[test]
    fn a_beat_is_anchored_when_it_was_received_and_the_position_moves_on_while_it_plays() {
        let t0 = Instant::now();
        let mut t = PositionTracker::default();
        assert!(t.moment(t0).is_none());
        t.observe(t0, true, Some(beat(1, 10.0, true)), 1, &[]);
        let m = t.moment(ms(t0, 400)).unwrap();
        assert!((m.position - 10.4).abs() < 1e-9 && m.playing && (m.age_s - 0.4).abs() < 1e-9);
        // Paused: the position stays.
        t.observe(ms(t0, 1000), true, Some(beat(1, 11.0, false)), 2, &[]);
        assert_eq!(t.moment(ms(t0, 3000)).unwrap().position, 11.0);
        // A silent connection does not run away.
        t.observe(ms(t0, 4000), true, Some(beat(1, 11.0, true)), 3, &[]);
        assert_eq!(
            t.moment(ms(t0, 60_000)).unwrap().position,
            11.0 + MAX_EXTRAPOLATE_S
        );
    }

    #[test]
    fn a_play_event_says_playing_before_any_beat_and_counts_a_start_only_for_song_started() {
        let t0 = Instant::now();
        let mut t = PositionTracker::default();
        t.observe(t0, true, Some(beat(1, 0.0, false)), 1, &[]);
        let started = event(EventType::SongStarted, 1, Some(0.0), true);
        t.observe(ms(t0, 500), true, Some(beat(1, 0.0, false)), 1, &[started]);
        let m = t.moment(ms(t0, 600)).unwrap();
        assert!(m.playing && m.source == AnchorSource::Event && m.starts == 1);
        assert!((m.position - 0.1).abs() < 1e-9);
        // The old beat that was in flight shows it stopped: not believed.
        t.observe(ms(t0, 700), true, Some(beat(1, 0.0, false)), 2, &[]);
        assert!(t.moment(ms(t0, 800)).unwrap().playing);
        // The next one confirms it and takes over.
        t.observe(ms(t0, 1500), true, Some(beat(1, 1.0, true)), 3, &[]);
        let m = t.moment(ms(t0, 1500)).unwrap();
        assert!(m.source == AnchorSource::Heartbeat && m.position == 1.0 && m.starts == 1);
        // Pause then resume: no new start.
        t.observe(
            ms(t0, 2000),
            true,
            Some(beat(1, 1.5, true)),
            3,
            &[event(EventType::SongPaused, 1, Some(1.5), true)],
        );
        assert!(!t.moment(ms(t0, 2100)).unwrap().playing);
        t.observe(
            ms(t0, 2500),
            true,
            Some(beat(1, 1.5, true)),
            3,
            &[event(EventType::SongResumed, 1, Some(1.5), true)],
        );
        let m = t.moment(ms(t0, 2500)).unwrap();
        assert!(m.playing && m.starts == 1);
    }

    #[test]
    fn a_seek_moves_the_position_at_once_but_a_section_jump_and_a_beat_in_flight_do_not() {
        let t0 = Instant::now();
        let mut t = PositionTracker::default();
        t.observe(t0, true, Some(beat(1, 20.0, true)), 1, &[]);
        t.observe(
            ms(t0, 300),
            true,
            Some(beat(1, 20.0, true)),
            1,
            &[event(EventType::Seek, 1, Some(90.0), false)],
        );
        assert!((t.moment(ms(t0, 400)).unwrap().position - 90.1).abs() < 1e-9);
        // A beat sent before the seek still shows the old place: ignored.
        t.observe(ms(t0, 500), true, Some(beat(1, 20.5, true)), 2, &[]);
        assert!((t.moment(ms(t0, 500)).unwrap().position - 90.2).abs() < 1e-9);
        // A section jump carries no position: nothing moves until the beat.
        t.observe(
            ms(t0, 600),
            true,
            Some(beat(1, 20.5, true)),
            2,
            &[event(EventType::SectionJump, 1, None, false)],
        );
        assert!((t.moment(ms(t0, 600)).unwrap().position - 90.3).abs() < 1e-9);
        // After the grace the beat is believed.
        t.observe(ms(t0, 2000), true, Some(beat(1, 95.0, true)), 3, &[]);
        assert_eq!(t.moment(ms(t0, 2000)).unwrap().position, 95.0);
        // `waveformSeek(86400)` (to the end) is not a position.
        t.observe(
            ms(t0, 2100),
            true,
            Some(beat(1, 95.0, true)),
            3,
            &[event(EventType::Seek, 1, Some(86_400.0), false)],
        );
        assert!(t.moment(ms(t0, 2100)).unwrap().position < 100.0);
    }

    #[test]
    fn a_refused_command_and_a_lost_connection_are_forgotten() {
        let t0 = Instant::now();
        let mut t = PositionTracker::default();
        t.observe(t0, true, Some(beat(1, 0.0, false)), 1, &[]);
        t.observe(
            ms(t0, 100),
            true,
            Some(beat(1, 0.0, false)),
            1,
            &[event(EventType::SongStarted, 1, Some(0.0), true)],
        );
        let mut reverted = event(EventType::TransportReverted, 1, Some(0.0), false);
        reverted.playing = Some(false);
        t.observe(
            ms(t0, 4000),
            true,
            Some(beat(1, 0.0, false)),
            2,
            &[reverted],
        );
        assert!(!t.moment(ms(t0, 4100)).unwrap().playing);
        t.observe(ms(t0, 5000), false, None, 2, &[]);
        assert!(t.moment(ms(t0, 5000)).is_none());
    }
}
