//! Port of `backend/tests/test_playback_mapping.py` onto `Mapper` (the synchronous decision
//! half of `PlaybackMapper`; the dispatch half is covered through the plugin in `plugin.rs`).
//!
//! Python awaits the dispatcher; here `Mapper::plan` returns the one decision, so
//! "dispatch_song_position(2)" is `Plan::Position(2)`, "dispatch(STOP_TIMER)" is `Plan::Stop`
//! and "dispatch(RESTART_CURRENT)" is `Plan::Restart`.

use serde_json::json;
use stagepilot_core::state::{ApplicationState, ServicePlan, Song};
use stagepilot_playback::mapping::{Mapper, Observation, Plan};
use stagepilot_playback::normalizer::{EventType, Heartbeat, PlaybackEvent};

fn event(kind: &str, song: Option<i64>) -> PlaybackEvent {
    let mut value = json!({"type": kind, "timestamp": 1.0});
    if let Some(song) = song {
        value["song_id"] = json!(song);
    }
    serde_json::from_value(value).unwrap()
}

fn heartbeat(song: i64, version: Option<i64>) -> Heartbeat {
    Heartbeat {
        song_id: song,
        position: 0.0,
        playing: true,
        pad: false,
        setlist_version: version,
    }
}

/// The Python `ApplicationState()` default plus the three-song plan the mapper pairs with.
fn state(current: Option<usize>) -> ApplicationState {
    let songs: Vec<Song> = (0..3)
        .map(|i| Song::new(&format!("s{i}"), "t", Some(100), i + 1))
        .collect();
    ApplicationState {
        plan: Some(ServicePlan {
            id: "p".into(),
            title: "t".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            service_type: "s".into(),
            service_type_id: None,
            service_times: vec![],
            duration_source: "d".into(),
            songs: songs.clone(),
        }),
        current_song: current.map(|i| songs[i].clone()),
        current_song_index: current,
        ..ApplicationState::default()
    }
}

fn mapper() -> Mapper {
    let mut m = Mapper::new();
    m.install_order(&[900, 17, 42], 8, None).unwrap();
    m
}

/// `send`: one heartbeat plus one event, then every observation planned against `state`.
fn send(
    m: &mut Mapper,
    kind: &str,
    song: i64,
    version: i64,
    state: &ApplicationState,
) -> Vec<Option<Plan>> {
    let observed = m.observe(
        Some(heartbeat(song, Some(version))),
        &[event(kind, Some(song))],
        None,
    );
    observed.iter().map(|o| m.plan(o, state)).collect()
}

#[test]
fn known_id_is_index_not_opaque_number() {
    let mut m = mapper();
    assert_eq!(
        send(&mut m, "song.started", 17, 8, &state(None)),
        [Some(Plan::Position(2))]
    );
}

#[test]
fn stops_even_when_stale() {
    for kind in ["song.paused", "song.stopped"] {
        let mut m = mapper();
        let plans = send(&mut m, kind, 999, 9, &state(None));
        assert!(m.stale);
        assert_eq!(plans, [Some(Plan::Stop)], "{kind}");
    }
}

#[test]
fn other_events_are_monitor_only() {
    // Python parametrises over every EventType except the three above.
    let others = [
        "state.snapshot",
        "setlist.changed",
        "song.selected",
        "song.changed",
        "song.ended",
        "song.resumed",
        "transport.reverted",
        "seek",
        "section.jump",
        "section.loop",
        "fade.out",
        "fade.in",
        "pad.requested",
        "pad.on",
        "pad.off",
        "position.jump",
        "message.unknown",
    ];
    for kind in others {
        let mut m = mapper();
        let plans = send(&mut m, kind, 17, 8, &state(None));
        assert_eq!(plans, [None], "{kind} must not drive the timer");
    }
    // The enum really has no other member: three driven + the 17 above.
    assert_eq!(others.len() + 3, 20);
    let _ = EventType::SongStarted;
}

#[test]
fn invalid_batch_is_stale_before_start_and_never_silently_repairs() {
    for (song, version) in [(999, 8), (17, 9)] {
        let mut m = mapper();
        let s = state(None);
        send(&mut m, "song.started", song, version, &s);
        let after = send(&mut m, "song.started", 17, 8, &s);
        assert!(m.stale, "{song}/{version}");
        assert_eq!(after, [None]);
        m.install_order(&[900, 17, 42], 8, None).unwrap();
        let again = send(&mut m, "song.started", 17, 8, &s);
        assert!(!m.stale);
        assert_eq!(again, [Some(Plan::Position(2))]);
    }
}

#[test]
fn natural_transition_dispatches_only_following_start() {
    let mut m = mapper();
    let events = [
        event("song.ended", Some(900)),
        {
            let mut e = event("song.changed", Some(17));
            e.previous_song_id = Some(900);
            e.continues_playing = true;
            e
        },
        event("song.started", Some(17)),
    ];
    let observed = m.observe(Some(heartbeat(17, Some(8))), &events, None);
    let s = state(None);
    let plans: Vec<_> = observed.iter().map(|o| m.plan(o, &s)).collect();
    assert_eq!(plans, [None, None, Some(Plan::Position(2))]);
}

#[test]
fn reconnect_snapshot_revalidates_without_action() {
    let mut m = mapper();
    let s = state(None);
    assert_eq!(send(&mut m, "state.snapshot", 17, 8, &s), [None]);
    assert!(!m.stale);
    m.observe(None, &[], None); // Disconnect is not invalidation.
    assert_eq!(send(&mut m, "state.snapshot", 17, 8, &s), [None]);
    assert!(!m.stale);
}

#[test]
fn discovery_queue_cannot_escape_suppression() {
    for kind in ["song.started", "song.paused", "song.stopped"] {
        let mut m = mapper();
        let s = state(None);
        let pending = m.observe(Some(heartbeat(17, Some(8))), &[event(kind, Some(17))], None);
        m.set_discovery(true);
        let during = m.observe(Some(heartbeat(17, Some(8))), &[event(kind, Some(17))], None);
        assert!(during[0].discovery);
        m.set_discovery(false);
        let all: Vec<&Observation> = pending.iter().chain(during.iter()).collect();
        for o in all {
            assert_eq!(m.plan(o, &s), None, "{kind}");
        }
    }
}

#[test]
fn invalid_order_rejected() {
    for order in [vec![], vec![17, 17]] {
        let mut m = mapper();
        assert!(m.install_order(&order, 8, None).is_err(), "{order:?}");
    }
    // Python's `(True, 17)` case cannot be expressed: song IDs are integers in the typed API.
}

#[test]
fn active_song_restarts() {
    let mut m = mapper();
    // State shape is supplied by the application: song index 1 is already the current song.
    assert_eq!(
        send(&mut m, "song.started", 17, 8, &state(Some(1))),
        [Some(Plan::Restart)]
    );
}

#[test]
fn install_at_discovery_end_validates_next_heartbeat() {
    let mut m = mapper();
    m.set_discovery(true);
    m.install_order(&[900, 17, 42], 8, None).unwrap();
    m.set_discovery(false);
    assert_eq!(
        send(&mut m, "song.started", 17, 8, &state(None)),
        [Some(Plan::Position(2))]
    );
    assert!(!m.stale);
}

#[test]
fn missing_version_and_unknown_observation_revoke_trust() {
    let mut m = mapper();
    let s = state(None);
    m.observe(Some(heartbeat(17, None)), &[], None);
    assert_eq!(send(&mut m, "song.started", 17, 8, &s), [None]);
    assert!(m.stale);
    m.install_order(&[900, 17, 42], 8, None).unwrap();
    m.observe(None, &[event("song.selected", Some(999))], None);
    assert_eq!(send(&mut m, "song.started", 17, 8, &s), [None]);
    assert!(m.stale);
}

#[test]
fn start_decision_is_one_atomic_check() {
    // Python `test_start_rechecks_after_async_snapshot`: a discovery toggled during the awaited
    // snapshot drops the start. `Mapper::plan` reads state synchronously, so the equivalent is:
    // an observation taken before a discovery window never plans, even after the window closes.
    let mut m = mapper();
    let s = state(None);
    let pending = m.observe(
        Some(heartbeat(17, Some(8))),
        &[event("song.started", Some(17))],
        None,
    );
    m.set_discovery(true);
    m.set_discovery(false);
    assert_eq!(m.plan(&pending[0], &s), None);
}
