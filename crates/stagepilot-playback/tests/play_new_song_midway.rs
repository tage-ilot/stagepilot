//! Jumping into the middle of a newly selected song and pressing play must switch StagePilot to
//! that song (F-H4).
//!
//! Real rig, 2026-11-08 (rc.10): in Playback the operator selected a song that was not
//! StagePilot's current song, jumped into it and pressed play. Playback sent `song.selected`,
//! `seek` and `song.resumed` (song 30049080, position 232.836). The normalizer only calls a play
//! `song.started` below 2 s, so this was a `song.resumed`, and a resume of a song other than the
//! current one was dropped: StagePilot kept the previous song and never followed the new one.
//!
//! The rule now is a deliberate difference from the Python app: a `song.resumed` of the song
//! that was just selected, when that is not StagePilot's current song, is a start at that
//! position.

use serde_json::json;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{IntegrationModes, MidiSource, PlaybackApiSettings};
use stagepilot_core::events::{new_event, EventPayload, EventType as BusEvent, StagePilotEvent};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::settings::{PersistentSettings, RuntimeSettings, SettingsService};
use stagepilot_core::state::{ApplicationState, ServicePlan, Song};
use stagepilot_core::state::{ServiceLoadStatus, TimerStatus};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_playback::mapping::{Mapper, Observation, Plan};
use stagepilot_playback::normalizer::{EventType, Heartbeat, Normalizer, PlaybackEvent};
use stagepilot_playback::plugin::{PlaybackPlugin, PluginOptions};
use stagepilot_playback::testkit::{heartbeat, FakePlayback, FakeState};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FIRST: i64 = 30050662;
const SECOND: i64 = 30049079;
const THIRD: i64 = 30049080;
const ORDER: [i64; 3] = [FIRST, SECOND, THIRD];

// ---- the wire messages of the rig ----------------------------------------------------------------

fn select(id: i64) -> String {
    json!({"setlistSelectSong": {"setlistSongID": id}}).to_string()
}

fn seek(position: f64) -> String {
    json!({"waveformSeek": {"sequenceTime": position}}).to_string()
}

fn play(playing: bool) -> String {
    json!({"transportPlay": {"playing": playing}}).to_string()
}

fn hb(song: i64, position: f64, playing: bool) -> String {
    heartbeat(&FakeState {
        song,
        position,
        playing,
        pad: false,
        version: 1,
        stalled: false,
        length: 454.7,
        songs: ORDER.to_vec(),
    })
}

fn heartbeat_of(song: i64, position: f64, playing: bool) -> Heartbeat {
    Heartbeat {
        song_id: song,
        position,
        playing,
        pad: false,
        setlist_version: Some(1),
    }
}

// ---- mapper level ----------------------------------------------------------------------------------

fn plan_state(current: Option<usize>) -> ApplicationState {
    let songs: Vec<Song> = (0..3)
        .map(|i| Song::new(&format!("s{i}"), "t", Some(500), i + 1))
        .collect();
    ApplicationState {
        plan: Some(ServicePlan {
            id: "p".into(),
            title: "t".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
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

fn event(kind: &str, song: i64, position: Option<f64>) -> PlaybackEvent {
    let mut value = json!({"type": kind, "timestamp": 1.0, "song_id": song});
    if let Some(position) = position {
        value["position"] = json!(position);
    }
    serde_json::from_value(value).unwrap()
}

/// A scanned, valid order with one heartbeat seen.
fn mapper() -> Mapper {
    let mut m = Mapper::new();
    m.install_order(&ORDER, 1, None).unwrap();
    m.observe(Some(heartbeat_of(FIRST, 0.0, false)), &[], None);
    assert!(!m.stale);
    m
}

/// Feed `events` (one batch) and decide each against `state`.
fn decide_all(
    m: &mut Mapper,
    events: &[PlaybackEvent],
    state: &ApplicationState,
) -> Vec<Option<Plan>> {
    let observed: Vec<Observation> = m.observe(None, events, None);
    observed.iter().map(|o| m.decide(o, state).plan).collect()
}

#[test]
fn resume_of_a_newly_selected_song_starts_it_at_the_third_plan_position() {
    let mut m = mapper();
    let state = plan_state(Some(0));
    let plans = decide_all(
        &mut m,
        &[
            event("song.selected", THIRD, None),
            event("seek", THIRD, Some(232.0)),
            event("song.resumed", THIRD, Some(232.836)),
        ],
        &state,
    );
    assert_eq!(plans, [None, None, Some(Plan::Position(3))]);
}

#[test]
fn resume_with_no_current_song_at_all_starts_the_selected_song() {
    let mut m = mapper();
    let plans = decide_all(
        &mut m,
        &[
            event("song.selected", SECOND, None),
            event("song.resumed", SECOND, Some(90.0)),
        ],
        &plan_state(None),
    );
    assert_eq!(plans, [None, Some(Plan::Position(2))]);
}

#[test]
fn resume_inside_the_current_song_is_unchanged() {
    // The song is selected again (it is already current), a jump and a play: a normal resume.
    let mut m = mapper();
    let plans = decide_all(
        &mut m,
        &[
            event("song.selected", FIRST, None),
            event("song.resumed", FIRST, Some(120.0)),
        ],
        &plan_state(Some(0)),
    );
    assert_eq!(plans, [None, Some(Plan::ResumeAt(120.0))]);
    // No selection at all: pause then play inside the current song.
    let plans = decide_all(
        &mut m,
        &[event("song.resumed", FIRST, Some(130.0))],
        &plan_state(Some(0)),
    );
    assert_eq!(plans, [Some(Plan::ResumeAt(130.0))]);
}

#[test]
fn resume_without_a_selection_of_that_song_is_still_ignored() {
    let mut m = mapper();
    let state = plan_state(Some(0));
    // Another song was selected, not the one that resumes.
    let plans = decide_all(
        &mut m,
        &[
            event("song.selected", SECOND, None),
            event("song.resumed", THIRD, Some(50.0)),
        ],
        &state,
    );
    assert_eq!(plans, [None, None]);
    // Nothing selected at all.
    let mut m = mapper();
    let plans = decide_all(&mut m, &[event("song.resumed", THIRD, Some(50.0))], &state);
    assert_eq!(plans, [None]);
}

#[test]
fn the_selection_is_used_up_by_the_first_play() {
    let mut m = mapper();
    let state = plan_state(Some(0));
    let plans = decide_all(
        &mut m,
        &[
            event("song.selected", THIRD, None),
            event("song.resumed", THIRD, Some(232.0)),
        ],
        &state,
    );
    assert_eq!(plans, [None, Some(Plan::Position(3))]);
    // Were StagePilot still on song 1 (the start failed), a second resume is not a new start.
    let plans = decide_all(&mut m, &[event("song.resumed", THIRD, Some(240.0))], &state);
    assert_eq!(plans, [None]);
}

#[test]
fn start_auto_advance_and_end_clear_the_selection() {
    for clearing in ["song.started", "song.changed", "song.ended"] {
        let mut m = mapper();
        let state = plan_state(Some(0));
        let plans = decide_all(
            &mut m,
            &[
                event("song.selected", THIRD, None),
                event(clearing, SECOND, None),
                event("song.resumed", THIRD, Some(60.0)),
            ],
            &state,
        );
        assert_eq!(plans.last(), Some(&None), "{clearing}");
    }
}

#[test]
fn navigation_without_play_pause_and_stop_are_unchanged() {
    let mut m = mapper();
    let state = plan_state(Some(0));
    let plans = decide_all(
        &mut m,
        &[
            event("song.selected", THIRD, None),
            event("seek", THIRD, Some(232.0)),
            event("song.paused", FIRST, Some(100.0)),
            event("song.stopped", FIRST, Some(0.0)),
        ],
        &state,
    );
    assert_eq!(
        plans,
        [None, None, Some(Plan::StopAt(100.0)), Some(Plan::Stop)]
    );
}

#[test]
fn a_resume_of_a_song_outside_the_order_is_ignored_and_logged_once() {
    let mut m = mapper();
    let state = plan_state(Some(0));
    for _ in 0..3 {
        let plans = decide_all(
            &mut m,
            &[
                event("song.selected", 999, None),
                event("song.resumed", 999, Some(30.0)),
            ],
            &state,
        );
        assert_eq!(plans, [None, None]);
    }
}

#[test]
fn a_stale_order_never_starts_a_song_from_a_resume() {
    let mut m = Mapper::new();
    m.set_learning(false);
    m.install_order(&ORDER, 1, None).unwrap(); // stale until a heartbeat validates it
    let plans = decide_all(
        &mut m,
        &[
            event("song.selected", THIRD, None),
            event("song.resumed", THIRD, Some(232.0)),
        ],
        &plan_state(Some(0)),
    );
    assert_eq!(plans, [None, None]);
}

/// The exact rig sequence through the real normalizer and mapper, scanned order.
#[test]
fn the_rig_sequence_through_normalizer_and_mapper() {
    // Without the fast transport message the play is first seen in the heartbeat (232.836).
    run_rig_sequence(false, 232.836);
    // With it, the provisional resume carries the confirmed paused position; the heartbeat that
    // follows does not repeat it.
    run_rig_sequence(true, 232.0);
}

fn run_rig_sequence(fast_play: bool, expected_position: f64) {
    let mut n = Normalizer::new(true);
    let mut m = Mapper::new();
    m.install_order(&ORDER, 1, None).unwrap();
    let state = plan_state(Some(0)); // StagePilot is on the first song
    let mut t = 5000.0;
    let mut plans = vec![];
    let mut resumed = vec![];
    // (message, seconds after the previous one, the heartbeat that message is)
    let script: Vec<(String, f64, Option<Heartbeat>)> = vec![
        (
            hb(FIRST, 306.5, false),
            0.0,
            Some(heartbeat_of(FIRST, 306.5, false)),
        ),
        (select(FIRST), 4.0, None),
        (select(SECOND), 2.0, None),
        (
            hb(SECOND, 0.0, false),
            0.1,
            Some(heartbeat_of(SECOND, 0.0, false)),
        ),
        (select(THIRD), 1.5, None),
        (
            hb(THIRD, 0.0, false),
            0.1,
            Some(heartbeat_of(THIRD, 0.0, false)),
        ),
        (seek(232.0), 3.0, None),
        (
            hb(THIRD, 232.0, false),
            0.1,
            Some(heartbeat_of(THIRD, 232.0, false)),
        ),
        // The play arrives before the heartbeat shows it (fast path), then the heartbeats follow.
        (play(true), 0.9, None),
        (
            hb(THIRD, 232.836, true),
            0.1,
            Some(heartbeat_of(THIRD, 232.836, true)),
        ),
        (
            hb(THIRD, 233.9, true),
            1.0,
            Some(heartbeat_of(THIRD, 233.9, true)),
        ),
    ];
    for (raw, after, beat) in script {
        if !fast_play && raw == play(true) {
            continue;
        }
        t += after;
        let events = n.feed(&raw, t);
        resumed.extend(
            events
                .iter()
                .filter(|e| e.kind == EventType::SongResumed)
                .map(|e| (e.song_id, e.position)),
        );
        for o in m.observe(beat, &events, None) {
            plans.push((o.event.kind, m.decide(&o, &state).plan));
        }
    }
    let starts: Vec<_> = plans.iter().filter(|(_, plan)| plan.is_some()).collect();
    assert_eq!(
        starts,
        [&(EventType::SongResumed, Some(Plan::Position(3)))],
        "{plans:?}"
    );
    assert_eq!(resumed.len(), 1, "{resumed:?}");
    assert_eq!(resumed[0].0, Some(THIRD));
    assert!(
        (resumed[0].1.unwrap() - expected_position).abs() < 1e-9,
        "{resumed:?}"
    );
}

// ---- the whole plugin: fake Playback, real event bus, state store and state service -------------

struct App {
    bus: EventBus,
    store: StateStore,
    plugin: PlaybackPlugin,
    events: Arc<Mutex<Vec<StagePilotEvent>>>,
    fake: FakePlayback,
}

fn plugin_plan() -> ServicePlan {
    ServicePlan {
        id: "plan-1".into(),
        title: "Sunday".into(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
        service_type: "Weekend".into(),
        service_type_id: Some("st-1".into()),
        service_times: vec![],
        duration_source: "test".into(),
        songs: vec![
            Song::new("a", "Alpha", Some(300), 1),
            Song::new("b", "Beta", Some(310), 2),
            Song::new("c", "Gamma", Some(454), 3),
        ],
    }
}

async fn app(learn_order: bool) -> App {
    let fake = FakePlayback::start().await;
    fake.auto_beat(Duration::from_millis(100));
    {
        let mut s = fake.state.lock().unwrap();
        s.songs = ORDER.to_vec();
        s.song = FIRST;
        s.length = 454.7;
    }
    let persistent = PersistentSettings {
        integration_modes: IntegrationModes {
            midi_source: MidiSource::PlaybackApi,
            ..IntegrationModes::default()
        },
        playback_api: PlaybackApiSettings {
            host: Some("127.0.0.1".into()),
            port: fake.port,
            auto_scan: false,
            learn_order,
            ..PlaybackApiSettings::default()
        },
        ..PersistentSettings::default()
    };
    let runtime = RuntimeSettings::from_persistent(&persistent);
    let settings = SettingsService::ephemeral(runtime.clone());
    settings.save(persistent).unwrap();
    let bus = EventBus::new();
    let store = StateStore::default();
    let service = StateService::new(
        bus.clone(),
        store.clone(),
        runtime.recent_event_limit,
        runtime.recent_error_limit,
    );
    service.start();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    bus.add_tap(move |e| sink.lock().unwrap().push(e.clone()));
    let plugin = PlaybackPlugin::new(
        bus.clone(),
        store.clone(),
        service,
        Arc::new(settings),
        PluginOptions {
            step_timeout: Duration::from_secs(2),
            ..PluginOptions::default()
        },
    );
    bus.publish(new_event(
        BusEvent::ServiceLoaded,
        "test",
        EventPayload::Service {
            plan: plugin_plan(),
        },
    ))
    .await;
    bus.publish(new_event(
        BusEvent::ServiceLoadChanged,
        "test",
        EventPayload::ServiceLoad {
            status: ServiceLoadStatus::Loaded,
            target_date: None,
            candidates: vec![],
            skipped_items: vec![],
            message: None,
            is_stale: false,
        },
    ))
    .await;
    App {
        bus,
        store,
        plugin,
        events,
        fake,
    }
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..1500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

impl App {
    fn set(&self, song: i64, position: f64, playing: bool) {
        let mut s = self.fake.state.lock().unwrap();
        s.song = song;
        s.position = position;
        s.playing = playing;
    }

    /// The operator picks `id` in Playback; wait until the plugin has seen it.
    async fn select(&self, id: i64) {
        let seen = || {
            self.plugin
                .events()
                .iter()
                .filter(|e| e.event.kind == EventType::SongSelected && e.event.song_id == Some(id))
                .count()
        };
        let before = seen();
        self.fake.send_raw(&select(id));
        until("selection observed", || seen() > before).await;
    }

    /// Every `song.started` the state service published: (plan index, position, observed_at).
    fn started(&self) -> Vec<(usize, Option<f64>, Option<chrono::DateTime<chrono::Utc>>)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.event_type == BusEvent::SongStarted)
            .filter_map(|e| match &e.payload {
                EventPayload::Song {
                    index,
                    position_seconds,
                    observed_at,
                    ..
                } => Some((*index, *position_seconds, *observed_at)),
                _ => None,
            })
            .collect()
    }

    /// Play the first song from the top and pause it at 120 s: StagePilot is on the first song.
    async fn first_song_played_and_paused(&self, plan_index: usize) {
        self.select(FIRST).await;
        self.set(FIRST, 0.5, true);
        until("first song started", || {
            self.store.snapshot().playing_song_index == Some(plan_index)
        })
        .await;
        self.set(FIRST, 120.0, false);
        until("paused", || {
            self.store.snapshot().last_action.as_deref() == Some("stop_timer")
        })
        .await;
    }

    /// select 30050662, select 30049079, select 30049080, seek 232, play at 232.836.
    async fn the_rig_sequence(&self) {
        self.select(FIRST).await;
        self.select(SECOND).await;
        self.set(SECOND, 0.0, false);
        self.select(THIRD).await;
        self.set(THIRD, 0.0, false);
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.fake.send_raw(&seek(232.0));
        self.set(THIRD, 232.0, false);
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.set(THIRD, 232.836, true); // the play, seen in the next heartbeat
    }
}

#[tokio::test]
async fn jumping_into_a_newly_selected_song_and_playing_switches_stagepilot_to_it() {
    let a = app(true).await;
    a.plugin.start().await.unwrap();
    until("connected", || a.plugin.connection().connected).await;
    a.plugin.discover_song_order().await.unwrap();
    until("order valid", || !a.plugin.mapper_stale()).await;
    a.first_song_played_and_paused(0).await;
    assert_eq!(a.store.snapshot().current_song_index, Some(0));

    a.the_rig_sequence().await;
    until("StagePilot is on the third song", || {
        a.store.snapshot().playing_song_index == Some(2)
    })
    .await;
    let state = a.store.snapshot();
    assert_eq!(
        state.current_song.as_ref().map(|s| s.id.as_str()),
        Some("c")
    );
    assert_eq!(state.current_song_index, Some(2));

    // The song event the lights, ProPresenter and the timer follow carries the jump position.
    let started = a.started();
    let (index, position, observed) = *started.last().expect("a song.started");
    assert_eq!(index, 2);
    assert_eq!(position, Some(232.836));
    // Make the timer run the way the ProPresenter plugin does on this event: song time 0 was
    // `position` seconds before the heartbeat arrived.
    let started_at = observed.unwrap() - chrono::Duration::milliseconds(232_836);
    a.bus
        .publish(new_event(
            BusEvent::TimerStarted,
            "test",
            EventPayload::Timer(stagepilot_core::events::TimerPayload {
                duration_seconds: 454,
                song: state.current_song.clone(),
                started_at: Some(started_at),
                ..Default::default()
            }),
        ))
        .await;
    until("timer running", || {
        a.store.snapshot().timer.status == TimerStatus::Running
    })
    .await;
    let timer = a.store.snapshot().timer;
    let at_heartbeat = observed.unwrap();
    let on_timer = timer.position_seconds.unwrap_or(0.0)
        + (at_heartbeat - timer.started_at.unwrap()).num_milliseconds() as f64 / 1000.0;
    assert!(
        (on_timer - 232.836).abs() < 0.01,
        "the timer is at {on_timer} when the heartbeat said 232.836"
    );
    a.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_same_jump_works_while_the_order_is_still_being_learned() {
    let a = app(true).await;
    a.plugin.start().await.unwrap();
    until("connected", || a.plugin.connection().connected).await;
    assert!(a.plugin.mapper_stale(), "no scan: the order is learned");
    a.first_song_played_and_paused(0).await;
    a.the_rig_sequence().await;
    // The first song was paused after a moment, so its binding was released and Up next is the
    // first song again: the new Playback song is placed there, at the jump position.
    until("StagePilot starts the new song", || a.started().len() == 2).await;
    let (index, position, _) = *a.started().last().unwrap();
    assert_eq!((index, position), (0, Some(232.836)));
    let learned = a.plugin.order_view().1;
    assert_eq!(
        learned
            .iter()
            .map(|l| (l.song_id, l.plan_position))
            .collect::<Vec<_>>(),
        [(THIRD, 1)]
    );
    a.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_resume_inside_the_current_song_does_not_switch_or_restart() {
    let a = app(true).await;
    a.plugin.start().await.unwrap();
    until("connected", || a.plugin.connection().connected).await;
    a.plugin.discover_song_order().await.unwrap();
    until("order valid", || !a.plugin.mapper_stale()).await;
    a.first_song_played_and_paused(0).await;
    let starts = a.started().len();
    // Jump inside the same song and play: the timer is re-based, the song is not started again.
    a.fake.send_raw(&seek(200.0));
    a.set(FIRST, 200.0, false);
    tokio::time::sleep(Duration::from_millis(250)).await;
    a.set(FIRST, 200.5, true);
    until("resumed at the jump", || {
        a.store.snapshot().timer.position_seconds == Some(200.5)
    })
    .await;
    assert_eq!(a.started().len(), starts, "no new song.started");
    assert_eq!(a.store.snapshot().current_song_index, Some(0));
    a.plugin.stop().await.unwrap();
}
