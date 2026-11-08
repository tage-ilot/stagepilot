//! Pause -> jump (seek) -> play must resume at the jump position, whatever the latency.
//!
//! Real rig, 2026-11-08 (song 30049080, "Elohim"): the provisional `song.resumed` took the
//! position of the last heartbeat (from before the jump), so the timer, ProPresenter and the
//! lights ran 119 s off for the rest of the song. These tests replay that sequence.

use serde_json::json;
use stagepilot_playback::normalizer::{EventType, Normalizer, PlaybackEvent};
use stagepilot_playback::testkit::{heartbeat, FakeState};

const SONG: i64 = 30049080;

fn hb(position: f64, playing: bool) -> String {
    heartbeat(&FakeState {
        song: SONG,
        position,
        playing,
        pad: false,
        version: 1,
        stalled: false,
        length: 454.7,
        songs: vec![],
    })
}

fn seek(position: f64) -> String {
    json!({"waveformSeek": {"sequenceTime": position}}).to_string()
}

fn play(playing: bool) -> String {
    json!({"transportPlay": {"playing": playing}}).to_string()
}

/// A fast-transport normaliser that has seen the song playing, with a clock in seconds.
struct Rig {
    n: Normalizer,
    t: f64,
    events: Vec<PlaybackEvent>,
}

impl Rig {
    fn new(position: f64, playing: bool) -> Rig {
        let mut r = Rig {
            n: Normalizer::new(true),
            t: 1000.0,
            events: vec![],
        };
        r.feed(&hb(position, playing), 0.0);
        r
    }
    /// Feed one message `after` seconds after the previous one.
    fn feed(&mut self, raw: &str, after: f64) -> Vec<PlaybackEvent> {
        self.t += after;
        let out = self.n.feed(raw, self.t);
        self.events.extend(out.clone());
        out
    }
}

fn close(a: Option<f64>, b: f64) {
    assert!(a.is_some_and(|a| (a - b).abs() < 1e-9), "{a:?} != {b}");
}

/// The exact sequence from the rig, with the rig's gaps between messages.
#[test]
fn the_rig_sequence_resumes_at_the_jump_position() {
    // Playing around 306.5; pause pressed (fast path), then the paused heartbeats.
    let mut r = Rig::new(306.5, true);
    r.feed(&play(false), 0.5);
    r.feed(&hb(307.282, false), 0.4);
    r.feed(&hb(308.061, false), 1.0); // a late, still-paused beat: the stale position
                                      // 20:13:06.131 seek, 20:13:06.332 play (0.2 s later): before any heartbeat shows 229.253.
    r.feed(&seek(229.253), 2.0);
    let resumed = r.feed(&play(true), 0.2);
    let e = resumed
        .iter()
        .find(|e| e.kind == EventType::SongResumed)
        .unwrap();
    close(e.position, 229.253);
    // The player confirms: playing at 229.5, then 5 s later paused at 233.445.
    r.feed(&hb(229.5, true), 0.3);
    r.feed(&hb(230.5, true), 1.0);
    r.feed(&play(false), 3.7);
    r.feed(&hb(233.445, false), 0.1);
    // 20:13:13.130 seek 114.625, 20:13:13.506 play (0.376 s later).
    r.feed(&seek(114.625), 2.0);
    let resumed = r.feed(&play(true), 0.376);
    let e = resumed
        .iter()
        .find(|e| e.kind == EventType::SongResumed)
        .unwrap();
    close(e.position, 114.625);
    // Playback agrees: the settle must not emit a revert or a second resume.
    let kinds: Vec<_> = r
        .feed(&hb(114.9, true), 0.3)
        .iter()
        .map(|e| e.kind)
        .collect();
    assert!(kinds.is_empty(), "{kinds:?}");
}

#[test]
fn the_jumps_that_worked_stay_correct() {
    // 37.493 paused -> seek 182.688 -> resumed 182.688, 0.855 s later.
    let mut r = Rig::new(36.0, true);
    r.feed(&play(false), 0.5);
    r.feed(&hb(37.493, false), 0.4);
    r.feed(&seek(182.688), 2.5);
    r.feed(&hb(182.688, false), 0.4); // the heartbeat did arrive this time
    let resumed = r.feed(&play(true), 0.05);
    close(
        resumed
            .iter()
            .find(|e| e.kind == EventType::SongResumed)
            .unwrap()
            .position,
        182.688,
    );
    // 204.917 paused -> seek 304.477 -> resumed 304.477, 0.459 s later.
    let mut r = Rig::new(203.0, true);
    r.feed(&play(false), 0.5);
    r.feed(&hb(204.917, false), 0.4);
    r.feed(&seek(304.477), 2.8);
    r.feed(&hb(304.477, false), 0.3);
    let resumed = r.feed(&play(true), 0.459);
    close(
        resumed
            .iter()
            .find(|e| e.kind == EventType::SongResumed)
            .unwrap()
            .position,
        304.477,
    );
}

#[test]
fn a_second_seek_while_paused_wins() {
    let mut r = Rig::new(100.0, false);
    r.feed(&seek(50.0), 1.0);
    r.feed(&seek(75.0), 0.3);
    let resumed = r.feed(&play(true), 0.1);
    close(resumed[0].position, 75.0);
}

#[test]
fn a_seek_while_stopped_then_play_resumes_there_and_a_seek_to_zero_starts() {
    let mut r = Rig::new(0.0, false);
    r.feed(&seek(120.0), 1.0);
    let e = &r.feed(&play(true), 0.1)[0];
    assert_eq!(e.kind, EventType::SongResumed);
    close(e.position, 120.0);

    let mut r = Rig::new(90.0, false);
    r.feed(&seek(0.0), 1.0);
    let e = &r.feed(&play(true), 0.1)[0];
    assert_eq!(e.kind, EventType::SongStarted);
    close(e.position, 0.0);
}

#[test]
fn a_seek_while_playing_is_reported_and_a_later_pause_uses_the_heartbeat() {
    let mut r = Rig::new(10.0, true);
    let s = r.feed(&seek(200.0), 1.0);
    assert_eq!(s[0].kind, EventType::Seek);
    close(s[0].position, 200.0);
    r.feed(&hb(200.4, true), 0.4);
    let paused = r.feed(&play(false), 0.6);
    // A seek made while playing never replaces the pause position.
    close(paused[0].position, 200.4);
}

#[test]
fn a_play_that_beats_the_seek_message_is_followed_by_the_seek_event() {
    let mut r = Rig::new(307.282, false);
    let resumed = r.feed(&play(true), 0.05);
    close(resumed[0].position, 307.282); // nothing better is known yet
    let s = r.feed(&seek(229.253), 0.05);
    assert_eq!(s[0].kind, EventType::Seek); // the mapper turns this into a timer seek
    close(s[0].position, 229.253);
}

#[test]
fn a_heartbeat_sent_before_the_seek_does_not_undo_it_but_an_old_seek_does_not_stick() {
    let mut r = Rig::new(307.0, false);
    r.feed(&seek(229.253), 1.0);
    r.feed(&hb(307.0, false), 0.3); // in flight before the jump
    let e = &r.feed(&play(true), 0.1)[0];
    close(e.position, 229.253);

    // The player never took the seek: well after it, the heartbeat decides.
    let mut r = Rig::new(307.0, false);
    r.feed(&seek(229.253), 1.0);
    r.feed(&hb(307.0, false), 4.0);
    let e = &r.feed(&play(true), 0.1)[0];
    close(e.position, 307.0);
}

#[test]
fn a_heartbeat_that_confirms_the_seek_retires_it() {
    let mut r = Rig::new(307.0, false);
    r.feed(&seek(229.253), 1.0);
    r.feed(&hb(229.253, false), 0.3);
    r.feed(&hb(240.0, false), 0.3); // moved on by other means
    let e = &r.feed(&play(true), 0.1)[0];
    close(e.position, 240.0);
}
