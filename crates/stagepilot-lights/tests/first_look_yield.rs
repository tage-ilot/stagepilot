//! F-H2: the first look of a song must not make the moving heads "operator-touched".
//!
//! Real data from the 2026-11-08 rc.10 rig test (`fixtures/first_look_rig.json`): the plan rows of
//! the heads (MH1-MH4) and two other lights (Z1, Z2) of the saved songs "Ever Be" and "Elohim",
//! and the Lighting computer helper's `/fixtures/log` events around three song starts. Nobody
//! touched anything in those runs, yet 3, 1 and 4-5 lights were counted as the operator's. The
//! harness replays the plan on a simulated clock from the song position the run started at and
//! judges every event of the ring against the sends, once with the rc.10 echo rules (it must
//! reproduce the false yields, or the test proves nothing) and once with the current rules.
//!
//! The simulated clock is aligned to the rig by the first events of each ring. How late the
//! relay reports an event is not known to the millisecond, so every result must hold over a grid
//! of start offsets and relay lags, not for one lucky value.

use serde_json::Value;
use stagepilot_core::config::{LightingEffect, OscArgument, OscLightingCue};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_operator::EchoRules;
use stagepilot_lights::lightkey_yield_check::{run_start, Report, StartCase};
use std::collections::BTreeSet;

struct Song {
    rows: Vec<OscLightingCue>,
    effects: Vec<LightingEffect>,
}

struct Ring {
    /// Epoch ms of the play press.
    play_ms: f64,
    events: Vec<(f64, FixtureCommand)>,
}

fn data() -> Value {
    serde_json::from_str(include_str!("fixtures/first_look_rig.json")).expect("fixture json")
}

fn song(name: &str) -> Song {
    let d = data();
    Song {
        rows: serde_json::from_value(d["songs"][name]["rows"].clone()).unwrap(),
        effects: serde_json::from_value(d["songs"][name]["effects"].clone()).unwrap(),
    }
}

fn ring(name: &str) -> Ring {
    let d = data();
    let r = &d["rings"][name];
    let play_ms = r["play_epoch"].as_f64().unwrap() * 1000.0;
    let mut events = Vec::new();
    for e in r["events"].as_array().unwrap() {
        let t = play_ms + e["t_rel_play"].as_f64().unwrap() * 1000.0;
        for o in e["osc"].as_array().unwrap() {
            let v = o["v"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| match x {
                    Value::Number(n) if n.is_i64() && !n.to_string().contains('.') => {
                        FixtureArg::Int(n.as_i64().unwrap())
                    }
                    Value::Number(n) => FixtureArg::Float(n.as_f64().unwrap()),
                    other => panic!("unexpected value {other}"),
                })
                .collect();
            events.push((t, FixtureCommand::new(o["a"].as_str().unwrap(), v)));
        }
    }
    Ring { play_ms, events }
}

/// The ring's events up to `seconds` after the play press (the harness cannot model what
/// Lightkey's own effects do later).
fn until(ring: &Ring, seconds: f64) -> Vec<(f64, FixtureCommand)> {
    ring.events
        .iter()
        .filter(|(t, _)| *t <= ring.play_ms + seconds * 1000.0)
        .cloned()
        .collect()
}

fn names(report: &Report, which: &[&str]) -> BTreeSet<String> {
    report
        .flagged()
        .into_iter()
        .filter(|f| which.contains(&f.as_str()))
        .collect()
}

const HEADS: [&str; 4] = ["MH1", "MH2", "MH3", "MH4"];

fn case(
    ring: &Ring,
    from_s: f64,
    start_rel_s: f64,
    lag_ms: f64,
    baseline: &[(&str, f64)],
    rules: EchoRules,
) -> StartCase {
    StartCase {
        from_s,
        length_s: 8.0,
        engine_wall_ms: ring.play_ms + start_rel_s * 1000.0,
        relay_lag_ms: lag_ms,
        baseline: baseline.iter().map(|(a, x)| (a.to_string(), *x)).collect(),
        rules,
    }
}

// ------------------------------------------------------------------------ Ever Be from the top

/// The first run of the test: the song starts, 12 s in the first look of the song's lights goes out
/// (the heads that were the operator's in the test before are the replay's again) and the heads
/// glide about 70 degrees in under a second, then settle with a small back and forth. The
/// engine's first tick on the rig is at song position 13.52 s, 12.03 s after the play press.
#[test]
fn ever_be_from_the_top_does_not_yield_the_heads() {
    let song = song("ever_be");
    let ring = ring("ever1");
    for start in [11.98, 12.03] {
        for lag in [30.0, 60.0, 100.0, 150.0] {
            let new = run_start(
                &song.rows,
                &song.effects,
                &ring.events,
                &case(&ring, 13.52, start, lag, &[], EchoRules::current()),
            );
            assert!(
                names(&new, &HEADS).is_empty(),
                "start {start}, lag {lag} ms: yielded {:?}; first: {:?}",
                new.flagged(),
                new.flags.first()
            );
            assert_eq!(
                HEADS.iter().filter(|h| new.held.contains(**h)).count(),
                4,
                "MH1-MH4 are held: {:?}",
                new.held
            );
            assert!(new.relay_events > 100, "{}", new.relay_events);

            // the same ring with the rc.10 rules: the heads are taken without any operator
            let old = run_start(
                &song.rows,
                &song.effects,
                &ring.events,
                &case(&ring, 13.52, start, lag, &[], EchoRules::rc10()),
            );
            assert!(
                !names(&old, &HEADS).is_empty(),
                "start {start}, lag {lag} ms: the rc.10 rules must reproduce the false yield"
            );
        }
    }
}

// ------------------------------------------------------------------------ Elohim from 234 s

/// Elohim played from 234 s: the heads stood at tilt 90.529 (the old position), the first look
/// sends 75.7 / 66.2 / 75.7, and the old position is reported once more right after, by MH1, MH3
/// and MH4. The relay's state before the first look is not in the ring (the helper only logs
/// changes), so the test states it: the old position the finding names.
#[test]
fn elohim_from_234_s_does_not_yield_mh1_mh3_mh4() {
    let song = song("elohim");
    let ring = ring("elo3");
    let baseline = [
        ("/fixture/MH1/overrides/tiltAngle", 90.5294),
        ("/fixture/MH3/overrides/tiltAngle", 90.5294),
        ("/fixture/MH4/overrides/tiltAngle", 90.5294),
    ];
    let judged = until(&ring, 2.0);
    for start in [-0.90, -0.85, -0.80] {
        for lag in [30.0, 60.0, 100.0] {
            let new = run_start(
                &song.rows,
                &song.effects,
                &judged,
                &case(&ring, 234.0, start, lag, &baseline, EchoRules::current()),
            );
            assert!(
                names(&new, &HEADS).is_empty(),
                "start {start}, lag {lag} ms: yielded {:?}; first: {:?}",
                new.flagged(),
                new.flags.first()
            );
            // the plan values the finding names: 75.7 / 66.2 / 75.7
            for (head, want) in [("MH1", 75.7059), ("MH3", 66.1765), ("MH4", 75.7059)] {
                let address = format!("/fixture/{head}/overrides/tiltAngle");
                let first = new
                    .sent
                    .iter()
                    .find(|s| s.address == address)
                    .unwrap_or_else(|| panic!("{head} tilt sent"));
                assert!(
                    matches!(first.args.as_slice(), [OscArgument::Float(x)] if (x - want).abs() < 0.01),
                    "{head}: {:?}",
                    first.args
                );
            }

            let old = run_start(
                &song.rows,
                &song.effects,
                &judged,
                &case(&ring, 234.0, start, lag, &baseline, EchoRules::rc10()),
            );
            let hit = names(&old, &["MH1", "MH3", "MH4"]);
            assert_eq!(
                hit,
                ["MH1", "MH3", "MH4"].iter().map(|s| s.to_string()).collect(),
                "start {start}, lag {lag} ms: rc.10 yields exactly the three heads with a stale frame"
            );
            // at the aligned start (the sends 40 ms before their DMX) the event that takes the
            // heads is the stale tilt frame, as on the rig (an early start makes the burst's
            // dimmer echoes later than 200 ms and they take a head first)
            if start == -0.85 && lag == 30.0 {
                let stale = old.flags.iter().any(|f| {
                    f.command.a.ends_with("/tiltAngle")
                        && matches!(f.command.v.as_slice(), [FixtureArg::Float(x)] if (x - 90.5294).abs() < 1e-3)
                });
                assert!(stale, "start {start}, lag {lag} ms: {:?}", old.flags);
            }
        }
    }
}

// ------------------------------------------------------------------------ Ever Be from 175 s

/// Ever Be played from 175 s: Z2 stood at tilt 90.0 and the first look sends 68.0.
#[test]
fn ever_be_from_175_s_does_not_yield_z2() {
    let song = song("ever_be");
    let ring = ring("ever2");
    let baseline = [("/fixture/Z2/overrides/tiltAngle", 90.0)];
    let judged = until(&ring, 2.0);
    for start in [-0.95, -0.90, -0.85] {
        for lag in [30.0, 60.0, 100.0] {
            let new = run_start(
                &song.rows,
                &song.effects,
                &judged,
                &case(&ring, 175.0, start, lag, &baseline, EchoRules::current()),
            );
            assert!(
                names(&new, &["Z2"]).is_empty(),
                "start {start}, lag {lag} ms: yielded {:?}; first: {:?}",
                new.flagged(),
                new.flags.first()
            );
            assert!(new.held.contains("Z2"));
            let old = run_start(
                &song.rows,
                &song.effects,
                &judged,
                &case(&ring, 175.0, start, lag, &baseline, EchoRules::rc10()),
            );
            assert_eq!(
                names(&old, &["Z2"]),
                ["Z2".to_string()].into_iter().collect(),
                "start {start}, lag {lag} ms: rc.10 yields Z2"
            );
        }
    }
}

// ------------------------------------------------------------------------ the takeover

/// The same test run, later: the operator sets Z1's pan to -20 and then +20 by hand (values the
/// plan, which holds Z1 at -90, never has). Z1.panAngle -20.0 at +191.7 s and +19.99 at +192.9 s.
/// The replay must take that at once and leave Z1 alone (the 30 s are in
/// `lightkey_operator::tests` and the plugin test `a_hand_set_pan_of_minus_20_...`).
#[test]
fn a_hand_set_pan_of_minus_20_then_plus_20_on_z1_is_the_operator() {
    let song = song("ever_be");
    let play_ms = 1_794_191_000_000.0;
    let z1 = "/fixture/Z1/overrides/panAngle";
    let events = vec![
        (
            play_ms + 191_700.0,
            FixtureCommand::new(z1, vec![FixtureArg::Float(-20.0)]),
        ),
        (
            play_ms + 192_900.0,
            FixtureCommand::new(z1, vec![FixtureArg::Float(19.99)]),
        ),
    ];
    for lag in [30.0, 100.0, 150.0] {
        for rules in [EchoRules::current(), EchoRules::rc10()] {
            let case = StartCase {
                from_s: 175.0,
                length_s: 30.0,
                // the engine started at song position 175 s, 175 s before the song time of the events
                engine_wall_ms: play_ms + 175_000.0,
                relay_lag_ms: lag,
                baseline: vec![(z1.to_string(), -89.996)],
                rules,
            };
            let report = run_start(&song.rows, &song.effects, &events, &case);
            assert_eq!(report.flags.len(), 1, "{:?}", report.flags);
            let flag = &report.flags[0];
            assert_eq!(flag.fixture, "Z1");
            // taken on the first event, within 200 ms of it
            let at_event = 191.7 - 175.0;
            assert!(
                flag.wall_s >= at_event && flag.wall_s - at_event <= 0.2,
                "yielded at {} s, the event was at {at_event} s",
                flag.wall_s
            );
            // the second hand-set value only restarts the quiet time
            assert_eq!(report.restarts.len(), 1, "{:?}", report.restarts);
            assert_eq!(report.restarts[0].0, "Z1");
        }
    }
}

// ------------------------------------------------------------------------ the rest of the plan

#[test]
fn the_plan_values_do_not_depend_on_the_echo_rules() {
    // The tracker only listens: what the engine sends is the same under both rule sets.
    let song = song("elohim");
    let ring = ring("elo3");
    let a = run_start(
        &song.rows,
        &song.effects,
        &ring.events,
        &case(&ring, 234.0, -0.85, 60.0, &[], EchoRules::current()),
    );
    let b = run_start(
        &song.rows,
        &song.effects,
        &ring.events,
        &case(&ring, 234.0, -0.85, 60.0, &[], EchoRules::rc10()),
    );
    assert_eq!(a.pass_packets, b.pass_packets);
    assert!(a.pass_packets[0] > 40, "{}", a.pass_packets[0]);
}
