//! The replay of a capture that has a starting look: every light of the rig is set at the first
//! look and stays held. At the song's natural end, on pause, stop and seek NOTHING is given back
//! (Hunter, 2026-11-08: "I don't want them to change back at the end of the song"); the lights are
//! released only by the operator ("Release lights"), by turning Replay off, or by a new song that
//! does not use them. A capture from before the starting look replays exactly as it did.
//! Time is tokio's paused clock.

use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{
    CaptureStatus, LightingMethod, LightkeySettings, LightsSettings, OscArgument, OscCueSource,
    OscLightingCue, OscMessage, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_link::{ArmedStore, LinkOptions, MemoryArmedStore};
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
        fade_seconds: 0.0,
    }
}
#[allow(dead_code)]
fn plain(at: f64, address: &str) -> OscLightingCue {
    OscLightingCue {
        address: address.into(),
        messages: vec![],
        source: OscCueSource::Manual,
        edited: false,
        fade_seconds: 0.0,
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
    settings_with(songs, None)
}
fn settings_with(
    songs: Vec<(&str, Vec<OscLightingCue>)>,
    capture: Option<CaptureStatus>,
) -> LightsSettings {
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
                capture: capture.clone(),
                effects: Vec::new(),
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
                    position_seconds: None,
                    observed_at: None,
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

#[allow(dead_code)]
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.02
}

// ------------------------------------------------------------------------------ the songs

const HOUSE: usize = 10;

fn house(n: usize) -> String {
    format!("H{n}")
}

fn dmx(address: &str, x: f64) -> FixtureCommand {
    FixtureCommand::new(address, vec![FixtureArg::Float(x)])
}

fn fade_look(at: f64, fade: f64, messages: Vec<OscMessage>) -> OscLightingCue {
    OscLightingCue {
        fade_seconds: fade,
        ..look(at, messages)
    }
}

/// Fixture A moves at 5 s (and ramps in the smooth song); the ten house lights H0..H9 appear only
/// in the starting look, at 0.
fn song_rows(smooth: bool) -> Vec<OscLightingCue> {
    let mut start: Vec<OscMessage> = vec![msg(&dim("A"), 0.0)];
    for n in 0..HOUSE {
        start.push(msg(&dim(&house(n)), if n % 3 == 0 { 0.0 } else { 0.6 }));
    }
    let mut rows = vec![look(0.0, start)];
    if smooth {
        rows.push(fade_look(5.0, 20.0, vec![msg(&dim("A"), 1.0)]));
    } else {
        rows.push(look(5.0, vec![msg(&dim("A"), 1.0)]));
    }
    rows
}

fn with_start_look(lights: u32) -> CaptureStatus {
    CaptureStatus {
        complete: true,
        coverage_seconds: 300.0,
        recorded_at: "2026-11-08T00:00:00Z".into(),
        project: None,
        simplified: None,
        start_lights: Some(lights),
    }
}

fn before_start_look() -> CaptureStatus {
    CaptureStatus {
        start_lights: None,
        ..with_start_look(0)
    }
}

async fn held(r: &Rig) -> Option<u32> {
    r.plugin.lightkey_status().await.holding_fixtures
}

fn dim_sends(r: &Rig, name: &str) -> usize {
    r.addresses().iter().filter(|a| **a == dim(name)).count()
}

// ------------------------------------------------------------- R2: held from the first look

async fn the_whole_rig_is_held_from_the_first_look(smooth: bool) {
    let r = rig(settings_with(
        vec![("s", song_rows(smooth))],
        Some(with_start_look(11)),
    ))
    .await;
    r.start("s").await;
    run_for(1.0).await;
    // the first look sets every fixture of the starting look, none before the song's start
    for n in 0..HOUSE {
        assert!(
            dim_sends(&r, &house(n)) >= 1,
            "H{n} is set at the first look"
        );
    }
    assert!(dim_sends(&r, "A") >= 1);
    assert!(
        r.sent().iter().all(|(t, _)| *t >= -0.001),
        "nothing before song position 0"
    );
    assert_eq!(held(&r).await, Some(11), "Holding 11 fixtures");
    assert!(r.clears().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_step_song_holds_the_whole_starting_look_from_the_first_look() {
    the_whole_rig_is_held_from_the_first_look(false).await;
}

#[tokio::test(start_paused = true)]
async fn a_smooth_song_holds_the_whole_starting_look_from_the_first_look() {
    the_whole_rig_is_held_from_the_first_look(true).await;
}

// ------------------------------------------------------- R3: at the end nothing changes

async fn nothing_is_given_back(smooth: bool) {
    let r = rig(settings_with(
        vec![("s", song_rows(smooth))],
        Some(with_start_look(11)),
    ))
    .await;
    r.start("s").await;
    run_for(30.0).await;
    assert_eq!(held(&r).await, Some(11));
    // pause
    r.at(EventType::TimerStopRequested, "s", 30.0, false).await;
    run_for(5.0).await;
    assert_eq!(held(&r).await, Some(11), "pause: held count unchanged");
    // seek back to the very start, and forward, while paused and while running
    r.at(EventType::TimerSeeked, "s", 0.0, true).await;
    run_for(2.0).await;
    r.at(EventType::TimerSeeked, "s", 200.0, true).await;
    run_for(2.0).await;
    assert_eq!(held(&r).await, Some(11), "seek: held count unchanged");
    // stop and reset
    r.at(EventType::TimerStopped, "s", 200.0, false).await;
    r.at(EventType::TimerResetRequested, "s", 0.0, false).await;
    assert_eq!(held(&r).await, Some(11), "stop: held count unchanged");
    // the natural end of the song: the clock runs out while it plays
    r.at(EventType::TimerResumed, "s", 250.0, true).await;
    run_for(60.0).await;
    r.at(EventType::TimerStopped, "s", 300.0, false).await;
    run_for(10.0).await;
    assert_eq!(held(&r).await, Some(11), "song end: held count unchanged");
    assert!(
        r.clears().is_empty(),
        "no overrides/clear is sent for any fixture: {:?}",
        r.clears()
    );
    // a fixture the operator takes still yields, and nothing is cleared for it either
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim(&house(1)), 0.95)], false);
    run_for(1.0).await;
    let status = r.plugin.lightkey_status().await;
    assert_eq!(
        status.yielded_fixtures,
        Some(1),
        "a manual change yields that fixture"
    );
    assert!(r.clears().is_empty());
}

#[tokio::test(start_paused = true)]
async fn at_song_end_pause_stop_and_seek_a_step_song_clears_nothing() {
    nothing_is_given_back(false).await;
}

#[tokio::test(start_paused = true)]
async fn at_song_end_pause_stop_and_seek_a_smooth_song_clears_nothing() {
    nothing_is_given_back(true).await;
}

#[tokio::test(start_paused = true)]
async fn release_lights_clears_every_held_fixture_of_the_starting_look() {
    let r = rig(settings_with(
        vec![("s", song_rows(false))],
        Some(with_start_look(11)),
    ))
    .await;
    r.start("s").await;
    run_for(310.0).await; // the song has ended
    assert_eq!(held(&r).await, Some(11));
    r.forget();
    let (accepted, message) = r.plugin.lightkey_release_lights().await;
    assert!(accepted, "{message}");
    let clears = r.clears();
    let mut want: Vec<String> = (0..HOUSE).map(|n| clear(&house(n))).collect();
    want.push(clear("A"));
    for name in &want {
        assert_eq!(
            clears.iter().filter(|c| *c == name).count(),
            2,
            "{name} twice"
        );
    }
    assert_eq!(clears.len(), want.len() * 2);
    assert_eq!(held(&r).await, None);
}

#[tokio::test(start_paused = true)]
async fn a_manual_change_on_a_fixture_held_only_by_the_starting_look_yields_it() {
    let r = rig(settings_with(
        vec![("s", song_rows(false))],
        Some(with_start_look(11)),
    ))
    .await;
    r.start("s").await;
    run_for(4.0).await; // past the 3 s after the stream began, when changes are never the operator's
    r.forget();
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim(&house(4)), 0.2)], false);
    run_for(5.0).await;
    let status = r.plugin.lightkey_status().await;
    assert_eq!(status.yielded_fixtures, Some(1));
    assert!(r.clears().is_empty(), "the operator's change stays");
    assert_eq!(
        dim_sends(&r, &house(4)),
        0,
        "nothing is sent to H4 while yielded"
    );
    // 30 s without another change and the replay takes it back at its starting-look value
    run_for(31.0).await;
    assert_eq!(dim_sends(&r, &house(4)), 1);
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, None);
}

// ------------------------------------------------------- R4: captures from before this

#[tokio::test(start_paused = true)]
async fn a_capture_from_before_the_starting_look_replays_exactly_as_before() {
    // only the fixtures that changed are in the rows: A. Nothing else is touched or held.
    let rows = vec![
        look(0.0, vec![msg(&dim("A"), 0.0)]),
        look(5.0, vec![msg(&dim("A"), 1.0)]),
    ];
    let r = rig(settings_with(vec![("s", rows)], Some(before_start_look()))).await;
    r.start("s").await;
    run_for(310.0).await;
    assert_eq!(held(&r).await, Some(1), "only the captured fixture is held");
    assert!(
        r.addresses().iter().all(|a| *a == dim("A")),
        "{:?}",
        r.addresses()
    );
    assert!(r.clears().is_empty());
    // and with no capture status at all (the oldest files) the same
    let rows = vec![look(0.0, vec![msg(&dim("A"), 0.0)])];
    let r = rig(settings(vec![("s", rows)])).await;
    r.start("s").await;
    run_for(10.0).await;
    assert_eq!(held(&r).await, Some(1));
}

#[test]
fn an_old_capture_is_recognised_and_a_new_one_is_not() {
    let map = |capture: Option<CaptureStatus>| SongLightingCueMap {
        song_key: "s".into(),
        song_title: "s".into(),
        cues: vec![],
        osc_cues: vec![look(0.0, vec![msg(&dim("A"), 0.0)])],
        loudness: None,
        captured_edited: false,
        capture,
        effects: vec![],
    };
    assert!(map(Some(before_start_look())).recorded_before_start_look());
    assert!(map(None).recorded_before_start_look());
    assert!(!map(Some(with_start_look(66))).recorded_before_start_look());
    // could not be read at record time: the capture is new, not "old"
    assert!(!map(Some(with_start_look(0))).recorded_before_start_look());
    let empty = SongLightingCueMap {
        osc_cues: vec![],
        ..map(None)
    };
    assert!(
        !empty.recorded_before_start_look(),
        "nothing captured, nothing to say"
    );
}
