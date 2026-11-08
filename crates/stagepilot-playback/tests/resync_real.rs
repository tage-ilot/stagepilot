//! Evidence from REAL captures: the position safety net must not re-sync on Playback's own
//! section loops and jumps (monitor-only), only on a real fault.
//!
//! Every frame goes through the real normalizer. A timer model moves only the way the mapper
//! moves it (start, resume, snapshot, seek, and the net's own re-sync); pause, stop and end
//! stop it. The net's decision is the same as `plugin.rs::position_resync`: a playing
//! heartbeat of the timer's song, with no events in its batch, compared by `DriftWatch`.

use serde_json::Value;
use stagepilot_playback::normalizer::{
    parse_heartbeat, parse_message, EventType, Normalizer, PlaybackEvent,
};
use stagepilot_playback::resync::DriftWatch;

const FILES: &[(&str, &str)] = &[
    (
        "operator-25min.jsonl",
        include_str!("fixtures/real/operator-25min.jsonl"),
    ),
    (
        "operator-pad-section.jsonl",
        include_str!("fixtures/real/operator-pad-section.jsonl"),
    ),
];

#[derive(Default)]
struct Outcome {
    loop_jumps: usize,
    jump_times: Vec<f64>,
    /// (t, heartbeat position, timer position, seconds since the last non-playing heartbeat)
    resyncs: Vec<(f64, f64, f64, f64)>,
}

impl Outcome {
    /// Re-syncs within 6 s after a playing -> playing position jump: the ones this rule bans.
    fn on_loop_jumps(&self) -> Vec<&(f64, f64, f64, f64)> {
        self.resyncs
            .iter()
            .filter(|r| self.jump_times.iter().any(|j| r.0 >= *j && r.0 <= *j + 6.0))
            .collect()
    }
}

/// `suspend_on_jump = false` removes the new rule (events no longer reach the net).
fn run(body: &str, fast: bool, suspend_on_jump: bool) -> Outcome {
    let mut norm = Normalizer::new(fast);
    let mut net = DriftWatch::default();
    // (song, position at `at`, `at`)
    let mut timer: Option<(i64, f64, f64)> = None;
    let mut out = Outcome::default();
    let mut last_not_playing = f64::NEG_INFINITY;
    for line in body.lines() {
        let rec: Value = serde_json::from_str(line).unwrap();
        if rec["kind"] != "frame" {
            continue;
        }
        let t = rec["t"].as_f64().unwrap();
        let raw = serde_json::to_string(&rec["msg"]).unwrap();
        let events: Vec<PlaybackEvent> = norm.feed(&raw, t);
        if suspend_on_jump {
            net.note_events(&events);
        } else {
            // the net as first written: only starts/resumes/seeks reset its count
            let rearm: Vec<_> = events
                .iter()
                .filter(|e| e.kind != EventType::PositionJump)
                .cloned()
                .collect();
            net.note_events(&rearm);
        }
        for e in &events {
            if e.kind == EventType::PositionJump {
                out.loop_jumps += 1;
                out.jump_times.push(t);
            }
            match (e.kind, e.song_id, e.position) {
                (
                    EventType::SongStarted
                    | EventType::SongResumed
                    | EventType::StateSnapshot
                    | EventType::Seek,
                    Some(song),
                    pos,
                ) if e.playing != Some(false) => {
                    if e.kind == EventType::StateSnapshot && e.playing != Some(true) {
                        continue;
                    }
                    timer = Some((song, pos.unwrap_or(0.0), t));
                }
                (EventType::Seek, Some(song), Some(pos)) => {
                    // a seek while paused/stopped moves nothing that runs
                    if timer.is_some() {
                        timer = Some((song, pos, t));
                    }
                }
                (
                    EventType::SongPaused
                    | EventType::SongStopped
                    | EventType::SongEnded
                    | EventType::SongChanged
                    | EventType::SongSelected,
                    ..,
                ) => timer = None,
                _ => {}
            }
        }
        let Some((kind, body)) = parse_message(&raw) else {
            continue;
        };
        if kind != "heartbeat" {
            continue;
        }
        let Some(h) = parse_heartbeat(&body) else {
            continue;
        };
        if !h.playing {
            last_not_playing = t;
        }
        let comparable = h.playing && events.is_empty();
        let app = timer.and_then(|(song, pos, at)| (song == h.song_id).then_some(pos + t - at));
        let Some(app) = app.filter(|_| comparable && !net.is_suspended_for(h.song_id)) else {
            net.note_events(&[]);
            continue;
        };
        if net.check(h.position, app) {
            out.resyncs.push((t, h.position, app, t - last_not_playing));
            timer = Some((h.song_id, h.position, t));
        }
    }
    out
}

#[test]
fn the_net_makes_zero_resyncs_on_the_real_section_loops_and_jumps() {
    let mut jumps = [0usize; 2];
    for fast in [false, true] {
        for (i, (name, body)) in FILES.iter().enumerate() {
            let o = run(body, fast, true);
            assert!(
                o.on_loop_jumps().is_empty(),
                "{name} fast={fast}: net re-synced on Playback's own jump: {:?}",
                o.on_loop_jumps()
            );
            // Any re-sync that remains is a paused -> playing disagreement (the fault the
            // net is for: e.g. 25min t=1004..1006, pause, navigate to another section, play).
            for r in &o.resyncs {
                assert!(
                    r.3 <= 5.0,
                    "{name} fast={fast}: re-sync not after a pause: {r:?}"
                );
            }
            eprintln!(
                "{name} fast={fast}: {} re-syncs, all after a pause",
                o.resyncs.len()
            );
            if fast && i == 0 {
                // After 24 loops, a real pause -> other section -> play at t=1004 is corrected
                // again: the suspend ended with the resume.
                assert!(
                    o.resyncs.iter().any(|r| (1005.0..1008.0).contains(&r.0)),
                    "the net must be armed again after the loops: {:?}",
                    o.resyncs
                );
            }
            jumps[i] = o.loop_jumps;
        }
    }
    eprintln!(
        "position jumps seen: 25min={} pad-section={}",
        jumps[0], jumps[1]
    );
    assert!(jumps[0] >= 24, "25min capture lost its loops: {}", jumps[0]);
}

#[test]
fn without_the_suspend_rule_the_net_resyncs_on_the_loops() {
    // The assertion of the test above, run with the rule removed: it must NOT hold.
    let o = run(FILES[0].1, true, false);
    let bad = o.on_loop_jumps();
    eprintln!(
        "without the suspend rule: {} re-syncs on loop jumps (of {} jumps), first {:?}",
        bad.len(),
        o.loop_jumps,
        bad.first()
    );
    assert!(
        bad.len() >= 20,
        "expected the loops to trigger re-syncs: {}",
        bad.len()
    );
}
