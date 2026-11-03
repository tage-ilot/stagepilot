//! The song-lighting timeline as the one replay engine: look rows (several OSC messages at one
//! time), the early lead, the re-sync on start/resume/seek, hold on pause/stop/end and the
//! release triggers. Time is tokio's paused clock, so the 400-row song runs in no real time.

use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{
    LightingMethod, LightkeySettings, LightsSettings, OscArgument, OscCueSource, OscLightingCue,
    OscMessage, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand, FixtureState};
use stagepilot_lights::lightkey_link::{ArmedStore, LinkOptions, MemoryArmedStore};
use stagepilot_lights::lightkey_looks::{look_rows, replace_captured, LookRecorder};
use stagepilot_lights::plugin::LOOK_LEAD_SECONDS;
use stagepilot_lights::{LightsOptions, LightsPlugin};
use stagepilot_osc::{OscBackend, OscOutput, OutputError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

type Log = Arc<Mutex<Vec<(Instant, String, Vec<OscArgument>)>>>;

struct TimedOutput(Log);
impl OscOutput for TimedOutput {
    fn closed(&self) -> bool {
        false
    }
    fn send_message(&self, address: &str, args: &[OscArgument]) -> Result<(), OutputError> {
        self.0
            .lock()
            .unwrap()
            .push((Instant::now(), address.into(), args.to_vec()));
        Ok(())
    }
    fn close(&self) {}
}
struct TimedBackend(Log);
impl OscBackend for TimedBackend {
    fn open_output(&self, _: &str, _: u16) -> Result<Arc<dyn OscOutput>, OutputError> {
        Ok(Arc::new(TimedOutput(self.0.clone())))
    }
}

fn dim(name: &str) -> String {
    format!("/fixture/{name}/overrides/dimmer")
}
fn clear(name: &str) -> String {
    format!("/fixture/{name}/overrides/clear")
}
fn f(x: f64) -> Vec<OscArgument> {
    vec![OscArgument::Float(x)]
}
fn msg(address: &str, x: f64) -> OscMessage {
    OscMessage {
        address: address.into(),
        args: f(x),
    }
}
fn look(at: f64, messages: Vec<OscMessage>) -> OscLightingCue {
    OscLightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        address: String::new(),
        args: vec![],
        label: String::new(),
        color: None,
        messages,
        source: OscCueSource::Captured,
        edited: false,
    }
}
fn plain(at: f64, address: &str) -> OscLightingCue {
    OscLightingCue {
        address: address.into(),
        messages: vec![],
        source: OscCueSource::Manual,
        edited: false,
        ..look(at, vec![])
    }
}
fn song(key: &str) -> Song {
    Song {
        id: format!("item-{key}"),
        title: key.into(),
        duration_seconds: Some(300),
        order: 1,
        service_sequence: None,
        is_generic: false,
        source_song_id: Some(key.into()),
        description: None,
        formatted_duration: None,
    }
}
fn settings(songs: Vec<(&str, Vec<OscLightingCue>)>) -> LightsSettings {
    let mut cue_maps = BTreeMap::new();
    for (key, osc_cues) in songs {
        cue_maps.insert(
            key.to_string(),
            SongLightingCueMap {
                song_key: key.into(),
                song_title: key.into(),
                cues: vec![],
                osc_cues,
                loudness: None,
                captured_edited: false,
            },
        );
    }
    LightsSettings {
        enabled: true,
        method: LightingMethod::Osc,
        osc_host: "127.0.0.1".into(),
        osc_port: 9000,
        cue_maps,
        lightkey: LightkeySettings {
            api_url: "http://127.0.0.1:9".into(),
            replay: true,
            ..LightkeySettings::default()
        },
        ..LightsSettings::default()
    }
}

struct Rig {
    bus: EventBus,
    plugin: LightsPlugin,
    log: Log,
    t0: std::cell::Cell<Instant>,
}

async fn rig_with_store(lights: LightsSettings, store: Option<Arc<MemoryArmedStore>>) -> Rig {
    let bus = EventBus::new();
    let log: Log = Arc::default();
    let mut options = LightsOptions::new(Arc::new(|| {
        Err(stagepilot_midi::MidiError::public("no MIDI in this test"))
    }));
    let backend_log = log.clone();
    options.osc_backend_factory = Arc::new(move || Arc::new(TimedBackend(backend_log.clone())));
    options.lightkey = LinkOptions {
        armed_store: store.map_or_else(
            || Arc::new(MemoryArmedStore::new()) as Arc<dyn ArmedStore>,
            |s| s as Arc<dyn ArmedStore>,
        ),
        ..LinkOptions::default()
    };
    let plugin = LightsPlugin::new(bus.clone(), lights, options).unwrap();
    plugin.start().await.unwrap();
    Rig {
        bus,
        plugin,
        log,
        t0: std::cell::Cell::new(Instant::now()),
    }
}

async fn rig(lights: LightsSettings) -> Rig {
    rig_with_store(lights, None).await
}

impl Rig {
    async fn publish(&self, kind: EventType, payload: TimerPayload) {
        self.bus
            .publish(new_event(kind, "t", EventPayload::Timer(payload)))
            .await;
    }

    async fn start(&self, key: &str) {
        self.bus
            .publish(new_event(
                EventType::SongStarted,
                "t",
                EventPayload::Song {
                    song: song(key),
                    index: 0,
                },
            ))
            .await;
        self.publish(
            EventType::TimerStarted,
            TimerPayload {
                duration_seconds: 300,
                song: Some(song(key)),
                running: true,
                ..Default::default()
            },
        )
        .await;
        self.t0.set(Instant::now());
    }

    async fn at(&self, kind: EventType, key: &str, position: f64, running: bool) {
        self.publish(
            kind,
            TimerPayload {
                duration_seconds: 300 - position as u32,
                song: Some(song(key)),
                position_seconds: Some(position),
                remaining_seconds: Some(300 - position as u32),
                running,
                ..Default::default()
            },
        )
        .await;
    }

    /// Everything sent so far as `(seconds since the song started, address)`.
    fn sent(&self) -> Vec<(f64, String)> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(t, a, _)| ((*t - self.t0.get()).as_secs_f64(), a.clone()))
            .collect()
    }

    fn addresses(&self) -> Vec<String> {
        self.sent().into_iter().map(|(_, a)| a).collect()
    }

    fn clears(&self) -> Vec<String> {
        self.addresses()
            .into_iter()
            .filter(|a| a.ends_with("/overrides/clear"))
            .collect()
    }

    fn forget(&self) {
        self.log.lock().unwrap().clear();
    }
}

async fn run_for(seconds: f64) {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.02
}

// ------------------------------------------------------------------------------ the scheduler

#[tokio::test(start_paused = true)]
async fn a_look_row_sends_every_message_once_and_early() {
    let row = look(
        2.0,
        vec![
            msg(&dim("A"), 0.1),
            msg(&dim("B"), 0.2),
            msg(&dim("C"), 0.3),
        ],
    );
    let r = rig(settings(vec![("s", vec![row])])).await;
    r.start("s").await;
    run_for(3.0).await;
    let sent = r.sent();
    assert_eq!(
        sent.iter().map(|(_, a)| a.clone()).collect::<Vec<_>>(),
        [dim("A"), dim("B"), dim("C")],
        "all messages, once, in order"
    );
    let first = sent[0].0;
    assert!(
        close(first, 2.0 - LOOK_LEAD_SECONDS),
        "left at {first}, wanted about {}",
        2.0 - LOOK_LEAD_SECONDS
    );
    assert!(
        sent[2].0 - first <= 0.02,
        "one row goes out together, paced"
    );
}

#[tokio::test(start_paused = true)]
async fn a_plain_row_still_sends_its_single_address_exactly_on_time() {
    let r = rig(settings(vec![("s", vec![plain(2.0, "/live/cue/1")])])).await;
    r.start("s").await;
    run_for(3.0).await;
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "/live/cue/1");
    assert!(
        close(sent[0].0, 2.0),
        "no lead for a non-fixture cue: {}",
        sent[0].0
    );
}

#[tokio::test(start_paused = true)]
async fn resuming_mid_song_re_syncs_the_latest_message_per_address_then_continues() {
    let rows = vec![
        look(1.0, vec![msg(&dim("A"), 0.1)]),
        look(2.0, vec![msg(&dim("A"), 0.5), msg(&dim("B"), 0.2)]),
        plain(3.0, "/live/cue/old"),
        plain(8.0, "/live/cue/later"),
        look(10.0, vec![msg(&dim("A"), 1.0)]),
    ];
    let r = rig(settings(vec![("s", rows)])).await;
    r.start("s").await;
    r.at(EventType::TimerStopRequested, "s", 4.5, false).await;
    r.forget();
    r.at(EventType::TimerResumed, "s", 5.0, true).await;
    r.t0.set(Instant::now() - Duration::from_secs(5)); // song time 5.0 is now
    run_for(0.5).await;
    let now = r.sent();
    let args = r
        .log
        .lock()
        .unwrap()
        .iter()
        .map(|(_, a, v)| (a.clone(), v.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [(dim("A"), f(0.5)), (dim("B"), f(0.2))],
        "latest per address, once, and not the superseded 0.1: {now:?}"
    );
    run_for(10.0).await;
    let later = r.sent();
    let addresses: Vec<_> = later.iter().map(|(_, a)| a.as_str()).collect();
    assert_eq!(
        addresses,
        [
            dim("A").as_str(),
            dim("B").as_str(),
            "/live/cue/later",
            dim("A").as_str()
        ]
    );
    assert!(close(later[2].0, 8.0), "other cues keep their normal time");
    assert!(close(later[3].0, 10.0 - LOOK_LEAD_SECONDS));
}

#[tokio::test(start_paused = true)]
async fn seeking_back_re_syncs_to_the_state_at_the_new_position() {
    let rows = vec![
        look(1.0, vec![msg(&dim("A"), 0.1)]),
        look(20.0, vec![msg(&dim("A"), 0.9)]),
    ];
    let r = rig(settings(vec![("s", rows)])).await;
    r.start("s").await;
    r.at(EventType::TimerSeeked, "s", 30.0, true).await;
    run_for(0.5).await;
    r.at(EventType::TimerSeeked, "s", 10.0, true).await;
    run_for(0.5).await;
    let log = r
        .log
        .lock()
        .unwrap()
        .iter()
        .map(|(_, a, v)| (a.clone(), v.clone()))
        .collect::<Vec<_>>();
    // start: the 1.0 row is due at once (0.1); seek to 30: 0.9; seek to 10: back to 0.1.
    assert_eq!(log.last().unwrap(), &(dim("A"), f(0.1)), "{log:?}");
    assert!(log.contains(&(dim("A"), f(0.9))));
    // The seek back cancelled the schedule made at 30 s and scheduled the 20 s row afresh: it
    // fires once, 10 s later, as the second 0.9 (the first is the re-sync at 30 s).
    run_for(20.0).await;
    assert_eq!(
        r.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, _, v)| *v == f(0.9))
            .count(),
        2
    );
}

#[tokio::test(start_paused = true)]
async fn pause_stop_end_and_reset_cancel_pending_sends_without_any_clear() {
    for stop in [
        EventType::TimerStopRequested,
        EventType::TimerStopped,
        EventType::TimerResetRequested,
        EventType::TimerFailed,
    ] {
        let rows = vec![
            look(1.0, vec![msg(&dim("A"), 0.1)]),
            look(5.0, vec![msg(&dim("A"), 0.9)]),
        ];
        let r = rig(settings(vec![("s", rows)])).await;
        r.start("s").await;
        run_for(2.0).await;
        r.at(stop, "s", 2.0, false).await;
        run_for(10.0).await;
        assert_eq!(
            r.addresses(),
            [dim("A")],
            "{stop:?}: the 5 s row was cancelled"
        );
        assert!(r.clears().is_empty(), "{stop:?}: nothing is cleared");
        r.plugin.stop().await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn song_end_holds_the_last_look() {
    let r = rig(settings(vec![(
        "s",
        vec![look(1.0, vec![msg(&dim("A"), 0.1)])],
    )]))
    .await;
    r.start("s").await;
    run_for(2.0).await;
    r.at(EventType::TimerStopped, "s", 300.0, false).await;
    run_for(5.0).await;
    assert!(r.clears().is_empty());
}

// ---------------------------------------------------------------------------- release triggers

fn assert_two_rounds(r: &Rig, fixtures: &[&str]) {
    let clears = r.clears();
    for name in fixtures {
        assert_eq!(
            clears.iter().filter(|c| **c == clear(name)).count(),
            2,
            "two clears for {name}: {clears:?}"
        );
    }
    assert_eq!(clears.len(), fixtures.len() * 2, "{clears:?}");
}

fn held_song() -> Vec<(&'static str, Vec<OscLightingCue>)> {
    vec![(
        "s",
        vec![look(1.0, vec![msg(&dim("A"), 0.1), msg(&dim("B"), 0.2)])],
    )]
}

#[tokio::test(start_paused = true)]
async fn switching_replay_off_releases_what_a_pause_was_holding() {
    let r = rig(settings(held_song())).await;
    r.start("s").await;
    run_for(2.0).await;
    r.at(EventType::TimerStopRequested, "s", 2.0, false).await;
    assert!(r.clears().is_empty());
    r.plugin
        .lightkey_apply_settings(LightkeySettings {
            api_url: String::new(),
            replay: false,
            ..LightkeySettings::default()
        })
        .await;
    assert_two_rounds(&r, &["A", "B"]);
}

#[tokio::test(start_paused = true)]
async fn disabling_lighting_releases_the_held_fixtures() {
    let r = rig(settings(held_song())).await;
    r.start("s").await;
    run_for(2.0).await;
    let mut off = settings(held_song());
    off.enabled = false;
    r.plugin.reconfigure(off).await;
    assert_two_rounds(&r, &["A", "B"]);
}

#[tokio::test(start_paused = true)]
async fn shutdown_releases_the_held_fixtures() {
    let r = rig(settings(held_song())).await;
    r.start("s").await;
    run_for(2.0).await;
    r.plugin.stop().await.unwrap();
    assert_two_rounds(&r, &["A", "B"]);
}

#[tokio::test(start_paused = true)]
async fn the_next_song_releases_only_the_fixtures_its_rows_do_not_use() {
    let r = rig(settings(vec![
        (
            "s",
            vec![look(1.0, vec![msg(&dim("A"), 0.1), msg(&dim("B"), 0.2)])],
        ),
        ("t", vec![look(1.0, vec![msg(&dim("B"), 0.7)])]),
    ]))
    .await;
    r.start("s").await;
    run_for(2.0).await;
    r.forget();
    r.start("t").await;
    run_for(2.0).await;
    assert_two_rounds(&r, &["A"]);
    assert!(
        r.addresses().contains(&dim("B")),
        "the new song took B over"
    );
}

#[tokio::test(start_paused = true)]
async fn new_song_first_look_is_not_delayed_by_releasing_unused_fixtures() {
    // S-01: 100 fixtures held by song s; song t uses one of them and has a look at 0 s.
    let many: Vec<OscMessage> = (0..100).map(|i| msg(&dim(&format!("F{i}")), 0.5)).collect();
    let r = rig(settings(vec![
        ("s", vec![look(0.5, many)]),
        ("t", vec![look(0.0, vec![msg(&dim("F0"), 0.9)])]),
    ]))
    .await;
    r.start("s").await;
    run_for(5.0).await;
    r.forget();
    let before = Instant::now();
    r.start("t").await;
    run_for(0.2).await;
    let first = r
        .log
        .lock()
        .unwrap()
        .iter()
        .find(|(_, a, v)| *a == dim("F0") && *v == f(0.9))
        .map(|(t, _, _)| (*t - before).as_secs_f64());
    let first = first.expect("the new song's first look was sent");
    assert!(first < 0.1, "first look left after {first} s");
    run_for(5.0).await;
    assert_eq!(
        r.clears().len(),
        99 * 2,
        "the 99 unused fixtures still get both rounds"
    );
}

#[tokio::test(start_paused = true)]
async fn a_song_without_fixture_rows_releases_everything_held() {
    let r = rig(settings(vec![
        ("s", vec![look(1.0, vec![msg(&dim("A"), 0.1)])]),
        ("t", vec![plain(1.0, "/live/cue/1")]),
    ]))
    .await;
    r.start("s").await;
    run_for(2.0).await;
    r.forget();
    r.start("t").await;
    run_for(2.0).await;
    assert_two_rounds(&r, &["A"]);
}

// -------------------------------------------------------------------------------- the gates

#[tokio::test(start_paused = true)]
async fn replay_off_gates_fixture_messages_only() {
    let rows = vec![
        look(1.0, vec![msg(&dim("A"), 0.1)]),
        plain(1.0, "/live/cue/1"),
        look(2.0, vec![msg(&dim("A"), 0.4), msg("/live/cue/2", 1.0)]),
    ];
    let mut lights = settings(vec![("s", rows)]);
    lights.lightkey.replay = false;
    let r = rig(lights).await;
    r.start("s").await;
    run_for(3.0).await;
    assert_eq!(
        r.addresses(),
        ["/live/cue/1", "/live/cue/2"],
        "other cues behave as before; no fixture message, no clear"
    );
}

#[tokio::test(start_paused = true)]
async fn replay_does_not_run_while_recording_is_armed() {
    let store = Arc::new(MemoryArmedStore::new());
    store.save_armed(true).unwrap();
    let rows = vec![
        look(1.0, vec![msg(&dim("A"), 0.1)]),
        plain(1.0, "/live/cue/1"),
    ];
    let r = rig_with_store(settings(vec![("s", rows)]), Some(store)).await;
    r.start("s").await;
    run_for(3.0).await;
    assert_eq!(r.addresses(), ["/live/cue/1"]);
}

// ------------------------------------------------------------------------------- big songs

#[tokio::test(start_paused = true)]
async fn a_400_row_6k_message_song_schedules_without_drift() {
    let rows: Vec<_> = (0..400)
        .map(|i| {
            let at = 5.0 + f64::from(i) * 0.6;
            look(
                (at * 10.0).round() / 10.0,
                (0..15)
                    .map(|k| msg(&dim(&format!("F{k}")), f64::from(i) / 400.0))
                    .collect(),
            )
        })
        .collect();
    let times: Vec<f64> = rows.iter().map(|r| r.at_seconds).collect();
    let r = rig(settings(vec![("s", rows)])).await;
    r.start("s").await;
    run_for(5.0 + 400.0 * 0.6 + 2.0).await;
    let sent = r.sent();
    assert_eq!(sent.len(), 6_000, "every message once");
    let mut worst: f64 = 0.0;
    for (i, at) in times.iter().enumerate() {
        let first = sent[i * 15].0;
        let last = sent[i * 15 + 14].0;
        worst = worst.max((first - (at - LOOK_LEAD_SECONDS)).abs());
        assert!(
            last - first <= 15.0 * 0.005 + 0.005,
            "row {i} spread {}",
            last - first
        );
    }
    assert!(
        worst < 0.01,
        "worst start error {worst} s: no cumulative drift"
    );
    // never faster than 200 packets a second
    assert!(sent.windows(2).all(|w| w[1].0 - w[0].0 >= 0.004), "paced");
}

// ------------------------------------------------------------------- recorded rows replay

#[tokio::test(start_paused = true)]
async fn recorded_rows_replay_through_the_scheduler_next_to_a_manual_row() {
    // A recording: dimmer A sweeps for 3 s, B steps once; thinned to rows on the 0.5 s grid.
    let mut recorder = LookRecorder::new(&FixtureState::new());
    for i in 1..=30 {
        let at = 1.0 + 0.1 * f64::from(i);
        let value = FixtureArg::Float(0.2 + 0.02 * f64::from(i));
        recorder.observe(at, &[FixtureCommand::new(dim("A"), vec![value])]);
    }
    recorder.observe(
        2.0,
        &[FixtureCommand::new(dim("B"), vec![FixtureArg::Float(1.0)])],
    );
    let rows = look_rows(&recorder.looks(300.0, 0.5));
    assert!(rows.len() >= 4 && rows.len() < 30, "{} rows", rows.len());
    let existing = SongLightingCueMap {
        song_key: "s".into(),
        song_title: "s".into(),
        cues: vec![],
        osc_cues: vec![plain(1.0, "/live/cue/1")],
        loudness: None,
        captured_edited: false,
    };
    let saved = replace_captured(Some(&existing), "s", "s", rows.clone()).unwrap();
    assert!(saved
        .osc_cues
        .iter()
        .any(|row| row.address == "/live/cue/1"));

    let r = rig(settings(vec![("s", vec![plain(1.0, "/live/cue/1")])])).await;
    r.plugin.replace_cue_map(saved).await;
    r.start("s").await;
    run_for(8.0).await;
    let sent = r.sent();
    assert_eq!(
        sent.iter().filter(|(_, a)| a == "/live/cue/1").count(),
        1,
        "manual row still fires"
    );
    let fixtures: Vec<f64> = sent
        .iter()
        .filter(|(_, a)| a == &dim("A"))
        .map(|(t, _)| *t)
        .collect();
    let wanted: Vec<f64> = rows
        .iter()
        .filter(|row| row.messages.iter().any(|m| m.address == dim("A")))
        .map(|row| row.at_seconds - LOOK_LEAD_SECONDS)
        .collect();
    assert_eq!(fixtures.len(), wanted.len(), "{fixtures:?} vs {wanted:?}");
    for (got, want) in fixtures.iter().zip(&wanted) {
        assert!(close(*got, want.max(0.0)), "{got} vs {want}");
    }
    assert!(sent.iter().any(|(_, a)| a == &dim("B")));
}

struct FlakyOutput {
    log: Log,
    calls: Mutex<u32>,
    fail: std::ops::RangeInclusive<u32>,
}
impl OscOutput for FlakyOutput {
    fn closed(&self) -> bool {
        false
    }
    fn send_message(&self, address: &str, args: &[OscArgument]) -> Result<(), OutputError> {
        let n = {
            let mut c = self.calls.lock().unwrap();
            *c += 1;
            *c
        };
        if self.fail.contains(&n) {
            return Err(OutputError::public("Network is unreachable."));
        }
        self.log
            .lock()
            .unwrap()
            .push((Instant::now(), address.into(), args.to_vec()));
        Ok(())
    }
    fn close(&self) {}
}
struct FlakyBackend(Log, std::ops::RangeInclusive<u32>);
impl OscBackend for FlakyBackend {
    fn open_output(&self, _: &str, _: u16) -> Result<Arc<dyn OscOutput>, OutputError> {
        Ok(Arc::new(FlakyOutput {
            log: self.0.clone(),
            calls: Mutex::new(0),
            fail: self.1.clone(),
        }))
    }
}

async fn flaky_rig(lights: LightsSettings, fail: std::ops::RangeInclusive<u32>) -> Rig {
    let bus = EventBus::new();
    let log: Log = Arc::default();
    let mut options = LightsOptions::new(Arc::new(|| {
        Err(stagepilot_midi::MidiError::public("no MIDI in this test"))
    }));
    let backend_log = log.clone();
    options.osc_backend_factory =
        Arc::new(move || Arc::new(FlakyBackend(backend_log.clone(), fail.clone())));
    let plugin = LightsPlugin::new(bus.clone(), lights, options).unwrap();
    plugin.start().await.unwrap();
    Rig {
        bus,
        plugin,
        log,
        t0: std::cell::Cell::new(Instant::now()),
    }
}

fn three_fixture_rows() -> Vec<OscLightingCue> {
    vec![
        look(10.0, vec![msg(&dim("A"), 0.1)]),
        look(20.0, vec![msg(&dim("B"), 0.2)]),
        look(30.0, vec![msg(&dim("C"), 0.3)]),
    ]
}

#[tokio::test(start_paused = true)]
async fn one_failed_send_is_retried_and_the_rest_of_the_song_still_goes_out() {
    // S-02: the 2nd and 3rd sends fail, so row B fails twice and is dropped; C still goes out.
    let r = flaky_rig(settings(vec![("s", three_fixture_rows())]), 2..=3).await;
    r.start("s").await;
    run_for(40.0).await;
    let sent = r.addresses();
    assert!(sent.contains(&dim("A")));
    assert!(!sent.contains(&dim("B")), "B failed twice: {sent:?}");
    assert!(
        sent.contains(&dim("C")),
        "the rest of the song survives one bad row: {sent:?}"
    );
    let status = r.plugin.lightkey_status().await;
    assert_eq!(
        status.replaying_song.as_deref(),
        Some("s"),
        "still replaying"
    );
}

#[tokio::test(start_paused = true)]
async fn a_send_that_works_on_the_retry_is_not_lost() {
    let r = flaky_rig(settings(vec![("s", three_fixture_rows())]), 2..=2).await;
    r.start("s").await;
    run_for(40.0).await;
    let sent = r.addresses();
    for name in ["A", "B", "C"] {
        assert!(sent.contains(&dim(name)), "{name} missing: {sent:?}");
    }
}

// ------------------------------------------------------------------ operator override (L3)

use stagepilot_lights::plugin::OPERATOR_TAKE_BACK_SECONDS;

fn dmx(address: &str, x: f64) -> FixtureCommand {
    FixtureCommand::new(address, vec![FixtureArg::Float(x)])
}

fn pan(name: &str) -> String {
    format!("/fixture/{name}/overrides/panAngle")
}

/// Two fixtures, A and B, each with a dimmer row at 1 s and 10 s, and a pan row for A at 40 s.
fn two_fixture_song() -> Vec<OscLightingCue> {
    vec![
        look(1.0, vec![msg(&dim("A"), 0.1), msg(&dim("B"), 0.1)]),
        look(10.0, vec![msg(&dim("A"), 0.2), msg(&dim("B"), 0.2)]),
        look(20.0, vec![msg(&dim("A"), 0.3), msg(&dim("B"), 0.3)]),
        look(40.0, vec![msg(&dim("A"), 0.4), msg(&dim("B"), 0.4)]),
        look(60.0, vec![msg(&dim("A"), 0.5), msg(&dim("B"), 0.5)]),
    ]
}

fn count(r: &Rig, address: &str) -> usize {
    r.addresses().iter().filter(|a| *a == address).count()
}

#[tokio::test(start_paused = true)]
async fn an_operator_change_yields_that_fixture_without_a_clear() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false);
    r.forget();
    run_for(16.0).await; // the 10 s and 20 s rows go out (not past 30 s of take-back)
    assert_eq!(count(&r, &dim("A")), 0, "no more sends to A");
    assert_eq!(count(&r, &dim("B")), 2, "B keeps replaying");
    assert!(
        r.clears().is_empty(),
        "the operator's change is not cleared"
    );
    let status = r.plugin.lightkey_status().await;
    assert_eq!(status.yielded_fixtures, Some(1));
    assert_eq!(status.holding_fixtures, Some(1));
}

#[tokio::test(start_paused = true)]
async fn the_replays_own_echo_never_yields() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(1.0).await; // rows at 1 s go out at 0.955 s
                        // The relay repeats what was sent about 45 ms later, and may repeat it twice.
    run_for(0.045).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.1), dmx(&dim("B"), 0.1)], false);
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.1)], false);
    // Wheel slots are approximate: a different value of the same property is still an echo.
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.1004)], false);
    run_for(12.0).await;
    assert_eq!(count(&r, &dim("A")), 2, "A still replays");
    let status = r.plugin.lightkey_status().await;
    assert_eq!(status.yielded_fixtures, None);
}

#[tokio::test(start_paused = true)]
async fn an_unsent_property_never_yields_but_a_changed_sent_one_does() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(10.0).await; // the 10 s row went out at 9.955 s
                         // A pan the replay never sent is Lightkey's own business (a chase, an effect): not the operator.
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&pan("A"), 12.0)], false);
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, None);
    // A different dimmer value 100 ms after the send is the operator, not an echo (O-01).
    run_for(0.1).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("B"), 0.8)], false);
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, Some(1));
}

#[tokio::test(start_paused = true)]
async fn replay_takes_the_fixture_back_after_the_quiet_time() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false);
    r.forget();
    // 5 s + 30 s = 35 s into the song: the latest A row at or before then is the 0.3 at 20 s.
    run_for(OPERATOR_TAKE_BACK_SECONDS - 1.0).await;
    assert_eq!(count(&r, &dim("A")), 0, "still the operator's");
    run_for(1.5).await;
    let a: Vec<_> = r
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, address, _)| *address == dim("A"))
        .map(|(_, _, args)| args.clone())
        .collect();
    assert_eq!(
        a,
        vec![f(0.3)],
        "re-synced to the latest message at the position"
    );
    assert!(r.clears().is_empty());
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, None);
    run_for(10.0).await; // the 40 s row (sent at 39.955) continues
    assert_eq!(count(&r, &dim("A")), 2);
}

#[tokio::test(start_paused = true)]
async fn every_new_operator_change_restarts_the_quiet_time() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false);
    run_for(20.0).await;
    // Still yielded: the operator moves A again, even though nothing is held now.
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.8)], false);
    r.forget();
    run_for(OPERATOR_TAKE_BACK_SECONDS - 1.0).await;
    assert_eq!(count(&r, &dim("A")), 0, "30 s count from the last change");
    run_for(1.5).await;
    assert!(count(&r, &dim("A")) >= 1, "taken back after 30 s of quiet");
}

#[tokio::test(start_paused = true)]
async fn keyframes_and_reconnects_do_not_yield() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    // A keyframe-detected difference (a missed packet) is not operator input.
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9), dmx(&dim("B"), 0.9)], true);
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, None);
    // A reconnect makes the state unknown; the link tells the plugin so, and the changes that
    // follow while it re-learns the state are not operator input either.
    r.plugin.lightkey_stream_reset();
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9), dmx(&dim("B"), 0.9)], false);
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, None);
    run_for(7.0).await;
    assert_eq!(r.plugin.lightkey_status().await.holding_fixtures, Some(2));
}

#[tokio::test(start_paused = true)]
async fn release_lights_clears_exactly_the_held_fixtures() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false); // A is the operator's
    r.forget();
    let (accepted, message) = r.plugin.lightkey_release_lights().await;
    assert!(accepted, "{message}");
    assert_eq!(
        r.clears(),
        vec![clear("B"), clear("B")],
        "only the held fixture, twice"
    );
    let status = r.plugin.lightkey_status().await;
    assert_eq!(status.holding_fixtures, None);
    assert_eq!(status.yielded_fixtures, Some(1));
}

#[tokio::test(start_paused = true)]
async fn release_lights_after_a_restart_clears_every_fixture_the_looks_name() {
    // H-01: the held set is memory only. A fresh plugin holds nothing, yet Lightkey may still
    // hold the overrides of the run before the restart.
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    let status = r.plugin.lightkey_status().await;
    assert_eq!(status.holding_fixtures, None);
    assert_eq!(
        status.known_fixtures,
        Some(2),
        "the button must stay usable"
    );
    let (accepted, message) = r.plugin.lightkey_release_lights().await;
    assert!(accepted, "{message}");
    let mut clears = r.clears();
    clears.sort();
    assert_eq!(clears, vec![clear("A"), clear("A"), clear("B"), clear("B")]);
}

#[tokio::test(start_paused = true)]
async fn release_lights_with_no_looks_and_nothing_held_does_nothing() {
    let r = rig(settings(vec![("s", vec![plain(1.0, "/live/cue/1")])])).await;
    let (accepted, _) = r.plugin.lightkey_release_lights().await;
    assert!(!accepted);
    assert!(r.clears().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_paused_song_waits_to_take_back_until_resume() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false);
    r.at(EventType::TimerStopRequested, "s", 5.0, false).await;
    r.forget();
    run_for(OPERATOR_TAKE_BACK_SECONDS + 20.0).await;
    assert!(r.addresses().is_empty(), "paused: nothing is sent");
    // Resume at 20 s: the 30 s have passed, so the re-sync covers A again.
    r.at(EventType::TimerResumed, "s", 20.0, true).await;
    run_for(1.0).await;
    let a: Vec<_> = r
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, address, _)| *address == dim("A"))
        .map(|(_, _, args)| args.clone())
        .collect();
    assert_eq!(a, vec![f(0.3)]);
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, None);
}

#[tokio::test(start_paused = true)]
async fn a_seek_keeps_a_fixture_the_operator_still_has() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false);
    r.forget();
    run_for(3.0).await;
    r.at(EventType::TimerSeeked, "s", 30.0, true).await;
    run_for(1.0).await;
    assert_eq!(
        count(&r, &dim("A")),
        0,
        "within 30 s: A stays the operator's"
    );
    assert_eq!(count(&r, &dim("B")), 1, "B is re-synced");
    run_for(OPERATOR_TAKE_BACK_SECONDS).await;
    assert!(count(&r, &dim("A")) >= 1, "then taken back");
}

#[tokio::test(start_paused = true)]
async fn replay_off_gives_everything_back_and_forgets_the_operator() {
    let r = rig(settings(vec![("s", two_fixture_song())])).await;
    r.start("s").await;
    run_for(5.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false);
    r.forget();
    let mut lightkey = r.plugin.settings().lightkey;
    lightkey.replay = false;
    r.plugin.lightkey_apply_settings(lightkey).await;
    assert_eq!(r.clears(), vec![clear("B"), clear("B")]);
    // Replay is off: the operator's changes are nobody's business, and nothing is taken back.
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("B"), 0.9)], false);
    run_for(OPERATOR_TAKE_BACK_SECONDS + 5.0).await;
    let status = r.plugin.lightkey_status().await;
    assert_eq!(status.yielded_fixtures, None);
    assert_eq!(status.holding_fixtures, None);
    assert_eq!(r.clears().len(), 2);
}
