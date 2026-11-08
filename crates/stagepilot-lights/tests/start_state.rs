//! The starting look of a capture: the relay's state of EVERY fixture when recording begins is
//! stored at the first look, so house lights that never change during a song are replayed too.
//!
//! The rig is the real 66-fixture Sunday project (`tests/fixtures/rig_1108.json`): the names and
//! profiles come from the project's fixture list, the values from a relay ring taken after the
//! 2026-11-08 service. The 31 house/front lights (front wash, house lights, bulbs, the PixelStix
//! driver) never change in the song; every other fixture changes once, at 10 s.

use serde::Deserialize;
use stagepilot_lights::lightkey_fit::{fit_within_caps, Caps};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand, FixtureState};
use stagepilot_lights::lightkey_looks::LookRecorder;
use stagepilot_lights::lightkey_replay::{simulate, ReplayEngine};
use stagepilot_lights::lightkey_smooth::{smooth_rows, SmoothLook};
use stagepilot_lights::lightkey_start::{self, classify, Class};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Deserialize)]
struct RigFixture {
    name: String,
    profile: String,
}

#[derive(Deserialize)]
struct RigFile {
    fixtures: Vec<RigFixture>,
    state: Vec<FixtureCommand>,
    static_profiles: Vec<String>,
}

fn rig() -> RigFile {
    serde_json::from_str(include_str!("fixtures/rig_1108.json")).unwrap()
}

fn fixture_of(c: &FixtureCommand) -> String {
    c.fixture().unwrap().to_string()
}

/// A value different from `v`, to make a change.
fn moved(c: &FixtureCommand) -> FixtureCommand {
    FixtureCommand::new(
        c.a.clone(),
        c.v.iter()
            .map(|a| match a {
                FixtureArg::Float(x) => FixtureArg::Float(if *x > 0.5 { x - 0.3 } else { x + 0.3 }),
                FixtureArg::Int(i) => FixtureArg::Int(i + 1),
                FixtureArg::Bool(b) => FixtureArg::Bool(!b),
                FixtureArg::Text(t) => FixtureArg::Text(format!("{t}2")),
            })
            .collect(),
    )
}

struct Pass {
    changes: Vec<(f64, FixtureCommand)>,
    baseline: Vec<FixtureCommand>,
    applied: lightkey_start::Applied,
    static_fixtures: BTreeSet<String>,
}

/// One pass over the real rig: every non-static fixture changes all its properties at 10 s.
fn pass() -> Pass {
    let file = rig();
    let profile: BTreeMap<&str, &str> = file
        .fixtures
        .iter()
        .map(|f| (f.name.as_str(), f.profile.as_str()))
        .collect();
    let static_fixtures: BTreeSet<String> = file
        .fixtures
        .iter()
        .filter(|f| file.static_profiles.contains(&f.profile))
        .map(|f| f.name.clone())
        .collect();
    let mut state = FixtureState::new();
    state.apply_all(&file.state);
    let mut recorder = LookRecorder::new(&state);
    let moving: Vec<FixtureCommand> = file
        .state
        .iter()
        .filter(|c| !static_fixtures.contains(&fixture_of(c)))
        .map(moved)
        .collect();
    assert!(!moving.is_empty());
    recorder.observe(10.0, &moving);
    assert_eq!(static_fixtures.len(), 31);
    assert_eq!(profile.len(), 66);
    let raw = recorder.raw_with_start();
    let (at, snapshot) = raw.start.expect("the relay state was read");
    assert_eq!(at, 0.0);
    let mut changes = raw.changes;
    let applied = lightkey_start::apply(&mut changes, at, &snapshot);
    Pass {
        changes,
        baseline: raw.baseline,
        applied,
        static_fixtures,
    }
}

fn first_look(rows: &[SmoothLook]) -> Vec<&FixtureCommand> {
    rows.iter()
        .filter(|r| r.at_seconds <= 0.0)
        .flat_map(|r| r.messages.iter())
        .collect()
}

// ----------------------------------------------------------------------------- the recorder

#[test]
fn a_pass_over_the_real_rig_starts_with_all_66_fixtures_and_dark_ones_reduced() {
    let pass = pass();
    let fitted = fit_within_caps(&pass.baseline, &pass.changes, 60.0, &Caps::default());
    let rows = &fitted.capture.rows;
    let start = first_look(rows);
    let lights: BTreeSet<String> = start.iter().map(|c| fixture_of(c)).collect();
    assert_eq!(lights.len(), 66, "every fixture is in the first look");
    assert_eq!(
        lightkey_start::lights_at_start(rows, 0.0),
        66,
        "and the count the status shows says so"
    );
    let of = |fixture: &str| -> BTreeMap<String, Vec<FixtureArg>> {
        start
            .iter()
            .filter(|c| c.fixture() == Some(fixture))
            .map(|c| (c.a.rsplit('/').next().unwrap().to_string(), c.v.clone()))
            .collect()
    };
    // P29: static and lit (front wash): dimmer and both whites
    let p29 = of("P29");
    assert_eq!(
        p29.keys().cloned().collect::<Vec<_>>(),
        ["coolWhite", "dimmer", "warmWhite"]
    );
    // P33: static and dark (dimmer 0): only the dimmer that keeps it dark
    let p33 = of("P33");
    assert_eq!(p33.keys().cloned().collect::<Vec<_>>(), ["dimmer"]);
    assert_eq!(p33["dimmer"], vec![FixtureArg::Float(0.0)]);
    // P42: static, no dimmer, both whites 0: only what keeps it dark (its colours)
    let p42 = of("P42");
    assert_eq!(
        p42.keys().cloned().collect::<Vec<_>>(),
        ["coolWhite", "warmWhite"]
    );
    // P25: lit RGBW house light: dimmer, black colour and the warm white all stored
    assert_eq!(
        of("P25").keys().cloned().collect::<Vec<_>>(),
        ["color", "dimmer", "warmWhite"]
    );
    // a fixture that does change keeps its start value, and then changes at 10 s
    let file = rig();
    let mh1_dimmer = file
        .state
        .iter()
        .find(|c| c.a == "/fixture/MH1/overrides/dimmer")
        .unwrap();
    assert_eq!(of("MH1")["dimmer"], mh1_dimmer.v);
    let later: Vec<f64> = rows
        .iter()
        .filter(|r| {
            r.messages
                .iter()
                .any(|m| m.a == "/fixture/MH1/overrides/dimmer")
        })
        .map(|r| r.at_seconds)
        .collect();
    assert!(
        later.iter().any(|t| (t - 10.0).abs() < 0.5),
        "MH1 changes at 10 s: {later:?}"
    );
    // a mover's wheels that are not in use are not stored on a static fixture, and the
    // properties that are not appearance (lit, speeds, modes) never are
    for c in &start {
        let property = c.a.rsplit('/').next().unwrap();
        assert!(
            classify(property).is_some() || !pass.static_fixtures.contains(&fixture_of(c)),
            "{property} is not part of a look"
        );
    }
    // the numbers the PR reports
    let values = start.len();
    let static_values = pass.applied.static_values;
    println!(
        "START LOOK real rig: {} fixtures, {values} values at the first look \
         ({static_values} of them from the 31 fixtures that never change; {} of those were dark \
         and reduced)",
        lights.len(),
        pass.applied.dark_fixtures
    );
    assert_eq!(values, REAL_RIG_FIRST_LOOK_VALUES);
    assert_eq!(static_values, REAL_RIG_STATIC_VALUES);
}

/// The real rig, in a pass where every other fixture changes all its properties: 154 values at
/// the first look (49 from the 31 fixtures that never change, 105 from the 35 that do).
const REAL_RIG_FIRST_LOOK_VALUES: usize = 154;
const REAL_RIG_STATIC_VALUES: usize = 49;

#[test]
fn without_the_start_state_the_house_lights_are_missing() {
    // what the recorder alone produced before this change
    let file = rig();
    let mut state = FixtureState::new();
    state.apply_all(&file.state);
    let mut recorder = LookRecorder::new(&state);
    let moving: Vec<FixtureCommand> = file
        .state
        .iter()
        .filter(|c| c.a.contains("/MH1/"))
        .map(moved)
        .collect();
    recorder.observe(10.0, &moving);
    let (baseline, changes) = recorder.raw();
    let fitted = fit_within_caps(&baseline, &changes, 60.0, &Caps::default());
    let fixtures: HashSet<String> = fitted
        .capture
        .rows
        .iter()
        .flat_map(|r| r.messages.iter())
        .map(fixture_of)
        .collect();
    assert_eq!(fixtures.len(), 1, "only the fixture that changed");
}

#[test]
fn the_property_rule_follows_the_names() {
    let cmd = |a: &str, v: Vec<f64>| {
        FixtureCommand::new(
            format!("/fixture/{a}"),
            v.into_iter().map(FixtureArg::Float).collect(),
        )
    };
    let none: HashSet<&str> = HashSet::new();
    // a lit mover keeps pan, tilt, zoom, focus and a gobo that is in use; not an open wheel,
    // speeds or modes
    let mover = vec![
        cmd("M/overrides/dimmer", vec![0.8]),
        cmd("M/overrides/color", vec![1.0, 0.0, 0.0]),
        cmd("M/overrides/panAngle", vec![-90.0]),
        cmd("M/overrides/tiltAngle", vec![20.0]),
        cmd("M/overrides/zoomAngle", vec![12.0]),
        cmd("M/overrides/focus", vec![0.4]),
        cmd("M/overrides/gobo1", vec![3.0]),
        cmd("M/overrides/frostAmount", vec![0.0]),
        cmd("M/overrides/goboRotationSpeed1", vec![0.5]),
        cmd("M/overrides/panTiltSpeed", vec![1.0]),
    ];
    let kept: Vec<String> = lightkey_start::start_look(&mover, &none)
        .commands
        .iter()
        .map(|c| c.a.rsplit('/').next().unwrap().to_string())
        .collect();
    assert_eq!(
        kept,
        [
            "dimmer",
            "color",
            "panAngle",
            "tiltAngle",
            "zoomAngle",
            "focus",
            "gobo1"
        ]
    );
    // the same mover dark: only its dimmer
    let mut dark = mover.clone();
    dark[0] = cmd("M/overrides/dimmer", vec![0.0]);
    let kept = lightkey_start::start_look(&dark, &none);
    assert_eq!(kept.commands.len(), 1);
    assert_eq!(kept.dark_fixtures, 1);
    // but a dark mover that changes in the song keeps its other static properties
    let changing: HashSet<&str> = ["/fixture/M/overrides/dimmer"].into_iter().collect();
    let kept = lightkey_start::start_look(&dark, &changing);
    assert_eq!(kept.dark_fixtures, 0, "it changes, so it is not reduced");
    assert!(kept.commands.len() >= 6);
    // no dimmer, black colour: only the colour; with a coloured pixel the colour
    let pixel = |rgb: Vec<f64>| {
        vec![
            cmd("P/overrides/color", rgb),
            cmd("P/overrides/lit", vec![0.0]),
        ]
    };
    let black = lightkey_start::start_look(&pixel(vec![0.0, 0.0, 0.0]), &none);
    assert_eq!(black.commands.len(), 1);
    assert_eq!(black.dark_fixtures, 1);
    assert_eq!(
        lightkey_start::start_look(&pixel(vec![0.3, 0.0, 0.0]), &none).dark_fixtures,
        0
    );
    assert_eq!(classify("fogAmount"), Some(Class::Level));
}

#[test]
fn a_pass_that_begins_late_stamps_the_start_state_at_that_song_position() {
    let file = rig();
    let mut state = FixtureState::new();
    state.apply_all(&file.state);
    // the stream was not live at the song's start: no baseline, the first stretch opens at 12 s
    let mut recorder = LookRecorder::new(&FixtureState::new());
    recorder.begin_stretch(12.0, &state);
    recorder.observe(20.0, &[moved(&file.state[0])]);
    let raw = recorder.raw_with_start();
    let (at, snapshot) = raw.start.unwrap();
    assert_eq!(at, 12.0);
    assert_eq!(snapshot.len(), file.state.len());
    let mut changes = raw.changes;
    let applied = lightkey_start::apply(&mut changes, at, &snapshot);
    assert_eq!(applied.at, 12.0);
    let fitted = fit_within_caps(&raw.baseline, &changes, 60.0, &Caps::default());
    assert_eq!(
        lightkey_start::lights_at_start(&fitted.capture.rows, 12.0),
        66
    );
    assert!(
        fitted.capture.rows.iter().all(|r| r.at_seconds >= 12.0),
        "nothing before the recording began"
    );
}

#[test]
fn a_relay_that_gave_no_state_leaves_no_start_state() {
    let mut recorder = LookRecorder::new(&FixtureState::new());
    recorder.observe(
        1.0,
        &[FixtureCommand::new(
            "/fixture/A/overrides/dimmer",
            vec![FixtureArg::Float(0.5)],
        )],
    );
    assert!(recorder.raw_with_start().start.is_none());
}

// ----------------------------------------------------------------------------- the replay

#[test]
fn the_first_look_of_the_real_rig_goes_out_in_well_under_a_second_at_200_packets_a_second() {
    let pass = pass();
    let fitted = fit_within_caps(&pass.baseline, &pass.changes, 60.0, &Caps::default());
    let values = first_look(&fitted.capture.rows).len();
    let rows = smooth_rows(&fitted.capture.rows);
    let mut engine = ReplayEngine::new(&rows, &fitted.capture.effects);
    // a simulated clock from the song's start: no wall clock, no sockets
    let packets = simulate(&mut engine, 0.0, 5.0, 0.045, &|_, _| false);
    let first: Vec<_> = packets.iter().filter(|p| p.at < 5.0).collect();
    assert_eq!(
        first.len(),
        values,
        "one packet per value of the first look"
    );
    assert!(
        packets.iter().all(|p| p.at >= 0.0 && p.position >= 0.0),
        "nothing before song position 0"
    );
    let last = first.iter().map(|p| p.at).fold(0.0, f64::max);
    println!(
        "FIRST LOOK real rig: {values} packets, last one leaves {:.3} s after the start \
         (simulated clock, 200 packets a second)",
        last
    );
    // 200 packets a second: values x 5 ms; generous bound, the budget is one second
    assert!(last < 1.0, "first look took {last} s");
    // the worst case: no fixture changes in the song, so the whole rig is stored as a static look
    let file = rig();
    let mut state = FixtureState::new();
    state.apply_all(&file.state);
    let worst = lightkey_start::start_look(&state.commands(), &HashSet::new());
    println!(
        "FIRST LOOK worst case (nothing changes in the song): {} values = {:.3} s ({} fixtures dark and reduced)",
        worst.commands.len(),
        worst.commands.len() as f64 * 0.005,
        worst.dark_fixtures
    );
    assert!(worst.commands.len() as f64 * 0.005 < 1.0);
    assert!(last >= (values as f64 - 1.0) * 0.005 - 1e-9);
}

// ------------------------------------------------------------ songs captured before this

mod old_captures {
    use stagepilot_core::config::{
        CaptureStatus, LightkeyMode, OscCueSource, OscLightingCue, SongLightingCueMap,
    };
    use stagepilot_lights::lightkey_auto::{decide, song_capture, Action, Reason, SongCapture};
    use stagepilot_lights::lightkey_looks::{save_capture, CaptureSave};
    use uuid::Uuid;

    fn row() -> OscLightingCue {
        OscLightingCue {
            id: Uuid::new_v4(),
            at_seconds: 0.0,
            address: String::new(),
            args: vec![],
            label: String::new(),
            color: None,
            messages: vec![stagepilot_core::config::OscMessage {
                address: "/fixture/A/overrides/dimmer".into(),
                args: vec![stagepilot_core::config::OscArgument::Float(0.5)],
            }],
            source: OscCueSource::Captured,
            edited: false,
            fade_seconds: 0.0,
        }
    }

    fn status(complete: bool, coverage: f64, start_lights: Option<u32>) -> CaptureStatus {
        CaptureStatus {
            complete,
            coverage_seconds: coverage,
            recorded_at: "2026-11-08T00:00:00Z".into(),
            project: None,
            simplified: None,
            start_lights,
        }
    }

    fn old_map() -> SongLightingCueMap {
        SongLightingCueMap {
            song_key: "s".into(),
            song_title: "S".into(),
            cues: vec![],
            osc_cues: vec![row()],
            loudness: None,
            captured_edited: false,
            capture: Some(status(true, 300.0, None)),
            effects: vec![],
        }
    }

    /// Auto (Record off) never re-records on its own: an old capture that was complete still
    /// replays. Only an armed Record (or a pinned one) records the song again.
    #[test]
    fn auto_keeps_replaying_an_old_complete_capture_and_an_armed_record_records_it_again() {
        let old = old_map();
        assert!(old.recorded_before_start_look());
        assert_eq!(song_capture(Some(&old)), SongCapture::Complete);
        let auto = decide(LightkeyMode::Auto, true, SongCapture::Complete);
        assert_eq!(auto.action, Action::Replay);
        assert_eq!(auto.reason, Reason::AutoFullyCaptured);
        // Record armed: effective mode is Record, which captures every song
        let armed = decide(LightkeyMode::Record, true, SongCapture::Complete);
        assert_eq!(armed.action, Action::Capture);
        // and Replay pinned replays it, untouched
        assert_eq!(
            decide(LightkeyMode::Replay, true, SongCapture::Complete).action,
            Action::Replay
        );
    }

    #[test]
    fn a_complete_armed_pass_replaces_the_old_capture_with_one_that_has_the_start_look() {
        let old = old_map();
        let saved = save_capture(
            Some(&old),
            "s",
            "S",
            vec![row(), row()],
            status(true, 300.0, Some(66)),
        );
        let CaptureSave::Saved(map) = saved else {
            panic!("a complete pass replaces the capture");
        };
        assert_eq!(map.capture.as_ref().unwrap().start_lights, Some(66));
        assert!(!map.recorded_before_start_look());
        assert_eq!(map.osc_cues.len(), 2);
    }

    #[test]
    fn a_partial_pass_never_replaces_a_complete_old_capture_and_nothing_is_dropped() {
        let old = old_map();
        let saved = save_capture(
            Some(&old),
            "s",
            "S",
            vec![row()],
            status(false, 40.0, Some(66)),
        );
        assert_eq!(saved, CaptureSave::Kept);
    }

    #[test]
    fn with_record_off_nothing_in_the_old_capture_changes() {
        let old = old_map();
        let before = serde_json::to_value(&old).unwrap();
        // the notice and the decision read the map; they never write it
        let _ = old.recorded_before_start_look();
        let _ = song_capture(Some(&old));
        assert_eq!(serde_json::to_value(&old).unwrap(), before);
        assert!(before["capture"].get("start_lights").is_none());
    }
}

#[test]
fn the_starting_look_counts_toward_the_caps_and_the_pass_still_ends_saved() {
    let pass = pass();
    let caps = Caps {
        rows: 60,
        messages: 200,
        effects: 10,
        cycle_steps: 100,
    };
    let fitted = fit_within_caps(&pass.baseline, &pass.changes, 60.0, &caps);
    let rows = smooth_rows(&fitted.capture.rows);
    let messages: usize = rows.iter().map(|r| r.messages.len()).sum();
    assert!(rows.len() <= caps.rows, "{} rows", rows.len());
    assert!(messages <= caps.messages, "{messages} messages");
    assert!(!rows.is_empty(), "never discarded for size");
    println!(
        "CAPS: start look of the real rig inside {} rows / {} messages: {} rows, {messages} messages, {} fixtures at the first look (trimmed {})",
        caps.rows,
        caps.messages,
        rows.len(),
        lightkey_start::lights_at_start(&fitted.capture.rows, 0.0),
        fitted.trimmed
    );
    assert!(fitted.trimmed > 0, "the caps were small enough to trim");
    // a trim drops the least important parts first and the final value of an address last: the
    // 31 fixtures that never change (one value each) all keep their starting look; of the fixtures
    // that do change, the trim may drop the start value of the least important ones
    let first = first_look(&fitted.capture.rows);
    let lights: BTreeSet<String> = first.iter().map(|c| fixture_of(c)).collect();
    assert!(
        pass.static_fixtures.iter().all(|f| lights.contains(f)),
        "every static fixture keeps its starting look"
    );
}
