//! F-H3: what the operator touches while NO song is replaying (before the first look of a song,
//! after a song ended) must not count as a take-over of the next song. When a replay starts
//! (from the top, or a resume/seek into a song that was not on) every older yield is dropped and
//! the first look goes to every fixture it covers. A touch DURING the song (also while it is
//! paused) behaves exactly as before: the light stays the operator's for 30 s, then it is handed
//! back, and a touch of the last 30 s stays with the operator after a jump. Nothing is released
//! at a song's end or on stop. Time is tokio's paused clock.

use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{
    LightingMethod, LightkeySettings, LightsSettings, OscArgument, OscCueSource, OscLightingCue,
    OscMessage, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_link::{ArmedStore, LinkOptions, MemoryArmedStore};
use stagepilot_lights::plugin::OPERATOR_TAKE_BACK_SECONDS;
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
fn f(x: f64) -> Vec<OscArgument> {
    vec![OscArgument::Float(x)]
}
fn msg(address: &str, x: f64) -> OscMessage {
    OscMessage {
        address: address.into(),
        args: f(x),
    }
}
fn look(at: f64, fade: f64, messages: Vec<OscMessage>) -> OscLightingCue {
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
        fade_seconds: fade,
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
    for (key, rows) in songs {
        cue_maps.insert(
            key.to_string(),
            SongLightingCueMap {
                song_key: key.into(),
                song_title: key.into(),
                cues: vec![],
                osc_cues: rows,
                loudness: None,
                captured_edited: false,
                capture: None,
                effects: vec![],
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
    t0: Instant,
    fed: usize,
}

async fn rig(lights: LightsSettings) -> Rig {
    let bus = EventBus::new();
    let log: Log = Arc::default();
    let mut options = LightsOptions::new(Arc::new(|| {
        Err(stagepilot_midi::MidiError::public("no MIDI in this test"))
    }));
    let backend_log = log.clone();
    options.osc_backend_factory = Arc::new(move || Arc::new(TimedBackend(backend_log.clone())));
    options.lightkey = LinkOptions {
        armed_store: Arc::new(MemoryArmedStore::new()) as Arc<dyn ArmedStore>,
        ..LinkOptions::default()
    };
    let plugin = LightsPlugin::new(bus.clone(), lights, options).unwrap();
    plugin.start().await.unwrap();
    Rig {
        bus,
        plugin,
        log,
        t0: Instant::now(),
        fed: 0,
    }
}

impl Rig {
    async fn publish(&self, kind: EventType, payload: TimerPayload) {
        self.bus
            .publish(new_event(kind, "t", EventPayload::Timer(payload)))
            .await;
    }

    async fn start(&mut self, key: &str) {
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
        self.t0 = Instant::now();
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

    /// `(seconds since the song started, address, first number)` of everything sent.
    fn sent(&self) -> Vec<(f64, String, f64)> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(t, a, v)| {
                (
                    (*t - self.t0).as_secs_f64(),
                    a.clone(),
                    match v.first() {
                        Some(OscArgument::Float(x)) => *x,
                        _ => f64::NAN,
                    },
                )
            })
            .collect()
    }

    fn of(&self, address: &str) -> Vec<(f64, f64)> {
        self.sent()
            .into_iter()
            .filter(|(_, a, _)| a == address)
            .map(|(t, _, v)| (t, v))
            .collect()
    }

    fn clears(&self) -> usize {
        self.sent()
            .iter()
            .filter(|(_, a, _)| a.ends_with("/clear"))
            .count()
    }

    /// The relay does not report what was sent so far (it is old news).
    fn skip_echo(&mut self) {
        self.fed = self.log.lock().unwrap().len();
    }

    /// Run `seconds` in 20 ms steps; after each step the relay "sees" what was sent in it and
    /// reports it back as DMX changes (the fake echo). Returns nothing yielded by the echo.
    async fn run_with_echo(&mut self, seconds: f64) {
        let steps = (seconds / 0.02).round() as usize;
        for _ in 0..steps {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let new: Vec<FixtureCommand> = {
                let log = self.log.lock().unwrap();
                let out = log[self.fed..]
                    .iter()
                    .filter(|(_, a, _)| !a.ends_with("/clear"))
                    .map(|(_, a, v)| {
                        FixtureCommand::new(
                            a.clone(),
                            v.iter()
                                .map(|x| match x {
                                    OscArgument::Float(f) => FixtureArg::Float(*f),
                                    _ => FixtureArg::Float(0.0),
                                })
                                .collect(),
                        )
                    })
                    .collect();
                self.fed = log.len();
                out
            };
            if !new.is_empty() {
                self.plugin.lightkey_fixture_changes(new, false);
            }
        }
    }
}

async fn run_for(seconds: f64) {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
}

fn dmx(address: &str, x: f64) -> FixtureCommand {
    FixtureCommand::new(address, vec![FixtureArg::Float(x)])
}

fn pan_free_song() -> Vec<OscLightingCue> {
    // A ramps (so the song is replayed smoothly); B and C are written once, by the first look,
    // and never again: if the first look skips them they stay as the operator left them.
    vec![
        look(
            0.0,
            0.0,
            vec![
                msg(&dim("A"), 0.0),
                msg(&dim("B"), 0.0),
                msg(&dim("C"), 0.0),
            ],
        ),
        look(0.5, 60.0, vec![msg(&dim("A"), 1.0)]),
    ]
}

fn second_song() -> Vec<OscLightingCue> {
    vec![
        look(
            0.0,
            0.0,
            vec![
                msg(&dim("A"), 0.3),
                msg(&dim("B"), 0.3),
                msg(&dim("C"), 0.3),
            ],
        ),
        look(0.5, 60.0, vec![msg(&dim("D"), 1.0)]),
    ]
}

async fn yielded(r: &Rig) -> Option<u32> {
    r.plugin.lightkey_status().await.yielded_fixtures
}

fn sends_after(r: &Rig, address: &str, t: f64) -> usize {
    r.of(address).iter().filter(|(at, _)| *at >= t).count()
}

fn touch(r: &Rig, names: &[&str], x: f64) {
    r.plugin
        .lightkey_fixture_changes(names.iter().map(|n| dmx(&dim(n), x)).collect(), false);
}

#[tokio::test(start_paused = true)]
async fn a_preset_16_seconds_before_play_does_not_keep_the_first_look_away() {
    let mut r = rig(settings(vec![("s", pan_free_song())])).await;
    r.start("s").await;
    r.run_with_echo(10.0).await;
    // the song is stopped and the position reset; the lights stay as they are
    r.at(EventType::TimerStopRequested, "s", 10.0, false).await;
    r.at(EventType::TimerResetRequested, "s", 0.0, false).await;
    assert_eq!(r.clears(), 0);
    // the operator switches a preset on three lights, 16 s before play
    touch(&r, &["A", "B", "C"], 0.7);
    assert_eq!(yielded(&r).await, Some(3), "recorded as it always was");
    r.run_with_echo(16.0).await;
    assert_eq!(r.clears(), 0, "nothing is released while no song plays");
    // play: the song starts from the top
    r.start("s").await;
    assert_eq!(yielded(&r).await, None, "the start drops the older yields");
    r.run_with_echo(2.0).await;
    for light in ["A", "B", "C"] {
        assert!(
            sends_after(&r, &dim(light), 0.0) >= 1,
            "{light} is in the first look"
        );
    }
    assert_eq!(
        yielded(&r).await,
        None,
        "and the first look is not the operator"
    );
}

#[tokio::test(start_paused = true)]
async fn a_touch_during_the_song_still_yields_for_30_seconds_and_is_handed_back() {
    let mut r = rig(settings(vec![("s", pan_free_song())])).await;
    r.start("s").await;
    r.run_with_echo(5.0).await;
    touch(&r, &["B"], 0.7);
    assert_eq!(yielded(&r).await, Some(1));
    let b = r.of(&dim("B")).len();
    r.run_with_echo(OPERATOR_TAKE_BACK_SECONDS - 2.0).await;
    assert_eq!(r.of(&dim("B")).len(), b, "B is the operator's");
    assert_eq!(yielded(&r).await, Some(1));
    r.run_with_echo(3.0).await;
    assert_eq!(yielded(&r).await, None, "handed back after 30 s");
    assert!(r.of(&dim("B")).len() > b, "and driven again");
}

#[tokio::test(start_paused = true)]
async fn a_touch_while_the_song_is_paused_stays_with_the_operator_after_the_resume() {
    let mut r = rig(settings(vec![("s", pan_free_song())])).await;
    r.start("s").await;
    r.run_with_echo(5.0).await;
    r.at(EventType::TimerStopRequested, "s", 5.0, false).await;
    r.at(EventType::TimerStopped, "s", 5.0, false).await;
    touch(&r, &["B"], 0.7);
    assert_eq!(yielded(&r).await, Some(1));
    run_for(10.0).await;
    r.at(EventType::TimerResumed, "s", 5.0, true).await;
    assert_eq!(
        yielded(&r).await,
        Some(1),
        "a resume inside the song keeps it"
    );
    let b = r.of(&dim("B")).len();
    r.run_with_echo(10.0).await;
    assert_eq!(r.of(&dim("B")).len(), b, "B stays the operator's");
    assert_eq!(yielded(&r).await, Some(1));
    // 30 s after the touch it is the replay's again
    r.run_with_echo(12.0).await;
    assert_eq!(yielded(&r).await, None);
    assert!(r.of(&dim("B")).len() > b);
}

#[tokio::test(start_paused = true)]
async fn a_touch_of_the_last_30_seconds_stays_with_the_operator_after_a_jump() {
    let mut r = rig(settings(vec![("s", pan_free_song())])).await;
    r.start("s").await;
    r.run_with_echo(20.0).await;
    touch(&r, &["B"], 0.7);
    run_for(10.0).await;
    r.at(EventType::TimerSeeked, "s", 100.0, true).await;
    assert_eq!(
        yielded(&r).await,
        Some(1),
        "a seek inside the song keeps it"
    );
    let b = r.of(&dim("B")).len();
    r.run_with_echo(5.0).await;
    assert_eq!(r.of(&dim("B")).len(), b);
    // one that is older than 30 s is the replay's again at the jump
    r.run_with_echo(OPERATOR_TAKE_BACK_SECONDS).await;
    r.at(EventType::TimerSeeked, "s", 150.0, true).await;
    assert_eq!(yielded(&r).await, None);
}

#[tokio::test(start_paused = true)]
async fn a_seek_into_a_song_that_is_not_on_starts_it_like_a_play() {
    let mut r = rig(settings(vec![("s", pan_free_song())])).await;
    r.start("s").await;
    r.run_with_echo(10.0).await;
    r.at(EventType::TimerStopRequested, "s", 10.0, false).await;
    r.at(EventType::TimerResetRequested, "s", 0.0, false).await;
    touch(&r, &["B", "C"], 0.7);
    assert_eq!(yielded(&r).await, Some(2));
    r.run_with_echo(16.0).await;
    let before = r.sent().last().map_or(0.0, |s| s.0);
    // play pressed in the middle of the song
    r.at(EventType::TimerResumed, "s", 120.0, true).await;
    assert_eq!(yielded(&r).await, None);
    r.run_with_echo(2.0).await;
    for light in ["B", "C"] {
        assert!(
            sends_after(&r, &dim(light), before + 0.001) >= 1,
            "{light} gets the look of second 120"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn after_the_song_ended_a_resume_into_it_starts_a_new_replay() {
    let mut r = rig(settings(vec![("s", pan_free_song())])).await;
    r.start("s").await;
    run_for(299.5).await;
    // the song ends: the timer runs out
    r.at(EventType::TimerStopped, "s", 300.0, false).await;
    run_for(1.0).await;
    r.skip_echo();
    assert_eq!(r.clears(), 0, "nothing is released at the end");
    touch(&r, &["B"], 0.7);
    run_for(10.0).await;
    r.skip_echo();
    let before = r.of(&dim("B")).len();
    r.at(EventType::TimerResumed, "s", 100.0, true).await;
    assert_eq!(yielded(&r).await, None);
    r.run_with_echo(2.0).await;
    assert!(
        r.of(&dim("B")).len() > before,
        "B is in the look of second 100"
    );
    assert_eq!(r.clears(), 0);
}

#[tokio::test(start_paused = true)]
async fn an_auto_advance_to_the_next_song_gives_its_first_look_to_every_light() {
    let mut r = rig(settings(vec![("s", pan_free_song()), ("t", second_song())])).await;
    r.start("s").await;
    r.run_with_echo(20.0).await;
    run_for(270.0).await;
    r.skip_echo();
    // the operator touches a light in the last seconds of the song and one after its end
    touch(&r, &["A"], 0.9);
    run_for(9.6).await;
    r.at(EventType::TimerStopped, "s", 300.0, false).await;
    touch(&r, &["B"], 0.9);
    assert_eq!(yielded(&r).await, Some(2), "as it always was");
    run_for(5.0).await;
    r.skip_echo();
    assert_eq!(r.clears(), 0);
    // the next song starts by itself
    r.start("t").await;
    assert_eq!(yielded(&r).await, None);
    r.run_with_echo(2.0).await;
    for light in ["A", "B", "C"] {
        let first = r.of(&dim(light));
        let last = first.last().expect("sent");
        assert!(
            last.0 >= 0.0 && (last.1 - 0.3).abs() < 1e-9,
            "{light}: {last:?}"
        );
    }
    assert_eq!(yielded(&r).await, None);
    // and a touch during the second song is the operator's as before
    touch(&r, &["C"], 0.9);
    assert_eq!(yielded(&r).await, Some(1));
}
