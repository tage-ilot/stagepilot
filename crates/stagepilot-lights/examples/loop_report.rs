//! Report on how a song fits, rows against loops (and loop patches): the tool behind the
//! measurements of "loops with patches". Reads a song and prints what the fit makes of it and
//! how well the fitted song replays the original.
//!
//! ```text
//! cargo run --release -p stagepilot-lights --example loop_report -- --saved cue_map.json
//! cargo run --release -p stagepilot-lights --example loop_report -- --recording take.json
//! ```
//!
//! * `--saved FILE` is a saved song (a cue map: `osc_cues` and `effects`). Its replay is sampled
//!   every 25 ms and fitted again as if it had just been recorded; the sampled replay is what
//!   the fitted song is compared with.
//! * `--recording FILE` is a kept raw recording (`lightkey-recordings/`), fitted as the app does.
//!
//! Printed: items before (saved songs only) and after (rows, messages, loops, their members,
//! cycle steps, patch steps) and, per property class, how many 25 ms samples of the replay of the
//! fitted song are further than one tolerance (and than two) from the original. The file is
//! read only; nothing is written.

use serde_json::Value;
use stagepilot_core::config::{LightingEffect, OscLightingCue, SongLightingCueMap};
use stagepilot_lights::lightkey_curve::Timeline;
use stagepilot_lights::lightkey_fit::{fit_within_caps_budgeted, Caps, Size};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_recording;
use stagepilot_lights::lightkey_smooth::{
    clean_changes, prepare, property_class, smooth_rows, tolerance_for, Series,
};
use std::collections::BTreeMap;
use std::time::Instant;

const GRID: f64 = 0.025;

fn patch_steps(effect: &LightingEffect) -> usize {
    serde_json::to_value(effect)
        .ok()
        .and_then(|v| v.get("patches").cloned())
        .and_then(|p| match p {
            Value::Array(list) => Some(
                list.iter()
                    .map(|patch| {
                        patch
                            .get("steps")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len)
                    })
                    .sum(),
            ),
            _ => None,
        })
        .unwrap_or(0)
}

fn patch_count(effect: &LightingEffect) -> usize {
    serde_json::to_value(effect)
        .ok()
        .and_then(|v| v.get("patches").and_then(Value::as_array).map(Vec::len))
        .unwrap_or(0)
}

struct Counts {
    rows: usize,
    messages: usize,
    loops: usize,
    members: usize,
    cycle_steps: usize,
    patches: usize,
    patch_steps: usize,
}

fn counts(rows: &[OscLightingCue], effects: &[LightingEffect]) -> Counts {
    Counts {
        rows: rows.len(),
        messages: rows.iter().map(|r| r.outgoing().len()).sum(),
        loops: effects.len(),
        members: effects.iter().map(|e| e.members.len()).sum(),
        cycle_steps: effects.iter().map(|e| e.cycle.len()).sum(),
        patches: effects.iter().map(patch_count).sum(),
        patch_steps: effects.iter().map(patch_steps).sum(),
    }
}

fn print_counts(label: &str, c: &Counts) {
    println!(
        "{label:<8} rows {:>7}  messages {:>7}  loops {:>3} (members {:>4})  cycle steps {:>5}  patches {:>5} (steps {:>5})  items {:>7}",
        c.rows,
        c.messages,
        c.loops,
        c.members,
        c.cycle_steps,
        c.patches,
        c.patch_steps,
        c.messages + c.cycle_steps + c.patch_steps
    );
}

/// The replay of a saved song, sampled every 25 ms, as a recording: (baseline, changes, duration).
fn sample_song(map: &SongLightingCueMap) -> (Vec<FixtureCommand>, Vec<(f64, FixtureCommand)>, f64) {
    let timeline = Timeline::new(&map.osc_cues, &map.effects);
    let duration = map
        .osc_cues
        .iter()
        .map(|r| r.at_seconds + r.fade_seconds)
        .chain(map.effects.iter().map(|e| e.end_seconds))
        .fold(0.0, f64::max);
    let mut baseline = Vec::new();
    let mut changes = Vec::new();
    let steps = (duration / GRID).ceil() as usize;
    for address in timeline.addresses() {
        let mut last: Option<Vec<FixtureArg>> = None;
        for i in 0..=steps {
            let t = i as f64 * GRID;
            let Some(v) = timeline.playback_at(address, t) else {
                continue;
            };
            if last.as_ref() == Some(&v) {
                continue;
            }
            if last.is_none() {
                baseline.push(FixtureCommand::new(address, v.clone()));
            } else {
                changes.push((t, FixtureCommand::new(address, v.clone())));
            }
            last = Some(v);
        }
    }
    (baseline, changes, duration)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut saved, mut recording) = (None, None);
    let mut caps = Caps::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--saved" => saved = args.next(),
            "--recording" => recording = args.next(),
            "--old-caps" => caps = Caps::OLD,
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let (baseline, changes, duration) = match (saved, recording) {
        (Some(path), None) => {
            let text = std::fs::read_to_string(&path).expect("read the song");
            let map: SongLightingCueMap = serde_json::from_str(&text).expect("a cue map");
            print_counts("saved", &counts(&map.osc_cues, &map.effects));
            sample_song(&map)
        }
        (None, Some(path)) => {
            let rec = lightkey_recording::load(std::path::Path::new(&path)).expect("a recording");
            let duration = rec.duration_seconds;
            let (baseline, changes) = rec.into_parts();
            (baseline, changes, duration)
        }
        _ => {
            eprintln!("usage: loop_report --saved FILE | --recording FILE [--old-caps]");
            std::process::exit(2);
        }
    };
    println!(
        "input: {} baseline values, {} changes, {duration:.1} s",
        baseline.len(),
        changes.len()
    );
    let started = Instant::now();
    let fitted = fit_within_caps_budgeted(&baseline, &changes, duration, &caps, u128::MAX);
    let ms = started.elapsed().as_millis();
    let rows: Vec<OscLightingCue> = smooth_rows(&fitted.capture.rows);
    let effects = fitted.capture.effects.clone();
    print_counts("fitted", &counts(&rows, &effects));
    let size = Size::of_capture(&fitted.capture);
    println!(
        "stage {} scale x{} trimmed {} fit {ms} ms; size {} rows {} msgs {} loops {} steps",
        fitted.stage.as_str(),
        fitted.scale,
        fitted.trimmed,
        size.rows,
        size.messages,
        size.effects,
        size.cycle_steps
    );
    for e in &effects {
        println!(
            "  loop {:>8.3}-{:>8.3} period {:>7.3} members {:>3} steps {:>4} patches {:>4}/{:>5} steps",
            e.start_seconds,
            e.end_seconds,
            e.period_seconds,
            e.members.len(),
            e.cycle.len(),
            patch_count(e),
            patch_steps(e)
        );
    }

    // how well the fitted song replays the original: as the fit sees it (cleaned), and as it was
    let timeline = Timeline::new(&rows, &effects);
    println!("-- against the recording as the fit sees it (cleaned of flashes)");
    compare(
        &timeline,
        &baseline,
        &clean_changes(&changes, duration),
        duration,
    );
    println!("-- against the recording as it was");
    compare(&timeline, &baseline, &changes, duration);
}

fn compare(
    timeline: &Timeline,
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
) {
    let data = prepare(baseline, changes, duration);
    let mut classes: BTreeMap<&'static str, (usize, usize, usize, Vec<f32>)> = BTreeMap::new();
    for d in &data {
        let Some(series) = Series::from_data(d) else {
            continue;
        };
        let tol = tolerance_for(&d.address, 1.0);
        let class = property_class(&d.address, series.dim);
        let entry = classes.entry(class).or_default();
        let mut buf = vec![0.0; series.dim];
        let steps = (duration / GRID).floor() as usize;
        for i in 0..=steps {
            let t = i as f64 * GRID;
            let Some(v) = timeline.playback_at(&d.address, t) else {
                continue;
            };
            series.at(t, &mut buf);
            let diff = v
                .iter()
                .zip(&buf)
                .map(|(a, b)| match a {
                    FixtureArg::Float(a) => (a - b).abs(),
                    _ => 0.0,
                })
                .fold(0.0, f64::max);
            entry.0 += 1;
            if diff > tol {
                entry.1 += 1;
            }
            if diff > 2.0 * tol {
                entry.2 += 1;
            }
            entry.3.push((diff / tol) as f32);
        }
    }
    println!(
        "{:<18} {:>9} {:>9} {:>9} {:>9}",
        "class", "samples", "> 1 tol", "> 2 tol", "p99 tol"
    );
    let (mut all, mut over1, mut over2) = (0usize, 0usize, 0usize);
    for (class, (n, o1, o2, mut errs)) in classes {
        errs.sort_by(f32::total_cmp);
        let p99 = errs.get(errs.len().saturating_sub(1) * 99 / 100).copied();
        println!(
            "{class:<18} {n:>9} {:>8.2}% {:>8.2}% {:>9.2}",
            100.0 * o1 as f64 / n.max(1) as f64,
            100.0 * o2 as f64 / n.max(1) as f64,
            p99.unwrap_or(0.0)
        );
        all += n;
        over1 += o1;
        over2 += o2;
    }
    println!(
        "{:<18} {all:>9} {:>8.2}% {:>8.2}%",
        "ALL",
        100.0 * over1 as f64 / all.max(1) as f64,
        100.0 * over2 as f64 / all.max(1) as f64
    );
}
