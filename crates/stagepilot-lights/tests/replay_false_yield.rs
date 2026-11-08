//! The replay must never take its own fast sweep for the operator (field test 2026-11-08, saved
//! song "Elohim": MH1-MH4 were dropped after a 0.15 s pan/tilt sweep and stayed frozen across a
//! jump). The fixture holds the song's MH1-MH4 plan rows and the DMX the Lighting Mac helper
//! logged for them; the harness replays the plan on a simulated clock (no sockets).

use serde_json::Value;
use stagepilot_core::config::{LightingEffect, OscLightingCue};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_yield_check::{run, Timeline};

fn load() -> (
    Vec<OscLightingCue>,
    Vec<LightingEffect>,
    Vec<(f64, FixtureCommand)>,
) {
    let d: Value =
        serde_json::from_str(include_str!("fixtures/false_yield_mh.json")).expect("fixture json");
    let rows = serde_json::from_value(d["rows"].clone()).unwrap();
    let effects = serde_json::from_value(d["effects"].clone()).unwrap();
    let mut relay = Vec::new();
    for e in d["ring"].as_array().unwrap() {
        let t = e["t"].as_f64().unwrap();
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
            relay.push((t, FixtureCommand::new(o["a"].as_str().unwrap(), v)));
        }
    }
    (rows, effects, relay)
}

#[test]
fn the_real_elohim_sweep_does_not_yield_the_moving_heads() {
    let (rows, effects, relay) = load();
    // The relay lag (OSC out -> Lightkey -> sACN -> helper -> relay) is not known to the
    // millisecond and the simulated clock is within about 40 ms of the rig's: the result must
    // hold across the plausible range, not for one lucky value.
    for lag in [80.0, 100.0, 125.0, 150.0, 190.0] {
        let timeline = Timeline {
            relay_lag_ms: lag,
            ..Timeline::elohim()
        };
        let report = run(&rows, &effects, &relay, &timeline);
        assert!(
            report.flagged().is_empty(),
            "lag {lag} ms: yielded {:?}; first: {:?}",
            report.flagged(),
            report.flags.first()
        );
        assert_eq!(report.held.len(), 4, "MH1-MH4 are held: {:?}", report.held);
        assert!(report.relay_events > 150, "{}", report.relay_events);
    }
}

#[test]
fn the_two_replay_passes_send_the_plan_values_unchanged() {
    // The detector only listens; what the engine sends does not depend on it. The counts are the
    // plan's MH1-MH4 packets of the two passes (0-37.5 s and 182.7-205.9 s).
    let (rows, effects, relay) = load();
    let report = run(&rows, &effects, &relay, &Timeline::elohim());
    assert_eq!(report.pass_packets, [PASS1_PACKETS, PASS2_PACKETS]);
}

#[test]
fn the_sweep_needs_the_path_rule() {
    // Without the rule these DMX values are not within tolerance of anything the replay sent in
    // the last 200 ms: the sweep shows values between the 50 ms ticks. If this ever reads 0 the
    // test above no longer proves the fix.
    let (rows, effects, relay) = load();
    let timeline = Timeline::elohim();
    let report = run(&rows, &effects, &relay, &timeline);
    let mut strangers = 0;
    for (t, cmd) in &relay {
        let at = t + timeline.relay_lag_ms;
        if !cmd.a.ends_with("Angle") || *t < timeline.start_ms + 26_000.0 {
            continue;
        }
        if *t > timeline.start_ms + 27_500.0 {
            continue;
        }
        let FixtureArg::Float(seen) = cmd.v[0].clone() else {
            continue;
        };
        let near = report.sent.iter().any(|s| {
            s.address == cmd.a
                && at - s.wall_ms >= 0.0
                && at - s.wall_ms <= 200.0
                && matches!(s.args.as_slice(), [stagepilot_core::config::OscArgument::Float(x)] if (x - seen).abs() <= 1.0)
        });
        if !near {
            strangers += 1;
        }
    }
    assert!(strangers > 5, "values nothing sent: {strangers}");
}

const PASS1_PACKETS: usize = 424;
const PASS2_PACKETS: usize = 824;
