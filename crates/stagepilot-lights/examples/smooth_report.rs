//! Report on the smooth capture of recorded takes.
//!
//! ```text
//! cargo run --release -p stagepilot-lights --example smooth_report -- takes.json [--no-effects]
//! ```
//!
//! `takes.json` is `{"takes": [{"song_title", "duration_seconds", "fixtures": {"initial": [{a, v}],
//! "events": [{"at_seconds", "osc": [{a, v}]}]}}]}` (the shape the Lightkey link stores a
//! take in). For each take the report prints:
//!
//! * a sanity count of plain ramp segments at 3 % / 1.5 degrees on the raw samples (the same
//!   counting convention as the analysis script: gaps and segments under 0.1 s are steps);
//! * item counts (rows, messages, ramps, steps, effects, cycle steps) and the JSON size of the
//!   old thin-and-merge rows and of the new rows + effects;
//! * the reconstruction error per property class, outside and inside effects;
//! * every effect: period, window, members and phase offsets;
//! * why series stay plain ramps.
//!
//! The take file is read only; nothing is written.

use serde_json::Value;
use stagepilot_core::config::{OscLightingCue, SongLightingCueMap};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand, FixtureState};
use stagepilot_lights::lightkey_looks::{look_rows, LookRecorder};
use stagepilot_lights::lightkey_smooth::{
    polyline, reconstruct_report, smooth_rows, ClassError, SmoothParams, GAP_SECONDS,
    LEVEL_TOLERANCE, MAX_RAMP_SECONDS, STEP_SECONDS,
};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

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

fn command(value: &Value) -> Option<FixtureCommand> {
    let address = value.get("a")?.as_str()?.to_string();
    let args = value
        .get("v")?
        .as_array()?
        .iter()
        .map(arg)
        .collect::<Option<Vec<_>>>()?;
    Some(FixtureCommand::new(address, args))
}

/// The analysis script's count: samples of every numeric address, gaps over 0.5 s are steps,
/// segments under 0.1 s are steps, the rest ramps.
fn sanity(events: &[Value], level: f64, angle: f64) -> (usize, usize, usize, f64) {
    let mut series: HashMap<String, Vec<(f64, Vec<f64>)>> = HashMap::new();
    for event in events {
        let at = event["at_seconds"].as_f64().unwrap_or(0.0);
        for m in event["osc"].as_array().into_iter().flatten() {
            let Some(c) = command(m) else { continue };
            let numbers: Option<Vec<f64>> =
                c.v.iter()
                    .map(|a| match a {
                        FixtureArg::Float(f) => Some(*f),
                        FixtureArg::Int(i) => Some(*i as f64),
                        _ => None,
                    })
                    .collect();
            if let Some(n) = numbers {
                series.entry(c.a).or_default().push((at, n));
            }
        }
    }
    let (mut ramps, mut steps, mut longest) = (0usize, 0usize, 0.0f64);
    for (address, s) in &series {
        let tol = if address.to_lowercase().contains("angle") {
            angle
        } else {
            level
        };
        let mut chunks: Vec<Vec<&(f64, Vec<f64>)>> = vec![vec![&s[0]]];
        for pair in s.windows(2) {
            if pair[1].0 - pair[0].0 > GAP_SECONDS {
                steps += 1;
                chunks.push(vec![&pair[1]]);
            } else {
                chunks.last_mut().expect("chunk").push(&pair[1]);
            }
        }
        for chunk in chunks.iter().filter(|c| c.len() >= 2) {
            let dim = chunk[0].1.len();
            let ts: Vec<f64> = chunk.iter().map(|p| p.0).collect();
            let vs: Vec<f64> = chunk.iter().flat_map(|p| p.1.iter().copied()).collect();
            for (i, j) in polyline(&ts, &vs, dim, tol, MAX_RAMP_SECONDS) {
                let dt = ts[j] - ts[i];
                if dt < STEP_SECONDS {
                    steps += 1;
                } else {
                    ramps += 1;
                    longest = longest.max(dt);
                }
            }
        }
    }
    (ramps + steps, ramps, steps, longest)
}

fn table(title: &str, rows: &[ClassError]) {
    println!("  {title}");
    if rows.is_empty() {
        println!("    (none)");
        return;
    }
    println!(
        "    {:<12} {:>9} {:>10} {:>10} {:>10} {:>12} {:>12}",
        "class", "points", "tolerance", "max", "p99", "> tol", "> 2x tol"
    );
    for r in rows {
        println!(
            "    {:<12} {:>9} {:>10.3} {:>10.3} {:>10.3} {:>11.2}% {:>11.2}%",
            r.class,
            r.points,
            r.tolerance,
            r.max,
            r.p99,
            r.over_tolerance * 100.0,
            r.over_double * 100.0
        );
    }
}

fn json_size(rows: &[OscLightingCue], effects: usize, map: &SongLightingCueMap) -> usize {
    let _ = (rows, effects);
    serde_json::to_vec(map).map_or(0, |v| v.len())
}

fn map_of(
    rows: Vec<OscLightingCue>,
    effects: Vec<stagepilot_core::config::LightingEffect>,
) -> SongLightingCueMap {
    SongLightingCueMap {
        song_key: "report".into(),
        song_title: "report".into(),
        cues: vec![],
        osc_cues: rows,
        loudness: None,
        captured_edited: false,
        capture: None,
        effects,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: smooth_report <takes.json> [--no-effects]");
        std::process::exit(2);
    };
    let detect = !args.any(|a| a == "--no-effects");
    let text = std::fs::read_to_string(&path).expect("read the takes file");
    let doc: Value = serde_json::from_str(&text).expect("takes file is JSON");
    for take in doc["takes"].as_array().expect("takes") {
        let title = take["song_title"].as_str().unwrap_or("?");
        let duration = take["duration_seconds"].as_f64().unwrap_or(0.0);
        let events = take["fixtures"]["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        println!("\n==================== {title} ({duration:.0} s)");
        let mut baseline = FixtureState::new();
        for c in take["fixtures"]["initial"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(command)
        {
            baseline.apply(&c);
        }
        let mut recorder = LookRecorder::new(&baseline);
        for event in &events {
            let at = event["at_seconds"].as_f64().unwrap_or(0.0);
            let commands: Vec<FixtureCommand> = event["osc"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(command)
                .collect();
            recorder.observe(at, &commands);
        }
        println!("raw changes recorded: {}", recorder.len());

        let (all, ramps, steps, longest) = sanity(&events, LEVEL_TOLERANCE, 1.5);
        println!(
            "sanity (analysis script, plain ramps at 3 % / 1.5 deg, cap 60 s): {all} segments ({ramps} ramps, {steps} steps), longest ramp {longest:.1} s"
        );

        // before: the old thin-and-merge rows at the default Detail (0.5 s)
        let started = Instant::now();
        let old = look_rows(&recorder.looks(duration, 0.5));
        let old_messages: usize = old.iter().map(|r| r.messages.len()).sum();
        let old_bytes = json_size(&old, 0, &map_of(old.clone(), vec![]));
        println!(
            "before (thin + merge at 0.5 s): {} rows, {old_messages} messages, {old_bytes} bytes JSON ({:.1} s)",
            old.len(),
            started.elapsed().as_secs_f64()
        );

        let started = Instant::now();
        let params = SmoothParams {
            detect_effects: detect,
            ..SmoothParams::default()
        };
        let fit = recorder.smooth(duration, &params);
        let took = started.elapsed().as_secs_f64();
        let rows = smooth_rows(&fit.rows);
        let cycle_steps: usize = fit.effects.iter().map(|e| e.cycle.len()).sum();
        let bytes = json_size(
            &rows,
            fit.effects.len(),
            &map_of(rows.clone(), fit.effects.clone()),
        );
        let messages: usize = rows.iter().map(|r| r.messages.len()).sum();
        println!(
            "after  (ramps + effects{}): {} rows ({} ramp messages, {} step messages, {messages} messages), {} effects ({cycle_steps} cycle steps) => {} items, {bytes} bytes JSON (fit {took:.1} s)",
            if detect { "" } else { ", effects off" },
            rows.len(),
            fit.ramps,
            fit.steps,
            fit.effects.len(),
            rows.len() + fit.effects.len()
        );
        println!(
            "  raw samples {} -> {} messages + {} cycle steps",
            fit.raw_samples, messages, cycle_steps
        );

        println!("\nreconstruction (rebuilt at 25 ms from rows + effects vs the recorded samples)");
        let (base, changes) = recorder.raw();
        let report = reconstruct_report(&base, &changes, duration, &rows, &fit.effects, 1.0);
        table(
            "outside effects (acceptance: max <= tolerance)",
            &report.outside_effects,
        );
        table(
            "inside effects (acceptance: >= 95 % within 2x tolerance)",
            &report.inside_effects,
        );
        table("overall (acceptance: p99 <= 2x tolerance)", &report.overall);
        println!(
            "  options/text: {:.3}% of {} points differ",
            report.option_mismatch * 100.0,
            report.option_points
        );

        println!("\neffects ({}):", fit.effects.len());
        for (e, info) in fit.effects.iter().zip(&fit.effect_infos) {
            let members: Vec<String> = e
                .members
                .iter()
                .map(|m| {
                    format!(
                        "{}{}",
                        m.fixture,
                        if m.offset_seconds == 0.0 {
                            String::new()
                        } else {
                            format!("@{:.3}", m.offset_seconds)
                        }
                    )
                })
                .collect();
            println!(
                "  period {:.3} s, {:.1}-{:.1} s ({:.1} cycles), props {:?}, {} cycle steps (instead of {} plain segments), worst member {:.1}% within 2x tol, {} members: {}",
                e.period_seconds,
                e.start_seconds,
                e.end_seconds,
                (e.end_seconds - e.start_seconds) / e.period_seconds,
                info.properties,
                info.cycle_steps,
                info.replaces_segments,
                info.worst_share * 100.0,
                e.members.len(),
                members.join(" ")
            );
        }

        let mut by_time: BTreeMap<i64, (usize, usize)> = BTreeMap::new();
        for r in &rows {
            let e = by_time
                .entry((r.at_seconds / 20.0).floor() as i64)
                .or_default();
            e.0 += 1;
            e.1 += r.messages.len();
        }
        println!("\nrows per 20 s of the song (rows / messages):");
        println!(
            "  {}",
            by_time
                .iter()
                .map(|(k, (r, m))| format!("{}s:{}/{}", k * 20, r, m))
                .collect::<Vec<_>>()
                .join("  ")
        );

        println!("\nseries that stay plain ramps (and the rows they cost):");
        let mut by_reason: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
        for note in &fit.notes {
            let key = note
                .reason
                .split(" (best")
                .next()
                .unwrap_or("")
                .chars()
                .take(60)
                .collect::<String>();
            let key = if note.reason.starts_with("inside effect") {
                "inside an effect; the rest is plain ramps".to_string()
            } else if note.reason.starts_with("no loop") {
                "no loop of 3+ cycles found".to_string()
            } else if note.reason.starts_with("loop of") {
                "loop found but rejected (see below)".to_string()
            } else {
                key
            };
            let e = by_reason.entry(key).or_default();
            e.0 += 1;
            e.1 += note.samples;
            e.2 += note.items;
        }
        println!(
            "  {:<62} {:>9} {:>10} {:>10}",
            "reason", "addresses", "samples", "items"
        );
        for (reason, (n, samples, items)) in &by_reason {
            println!("  {reason:<62} {n:>9} {samples:>10} {items:>10}");
        }
        let mut worst: Vec<_> = fit.notes.iter().collect();
        worst.sort_by_key(|n| std::cmp::Reverse(n.items));
        println!("  most expensive series:");
        for n in worst.iter().take(12) {
            println!(
                "    {:<44} {:>6} samples -> {:>5} items: {}",
                n.address, n.samples, n.items, n.reason
            );
        }
    }
}
