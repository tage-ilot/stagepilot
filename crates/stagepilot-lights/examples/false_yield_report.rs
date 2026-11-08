//! Offline reproduction of a "false yield": the replay judging its own fast sweep to be the
//! operator. No sockets, no sleeping.
//!
//! ```text
//! cargo run --release -p stagepilot-lights --example false_yield_report -- <settings.json> <ring.json> [song key]
//! ```
//!
//! `settings.json` is a StagePilot settings file (the plan is `lights.cue_maps[<key>]`), `ring.json`
//! is the Lighting computer helper's fixture log (`events[]` with `t` in epoch ms and `osc`
//! `{a, v}`). The timeline of the 2026-11-08 "Elohim" test is built in
//! ([`Timeline::elohim`]); `PLAY_START_MS`, `PAUSE1_S`, `JUMP_TO_S`, `RESUME_MS`, `PAUSE2_MS`,
//! `SETTLE_S`, `PASS2_JUDGE_S` and `RELAY_LAG_MS` (delay of a relay event after the DMX time stamp,
//! default 100) change it. `SENDS_OUT=<file>` writes the simulated sends as JSON.
//!
//! The report names the first event that flags each fixture (value, time, what the engine had sent,
//! the echo window) and prints the packet count of the two passes (the plan values; they do not
//! depend on the detector).

use serde_json::Value;
use stagepilot_core::config::{LightingEffect, OscArgument, OscLightingCue};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_yield_check::{echo_window_ms, run, Timeline};

fn env_f(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn arg(value: &Value) -> Option<FixtureArg> {
    match value {
        Value::Bool(b) => Some(FixtureArg::Bool(*b)),
        Value::Number(n) if n.is_i64() && !n.to_string().contains('.') => {
            n.as_i64().map(FixtureArg::Int)
        }
        Value::Number(n) => n.as_f64().map(FixtureArg::Float),
        Value::String(s) => Some(FixtureArg::Text(s.clone())),
        _ => None,
    }
}

fn floats(args: &[OscArgument]) -> String {
    args.iter()
        .map(|a| match a {
            OscArgument::Float(f) => format!("{f:.3}"),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: false_yield_report <settings.json> <ring.json> [song key]");
        std::process::exit(2);
    }
    let key = a.get(3).cloned().unwrap_or_else(|| "27859457".into());
    let settings: Value = serde_json::from_slice(&std::fs::read(&a[1]).unwrap()).unwrap();
    let ring: Value = serde_json::from_slice(&std::fs::read(&a[2]).unwrap()).unwrap();
    let map = &settings["lights"]["cue_maps"][&key];
    let rows: Vec<OscLightingCue> = serde_json::from_value(map["osc_cues"].clone()).unwrap();
    let effects: Vec<LightingEffect> =
        serde_json::from_value(map["effects"].clone()).unwrap_or_default();

    let d = Timeline::elohim();
    let timeline = Timeline {
        start_ms: env_f("PLAY_START_MS", d.start_ms),
        pause1_s: env_f("PAUSE1_S", d.pause1_s),
        jump_to_s: env_f("JUMP_TO_S", d.jump_to_s),
        resume_ms: env_f("RESUME_MS", d.resume_ms),
        pause2_ms: env_f("PAUSE2_MS", d.pause2_ms),
        settle_s: env_f("SETTLE_S", d.settle_s),
        pass2_judge_s: env_f("PASS2_JUDGE_S", d.pass2_judge_s),
        relay_lag_ms: env_f("RELAY_LAG_MS", d.relay_lag_ms),
    };

    let mut relay = Vec::new();
    for e in ring["events"].as_array().unwrap() {
        let t = e["t"].as_f64().unwrap();
        for o in e["osc"].as_array().into_iter().flatten() {
            let Some(addr) = o["a"].as_str() else {
                continue;
            };
            let Some(v) = o["v"]
                .as_array()
                .and_then(|v| v.iter().map(arg).collect::<Option<Vec<_>>>())
            else {
                continue;
            };
            relay.push((t, FixtureCommand::new(addr, v)));
        }
    }

    let report = run(&rows, &effects, &relay, &timeline);
    if let Ok(path) = std::env::var("SENDS_OUT") {
        let lines: Vec<Value> = report
            .sent
            .iter()
            .map(|s| serde_json::json!({"t": s.wall_ms, "a": s.address, "v": floats(&s.args)}))
            .collect();
        std::fs::write(path, serde_json::to_vec(&lines).unwrap()).unwrap();
    }
    println!(
        "pass 1 (0-{:.1} s): {} fixture packets; pass 2 (from {:.1} s): {} fixture packets",
        timeline.pause1_s, report.pass_packets[0], timeline.jump_to_s, report.pass_packets[1]
    );
    println!("relay lag assumed: {} ms", timeline.relay_lag_ms);
    for flag in &report.flags {
        println!(
            "\nFIRST FLAG: {} at wall +{:.3} s ({}); event {} = [{}]",
            flag.fixture,
            flag.wall_s,
            if flag.second_pass { "pass 2" } else { "pass 1" },
            flag.command.a,
            flag.command
                .v
                .iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        println!(
            "  what the replay had sent on that address (echo window {} ms):",
            echo_window_ms()
        );
        if flag.sends.is_empty() {
            println!("    (nothing)");
        }
        for (ago, args) in &flag.sends {
            println!("    {ago:+.0} ms: [{}]", floats(args));
        }
    }
    println!(
        "\nrelay events judged: {}; fixtures the replay held: {}",
        report.relay_events,
        report.held.len()
    );
    let flagged = report.flagged();
    println!(
        "flagged (yielded) fixtures: {} {:?}",
        flagged.len(),
        flagged
    );
}
