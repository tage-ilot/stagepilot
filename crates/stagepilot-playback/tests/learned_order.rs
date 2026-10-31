//! Learned song order (no scan): the Normalizer + Mapper working together on real captures and on
//! a synthetic full service. A small `Sim` plays the part of the plugin + `StateService`: it feeds
//! the mapper, then applies each decision to a copy of the application state the same way
//! (`start_song_position` selects the song and moves Up next after it; `restore_up_next` puts it
//! back), and records the calls it would have made.

use serde_json::{json, Value};
use stagepilot_core::state::{ApplicationState, ServicePlan, Song};
use stagepilot_playback::mapping::OrderMode;
use stagepilot_playback::mapping::{Mapper, Plan};
use stagepilot_playback::normalizer::{Heartbeat, Normalizer, PlaybackEvent, SetlistId};

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Position(i64),
    Restart,
    Stop,
    StopAt(f64),
    ResumeAt(f64),
    SeekTo(f64),
    RestoreUpNext(Option<usize>),
}

struct Sim {
    mapper: Mapper,
    state: ApplicationState,
    calls: Vec<Call>,
}

fn plan_of(songs: usize, seconds: u32) -> ServicePlan {
    ServicePlan {
        id: "plan".into(),
        title: "t".into(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        service_type: "s".into(),
        service_type_id: None,
        service_times: vec![],
        duration_source: "d".into(),
        songs: (0..songs as u32)
            .map(|i| Song::new(&format!("s{i}"), "t", Some(seconds), i + 1))
            .collect(),
    }
}

impl Sim {
    /// A freshly loaded plan: nothing played, Up next is the first song.
    fn new(songs: usize, seconds: u32) -> Self {
        let plan = plan_of(songs, seconds);
        Sim {
            mapper: Mapper::new(),
            state: ApplicationState {
                up_next_song: plan.songs.first().cloned(),
                plan: Some(plan),
                ..ApplicationState::default()
            },
            calls: vec![],
        }
    }

    fn songs(&self) -> Vec<Song> {
        self.state.plan.as_ref().unwrap().songs.clone()
    }

    fn set_up_next(&mut self, index: Option<usize>) {
        let songs = self.songs();
        self.state.up_next_song = index.and_then(|i| songs.get(i).cloned());
    }

    fn step(
        &mut self,
        heartbeat: Option<Heartbeat>,
        events: &[PlaybackEvent],
        setlist: Option<&SetlistId>,
    ) {
        for o in self.mapper.observe(heartbeat, events, setlist) {
            let d = self.mapper.decide(&o, &self.state);
            if let Some(index) = d.restore_up_next {
                self.calls.push(Call::RestoreUpNext(index));
                self.set_up_next(index);
            }
            let Some(plan) = d.plan else { continue };
            let songs = self.songs();
            let call = match plan {
                Plan::Stop => Call::Stop,
                Plan::StopAt(p) => Call::StopAt(p),
                Plan::Restart => Call::Restart,
                Plan::Position(n) => {
                    let i = n as usize - 1;
                    self.state.current_song = Some(songs[i].clone());
                    self.state.current_song_index = Some(i);
                    self.set_up_next((i + 1 < songs.len()).then_some(i + 1));
                    Call::Position(n)
                }
                Plan::ResumeAt(p) => Call::ResumeAt(p),
                Plan::SeekTo(p) => Call::SeekTo(p),
            };
            self.calls.push(call);
        }
    }

    fn learned(&self) -> Vec<(i64, i64, bool)> {
        self.mapper
            .learned_songs()
            .iter()
            .map(|l| (l.song_id, l.plan_position, l.confirmed))
            .collect()
    }
}

// ---- real captures ------------------------------------------------------------------------

const CAPTURES: &[(&str, &str)] = &[
    (
        "operator-25min",
        include_str!("fixtures/real/operator-25min.jsonl"),
    ),
    (
        "operator-pad-off",
        include_str!("fixtures/real/operator-pad-off.jsonl"),
    ),
    (
        "operator-pad-section",
        include_str!("fixtures/real/operator-pad-section.jsonl"),
    ),
];

/// Seconds each Playback song id spent playing in a capture (consecutive playing heartbeats
/// less than 5 s apart), independent of the mapper.
fn played_seconds(body: &str) -> std::collections::BTreeMap<i64, f64> {
    let mut norm = Normalizer::new(true);
    let mut out = std::collections::BTreeMap::new();
    let mut last: Option<Heartbeat> = None;
    for line in body.lines() {
        let rec: Value = serde_json::from_str(line).unwrap();
        if rec["kind"] != "frame" {
            continue;
        }
        let serial = norm.heartbeat_serial;
        norm.feed(
            &serde_json::to_string(&rec["msg"]).unwrap(),
            rec["t"].as_f64().unwrap(),
        );
        if norm.heartbeat_serial != serial {
            let hb = norm.heartbeat.unwrap();
            if let Some(p) = last {
                let step = hb.position - p.position;
                if p.playing && hb.playing && p.song_id == hb.song_id && step > 0.0 && step <= 5.0 {
                    *out.entry(hb.song_id).or_insert(0.0) += step;
                }
            }
            last = Some(hb);
        }
    }
    out
}

/// What the real operator captures bind with a 6-song synthetic plan and nothing scanned.
///
/// The operator jumps between songs freely (`operator-25min`: 23 selections, 0 Next/Previous),
/// so only the learning rules apply, and only Up next and the 10 s rule decide:
/// * a song that never played 10 s (the 1.3 s / 0 s / 0.1 s accidental plays) is never kept;
/// * a song is placed at the plan's Up next at the moment it starts, never by its number;
/// * the run never panics, never holds two ids at one position, and at most one binding is
///   unconfirmed (the song playing right now).
#[test]
fn real_operator_captures_replay_without_panics_or_wasted_slots() {
    for (name, body) in CAPTURES {
        let played = played_seconds(body);
        let mut norm = Normalizer::new(true);
        let mut sim = Sim::new(6, 200);
        let mut serial = 0;
        for line in body.lines() {
            let rec: Value = serde_json::from_str(line).unwrap();
            if rec["kind"] != "frame" {
                continue;
            }
            let raw = serde_json::to_string(&rec["msg"]).unwrap();
            let events = norm.feed(&raw, rec["t"].as_f64().unwrap());
            let hb = (norm.heartbeat_serial != serial)
                .then_some(norm.heartbeat)
                .flatten();
            serial = norm.heartbeat_serial;
            sim.step(hb, &events, norm.setlist_id.as_ref());
        }
        let learned = sim.mapper.learned_songs();
        assert!(learned.len() <= 6, "{name}: more bindings than plan songs");
        let mut positions: Vec<_> = learned.iter().map(|l| l.plan_position).collect();
        positions.dedup();
        assert_eq!(
            positions.len(),
            learned.len(),
            "{name}: one id per position"
        );
        assert!(
            learned.iter().filter(|l| !l.confirmed).count() <= 1,
            "{name}: only the song playing now may be unconfirmed"
        );
        for l in learned.iter().filter(|l| l.confirmed) {
            assert!(
                played.get(&l.song_id).copied().unwrap_or(0.0) >= 10.0,
                "{name}: a kept binding played at least 10 s somewhere in the capture"
            );
        }
        eprintln!(
            "{name}: {} bindings {:?}; calls {}",
            learned.len(),
            learned
                .iter()
                .map(|l| (l.plan_position, l.confirmed))
                .collect::<Vec<_>>(),
            sim.calls.len()
        );
        if *name == "operator-25min" {
            // Four accidental plays (each bound, then released with Up next put back), the real
            // play of the first song (kept; its pause at 278 s freezes exactly there), one more
            // blip released.
            let restores = sim
                .calls
                .iter()
                .filter(|c| matches!(c, Call::RestoreUpNext(_)))
                .count();
            assert_eq!(restores, 5, "{:?}", sim.calls);
            assert!(sim.calls.contains(&Call::StopAt(277.984)));
            assert_eq!(sim.learned(), [(sim.learned()[0].0, 1, true)]);
        }
    }
}

// ---- a synthetic full service (fake ids) ----------------------------------------------------

struct Show {
    norm: Normalizer,
    sim: Sim,
    t: f64,
    serial: u64,
    version: i64,
}

impl Show {
    fn new() -> Self {
        Show {
            norm: Normalizer::new(false),
            sim: Sim::new(5, 200),
            t: 0.0,
            serial: 0,
            version: 3,
        }
    }

    fn send(&mut self, msg: Value) {
        self.t += 1.0;
        let events = self.norm.feed(&msg.to_string(), self.t);
        let hb = (self.norm.heartbeat_serial != self.serial)
            .then_some(self.norm.heartbeat)
            .flatten();
        self.serial = self.norm.heartbeat_serial;
        self.sim.step(hb, &events, self.norm.setlist_id.as_ref());
    }

    fn beat(&mut self, song: i64, position: f64, playing: bool) {
        let state = |p: bool| {
            if p {
                json!({"playing": {}})
            } else {
                json!({"stopped": {}})
            }
        };
        self.send(json!({"heartbeat": {"stateData": {
            "setlistSongID": song, "sequenceTime": position,
            "setlistCloudVersion": self.version, "setlistState": {"ready": {}},
            "sequencerPlayState": state(playing), "padPlayerPlayState": state(false)}}}));
    }

    fn setlist(&mut self, id: i64) {
        self.send(json!({"contentLoadSetlist": {"setlistData": {"setlistID": id}}}));
    }

    /// One heartbeat a second while `song` plays from `from` to `to`.
    fn play(&mut self, song: i64, from: u32, to: u32) {
        for p in from..=to {
            self.beat(song, f64::from(p), true);
        }
    }
}

#[test]
fn a_full_service_learned_then_scanned() {
    let mut s = Show::new();

    // Cold start: connected, plan loaded, no scan, nothing playing. No countdown yet.
    s.beat(9101, 0.0, false);
    s.setlist(8_000_001);
    s.beat(9101, 0.0, false);
    assert_eq!(s.sim.mapper.order_mode(), OrderMode::None);
    assert!(s.sim.calls.is_empty());

    // Song 1 starts: it takes the plan's Up next (position 1) and is kept after 10 s.
    s.play(9101, 0, 5);
    assert_eq!(s.sim.calls, [Call::Position(1)]);
    assert_eq!(s.sim.learned(), [(9101, 1, false)], "tentative at 5 s");
    s.play(9101, 6, 14);
    assert_eq!(s.sim.learned(), [(9101, 1, true)]);
    assert_eq!(s.sim.mapper.order_mode(), OrderMode::Learned);

    // Pause, resume and seek use the learned binding exactly like a scanned one.
    s.beat(9101, 14.0, false);
    s.beat(9101, 14.0, true);
    s.send(json!({"waveformSeek": {"sequenceTime": 60.0}}));
    s.play(9101, 60, 200);
    assert_eq!(
        s.sim.calls[1..],
        [Call::StopAt(14.0), Call::ResumeAt(14.0), Call::SeekTo(60.0)]
    );

    // Songs 2 and 3 follow by auto-advance: single-step evidence, not Up next.
    s.play(9102, 0, 12);
    assert_eq!(s.sim.calls.last(), Some(&Call::Position(2)));
    s.play(9102, 13, 200);
    s.play(9103, 0, 12);
    assert_eq!(s.sim.calls.last(), Some(&Call::Position(3)));
    assert_eq!(
        s.sim.learned(),
        [(9101, 1, true), (9102, 2, true), (9103, 3, true)]
    );
    assert!(
        s.sim.mapper.learned_songs().iter().all(|l| !l.doubtful),
        "Playback lengths matched the plan"
    );

    // An accidental play of the wrong song: pause on song 3, select 9105, 2 s of play, stop.
    s.beat(9103, 30.0, false);
    s.send(json!({"setlistSelectSong": {"setlistSongID": 9105}}));
    s.beat(9105, 0.0, false);
    let before_blip = s.sim.calls.len();
    s.play(9105, 0, 2);
    s.beat(9105, 2.0, false);
    assert_eq!(
        s.sim.calls[before_blip..],
        [Call::Position(4), Call::RestoreUpNext(Some(3)), Call::Stop],
        "the blip is released and Up next goes back to song 4"
    );
    assert_eq!(s.sim.learned().len(), 3, "the blip kept no slot");

    // The real song 4 then takes the freed position.
    s.send(json!({"setlistSelectSong": {"setlistSongID": 9104}}));
    s.beat(9104, 0.0, false);
    s.play(9104, 0, 12);
    assert_eq!(s.sim.learned()[3], (9104, 4, true));

    // The Playback setlist is reloaded mid-service: everything learned is dropped.
    s.version = 4;
    s.beat(9201, 0.0, false);
    s.setlist(8_000_002);
    s.beat(9201, 0.0, false);
    assert!(s.sim.learned().is_empty());
    assert_eq!(s.sim.mapper.order_mode(), OrderMode::None);
    s.sim.set_up_next(Some(0)); // the operator sets Up next to song 1 again
    s.play(9201, 0, 11);
    assert_eq!(s.sim.learned(), [(9201, 1, true)]);

    // A scan finishes: the scanned order replaces the learned one and drives from then on.
    s.sim
        .mapper
        .install_order(
            &[9201, 9202, 9203, 9204, 9205],
            4,
            Some(SetlistId::Int(8_000_002)),
        )
        .unwrap();
    assert!(s.sim.learned().is_empty());
    s.beat(9201, 12.0, true);
    assert_eq!(s.sim.mapper.order_mode(), OrderMode::Scanned);
    let before = s.sim.calls.len();
    s.sim.set_up_next(Some(0)); // would anchor 9203 at 1 if it were still learning
    s.beat(9203, 0.0, false);
    s.play(9203, 0, 3);
    assert_eq!(s.sim.calls[before..], [Call::Position(3)]);
    assert!(s.sim.learned().is_empty());
}
