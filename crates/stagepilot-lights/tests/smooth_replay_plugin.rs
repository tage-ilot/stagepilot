//! The smooth replay inside the lights plugin, on tokio's paused clock: a ramp and an effect
//! play through the real scheduler and pacing, seek and pause behave, the operator's take-over
//! works with interpolated values (a fake DMX echo feeds the relay's view of every send back),
//! and a song captured before smooth replay plays exactly as it always did.

use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{
    EffectMember, EffectStep, LightingEffect, LightingMethod, LightkeySettings, LightsSettings,
    OscArgument, OscCueSource, OscLightingCue, OscMessage, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey_curve::Timeline;
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_link::{ArmedStore, LinkOptions, MemoryArmedStore};
use stagepilot_lights::plugin::{LOOK_LEAD_SECONDS, OPERATOR_TAKE_BACK_SECONDS};
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
fn settings(rows: Vec<OscLightingCue>, effects: Vec<LightingEffect>) -> LightsSettings {
    let mut cue_maps = BTreeMap::new();
    cue_maps.insert(
        "s".to_string(),
        SongLightingCueMap {
            song_key: "s".into(),
            song_title: "s".into(),
            cues: vec![],
            osc_cues: rows,
            loudness: None,
            captured_edited: false,
            capture: None,
            effects,
        },
    );
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

    async fn start(&mut self) {
        self.bus
            .publish(new_event(
                EventType::SongStarted,
                "t",
                EventPayload::Song {
                    song: song("s"),
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
                song: Some(song("s")),
                running: true,
                ..Default::default()
            },
        )
        .await;
        self.t0 = Instant::now();
    }

    async fn at(&self, kind: EventType, position: f64, running: bool) {
        self.publish(
            kind,
            TimerPayload {
                duration_seconds: 300 - position as u32,
                song: Some(song("s")),
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

fn ramp_song() -> Vec<OscLightingCue> {
    vec![
        look(0.0, 0.0, vec![msg(&dim("A"), 0.0), msg(&dim("B"), 0.0)]),
        look(0.5, 60.0, vec![msg(&dim("A"), 1.0), msg(&dim("B"), 1.0)]),
    ]
}

// -------------------------------------------------------------------- songs from before this

#[tokio::test(start_paused = true)]
async fn a_song_captured_before_smooth_replay_plays_exactly_as_before() {
    // rows without fade_seconds, no effects: one message per row message, at the row's time
    // minus the look lead, and not one packet more however long the song runs
    let rows = vec![
        look(1.0, 0.0, vec![msg(&dim("A"), 0.1), msg(&dim("B"), 0.2)]),
        look(30.0, 0.0, vec![msg(&dim("A"), 0.9)]),
        look(200.0, 0.0, vec![msg(&dim("B"), 0.4)]),
    ];
    assert!(!stagepilot_lights::lightkey_replay::is_smooth(&rows, &[]));
    let mut r = rig(settings(rows, vec![])).await;
    r.start().await;
    run_for(250.0).await;
    let sent = r.sent();
    let names: Vec<_> = sent.iter().map(|(_, a, v)| (a.clone(), *v)).collect();
    assert_eq!(
        names,
        [
            (dim("A"), 0.1),
            (dim("B"), 0.2),
            (dim("A"), 0.9),
            (dim("B"), 0.4)
        ]
    );
    assert!((sent[0].0 - (1.0 - LOOK_LEAD_SECONDS)).abs() < 0.02);
    assert!((sent[2].0 - (30.0 - LOOK_LEAD_SECONDS)).abs() < 0.02);
    assert!((sent[3].0 - (200.0 - LOOK_LEAD_SECONDS)).abs() < 0.02);
    assert_eq!(r.clears(), 0);
}

// ------------------------------------------------------------------------------ the ramp

#[tokio::test(start_paused = true)]
async fn a_sixty_second_ramp_plays_through_the_scheduler() {
    let mut r = rig(settings(ramp_song(), vec![])).await;
    r.start().await;
    run_for(40.0).await;
    let a = r.of(&dim("A"));
    // at 40 s the ramp (0.5 s .. 60.5 s) is at 0.6583
    let (_, now) = *a.last().unwrap();
    assert!((now - (39.5 / 60.0)).abs() < 0.02, "{now}");
    run_for(30.0).await;
    let a = r.of(&dim("A"));
    assert_eq!(a.last().unwrap().1, 1.0, "ends exactly");
    assert!(
        (230..=300).contains(&a.len()),
        "{} packets for 255 steps",
        a.len()
    );
    assert!(a.windows(2).all(|w| w[1].1 >= w[0].1), "monotone");
    assert!(a.last().unwrap().0 < 61.0, "stops at the end of the ramp");
    assert_eq!(r.clears(), 0);
    // both fixtures move together
    assert_eq!(r.of(&dim("B")).len(), a.len());
}

#[tokio::test(start_paused = true)]
async fn an_effect_plays_through_the_scheduler_with_its_offsets() {
    let effect = LightingEffect {
        id: Uuid::new_v4(),
        label: "pulse".into(),
        start_seconds: 2.0,
        end_seconds: 100.0,
        period_seconds: 4.0,
        members: vec![
            EffectMember {
                fixture: "A".into(),
                offset_seconds: 0.0,
            },
            EffectMember {
                fixture: "B".into(),
                offset_seconds: 1.0,
            },
        ],
        cycle: vec![
            EffectStep {
                at_seconds: 0.0,
                property: "dimmer".into(),
                value: vec![1.0],
                fade_seconds: 2.0,
            },
            EffectStep {
                at_seconds: 2.0,
                property: "dimmer".into(),
                value: vec![0.0],
                fade_seconds: 2.0,
            },
        ],
        source: OscCueSource::Captured,
        edited: false,
    };
    let rows = vec![look(
        0.0,
        0.0,
        vec![msg(&dim("A"), 0.0), msg(&dim("B"), 0.0)],
    )];
    let timeline = Timeline::new(&rows, std::slice::from_ref(&effect));
    let mut r = rig(settings(rows, vec![effect])).await;
    r.start().await;
    run_for(30.0).await;
    for name in ["A", "B"] {
        let sent = r.of(&dim(name));
        assert!(sent.len() > 100, "{name}: {}", sent.len());
        for (t, v) in sent.iter().filter(|(t, _)| *t > 3.0) {
            let want = match timeline
                .playback_at(&dim(name), t + LOOK_LEAD_SECONDS)
                .unwrap()[0]
            {
                FixtureArg::Float(x) => x,
                _ => unreachable!(),
            };
            assert!((v - want).abs() < 0.08, "{name} at {t}: {v} vs {want}");
        }
    }
    assert_eq!(r.clears(), 0);
}

// ------------------------------------------------------------------- seek, pause, resume

#[tokio::test(start_paused = true)]
async fn pause_holds_the_look_and_a_resume_re_syncs_at_the_position() {
    let mut r = rig(settings(ramp_song(), vec![])).await;
    r.start().await;
    run_for(20.0).await;
    r.at(EventType::TimerStopRequested, 20.0, false).await;
    let held = r.sent().len();
    run_for(10.0).await;
    assert_eq!(r.sent().len(), held, "paused: no packets");
    assert_eq!(r.clears(), 0, "paused: nothing cleared");
    // resume at 40 s
    r.at(EventType::TimerResumed, 40.0, true).await;
    r.t0 = Instant::now() - Duration::from_secs(40);
    run_for(0.3).await;
    let first = r
        .of(&dim("A"))
        .into_iter()
        .find(|(t, _)| *t >= 40.0)
        .unwrap();
    assert!((first.1 - (39.5 / 60.0)).abs() < 0.02, "{first:?}");
    // a seek back re-syncs to the earlier value
    r.at(EventType::TimerSeeked, 10.0, true).await;
    r.t0 = Instant::now() - Duration::from_secs(10);
    run_for(0.3).await;
    let back = r
        .of(&dim("A"))
        .into_iter()
        .find(|(t, _)| *t >= 10.0 && *t < 10.3)
        .unwrap();
    assert!((back.1 - (9.5 / 60.0)).abs() < 0.02, "{back:?}");
    assert_eq!(r.clears(), 0);
}

// ------------------------------------------------------------------------ the operator

#[tokio::test(start_paused = true)]
async fn the_replays_own_interpolated_values_never_look_like_the_operator() {
    let mut r = rig(settings(ramp_song(), vec![])).await;
    r.start().await;
    r.run_with_echo(30.0).await;
    let status = r.plugin.lightkey_status().await;
    assert_eq!(
        status.yielded_fixtures, None,
        "the echo is not the operator"
    );
    assert!(r.of(&dim("A")).len() > 100, "still replaying");
}

#[tokio::test(start_paused = true)]
async fn an_operator_change_mid_ramp_stops_that_fixture_and_it_comes_back_interpolated() {
    let mut r = rig(settings(ramp_song(), vec![])).await;
    r.start().await;
    r.run_with_echo(20.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.05)], false);
    let at_take_over = r.t0.elapsed().as_secs_f64();
    let a_before = r.of(&dim("A")).len();
    let b_before = r.of(&dim("B")).len();
    r.run_with_echo(OPERATOR_TAKE_BACK_SECONDS - 2.0).await;
    assert_eq!(r.of(&dim("A")).len(), a_before, "A is the operator's");
    assert!(r.of(&dim("B")).len() > b_before + 50, "B keeps moving");
    assert_eq!(r.clears(), 0, "no clear, the operator's change stays");
    let status = r.plugin.lightkey_status().await;
    assert_eq!(status.yielded_fixtures, Some(1));
    // quiet for 30 s: the replay takes A back at the value of that moment
    r.run_with_echo(3.0).await;
    let back = r.of(&dim("A"));
    assert!(back.len() > a_before, "A is driven again");
    let (t, v) = back[a_before];
    assert!(t >= at_take_over + OPERATOR_TAKE_BACK_SECONDS - 0.1, "{t}");
    let want = (t + LOOK_LEAD_SECONDS - 0.5) / 60.0;
    assert!(
        (v - want).abs() < 0.03,
        "taken back at {v}, the ramp is at {want}"
    );
    assert_eq!(r.plugin.lightkey_status().await.yielded_fixtures, None);
    // and it finishes the ramp
    r.run_with_echo(30.0).await;
    assert_eq!(r.of(&dim("A")).last().unwrap().1, 1.0);
    assert_eq!(r.clears(), 0);
}

#[tokio::test(start_paused = true)]
async fn every_new_operator_change_restarts_the_thirty_seconds() {
    let mut r = rig(settings(ramp_song(), vec![])).await;
    r.start().await;
    r.run_with_echo(10.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.9)], false);
    r.run_with_echo(20.0).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("A"), 0.8)], false);
    let held = r.of(&dim("A")).len();
    r.run_with_echo(20.0).await; // 40 s after the first, 20 s after the second
    assert_eq!(r.of(&dim("A")).len(), held);
    r.run_with_echo(11.0).await;
    assert!(r.of(&dim("A")).len() > held);
}

// ------------------------------------------------------------------------------ step bursts

fn big_look_song() -> Vec<OscLightingCue> {
    let all = |x: f64| -> Vec<OscMessage> {
        (0..98).map(|i| msg(&dim(&format!("F{i:02}")), x)).collect()
    };
    vec![
        look(0.0, 0.0, all(0.0)),
        // a ramp elsewhere makes the song smooth
        look(1.0, 20.0, vec![msg(&dim("R"), 0.8)]),
        look(10.0, 0.0, all(1.0)),
    ]
}

#[tokio::test(start_paused = true)]
async fn a_98_value_look_leaves_centred_on_its_row_through_the_scheduler() {
    let mut r = rig(settings(big_look_song(), vec![])).await;
    r.start().await;
    run_for(12.0).await;
    let burst: Vec<(f64, String, f64)> = r
        .sent()
        .into_iter()
        .filter(|(t, a, v)| *t > 5.0 && *v == 1.0 && a.contains("/F"))
        .collect();
    assert_eq!(burst.len(), 98);
    let (first, last) = (burst[0].0, burst[97].0);
    let centre = (first + last) / 2.0;
    assert!(
        (centre - (10.0 - LOOK_LEAD_SECONDS)).abs() < 0.02,
        "centre {centre}"
    );
    assert!((last - first - 0.485).abs() < 0.03, "span {}", last - first);
    // the values go in the documented order: by address inside the class
    let names: Vec<&String> = burst.iter().map(|b| &b.1).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[tokio::test(start_paused = true)]
async fn a_fixture_the_operator_takes_mid_burst_is_skipped_and_the_rest_still_go() {
    let mut r = rig(settings(big_look_song(), vec![])).await;
    r.start().await;
    // the burst runs from about 9.71 s to 10.2 s; fixture F90 is still waiting at 9.8 s
    run_for(9.8).await;
    r.plugin
        .lightkey_fixture_changes(vec![dmx(&dim("F90"), 0.3)], false);
    run_for(3.0).await;
    let burst: Vec<(f64, String, f64)> = r
        .sent()
        .into_iter()
        .filter(|(t, a, v)| *t > 5.0 && *v == 1.0 && a.contains("/F"))
        .collect();
    assert!(
        burst.iter().all(|b| b.1 != dim("F90")),
        "F90 is the operator's now"
    );
    assert_eq!(burst.len(), 97);
    // the skipped value took no slot: no 10 ms hole where it would have gone
    let widest = burst
        .windows(2)
        .map(|w| w[1].0 - w[0].0)
        .fold(0.0, f64::max);
    assert!(widest < 0.0075, "a gap of {widest} s inside the burst");
}
