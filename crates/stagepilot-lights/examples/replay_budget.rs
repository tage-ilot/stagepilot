//! Packet-budget report of the smooth replay on a simulated clock (no sockets, no sleeping).
//!
//! ```text
//! cargo run --release -p stagepilot-lights --example replay_budget -- takes.json [song]
//! ```
//!
//! `takes.json` has the shape `smooth_report` reads. For each take (or only the ones whose
//! title contains `song`, case-insensitive) it fits ramps and effects exactly as a recording
//! would save them, replays the result with [`simulate`] (the plugin's engine and its 5 ms
//! pacing) and prints:
//!
//! * packets per second over the song: mean, 99th percentile, max, seconds above 150 per
//!   second, and how the packets split into steps, ramps and effect values;
//! * the share of effect values sent while the effect rate was lowered (below 20 Hz);
//! * how long a step waits behind the pacing queue;
//! * the largest difference between what the lights would show (the sent values, each active
//!   from `at + lead`) and the raw recorded motion, per property class, inside and outside
//!   effects, with the share beyond the tolerance (no operator take-over is simulated);
//! * the lateness of each step value against the time its row's step is meant to leave (row
//!   time minus the look lead; negative = early): median / p99 / max, the share later than 25 ms
//!   and the share earlier than 100 ms, the earliest value and the worst rows;
//! * the ramp values that had to wait for budget and the seconds in which one did;
//! * the CPU time of the simulation.
//!
//! `BURST_LEAD_MAX_MS=<ms>` overrides `STEP_LEAD_MAX_MS` (0 = step bursts exactly on the row).
//!
//! The take file is read only; nothing is written.

use serde_json::Value;
use stagepilot_core::config::OscArgument;
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand, FixtureState};
use stagepilot_lights::lightkey_looks::LookRecorder;
use stagepilot_lights::lightkey_replay::{
    simulate, Kind, ReplayEngine, SentPacket, FAST_TICK_SECONDS, PACKET_GAP_SECONDS,
};
use stagepilot_lights::lightkey_smooth::{
    prepare, property_class, smooth_rows, tolerance_for, Series, SmoothParams, CHECK_STEP_SECONDS,
};
use stagepilot_lights::plugin::LOOK_LEAD_SECONDS;
use std::collections::BTreeMap;
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

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i]
}

fn osc_floats(args: &[OscArgument]) -> Option<Vec<f64>> {
    args.iter()
        .map(|a| match a {
            OscArgument::Float(f) => Some(*f),
            _ => None,
        })
        .collect()
}

#[derive(Default)]
struct Deviation {
    points: usize,
    over_tolerance: usize,
    over_double: usize,
    max: f64,
    tolerance: f64,
    at_max: (String, f64),
}

/// Compare what the sent values would show (each packet active from `visible(p)`, held until the
/// next one) with the raw recorded motion on the 25 ms grid and print the table.
fn deviation(
    title: &str,
    packets: &[SentPacket],
    data: &[stagepilot_lights::lightkey_smooth::AddrData],
    timeline: &stagepilot_lights::lightkey_curve::Timeline,
    visible: &dyn Fn(&SentPacket) -> f64,
    show_worst: bool,
) {
    const SLACK: f64 = 1e-6;
    let mut by_address: BTreeMap<&str, Vec<(f64, &SentPacket)>> = BTreeMap::new();
    for p in packets {
        by_address
            .entry(&p.message.address)
            .or_default()
            .push((visible(p), p));
    }
    for list in by_address.values_mut() {
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    let mut inside: BTreeMap<&'static str, Deviation> = BTreeMap::new();
    let mut outside: BTreeMap<&'static str, Deviation> = BTreeMap::new();
    let (mut option_points, mut option_bad) = (0usize, 0usize);
    // worst outside-effect points: (error / tolerance, address, t, sent, raw, active from)
    let mut worst: Vec<(f64, String, f64, f64, f64, f64)> = Vec::new();
    for d in data {
        let Some(sent) = by_address.get(d.address.as_str()) else {
            continue;
        };
        let Some(series) = Series::from_data(d) else {
            // options and text: the held value must be the recorded one
            let mut k = 0usize;
            let end = d.items.last().map_or(0.0, |i| i.0);
            let mut t = d.items.first().map_or(0.0, |i| i.0);
            let mut j = 0usize;
            while t <= end + 1e-9 {
                while k + 1 < d.items.len() && d.items[k + 1].0 <= t + 1e-9 {
                    k += 1;
                }
                while j + 1 < sent.len() && sent[j + 1].0 <= t + SLACK {
                    j += 1;
                }
                if sent[j].0 <= t + SLACK {
                    option_points += 1;
                    let have: Vec<FixtureArg> = sent[j]
                        .1
                        .message
                        .args
                        .iter()
                        .map(|a| match a {
                            OscArgument::Float(f) => FixtureArg::Float(*f),
                            OscArgument::Int(i) => FixtureArg::Int(*i),
                            OscArgument::String(s) => FixtureArg::Text(s.clone()),
                            OscArgument::Bool(b) => FixtureArg::Bool(*b),
                        })
                        .collect();
                    if have != d.items[k].1 {
                        option_bad += 1;
                    }
                }
                t += CHECK_STEP_SECONDS;
            }
            continue;
        };
        let class = property_class(&d.address, series.dim);
        let tolerance = tolerance_for(&d.address, 1.0);
        let mut buf = vec![0.0; series.dim];
        let (first, last) = (series.ts[0], *series.ts.last().expect("samples"));
        let mut t = first;
        let mut j = 0usize;
        while t <= last + 1e-9 {
            while j + 1 < sent.len() && sent[j + 1].0 <= t + SLACK {
                j += 1;
            }
            if sent[j].0 <= t + SLACK {
                series.at(t, &mut buf);
                if let Some(have) = osc_floats(&sent[j].1.message.args) {
                    let err = have
                        .iter()
                        .zip(&buf)
                        .map(|(h, w)| (h - w).abs())
                        .fold(0.0, f64::max);
                    let in_effect = timeline.in_effect(&d.address, t);
                    if !in_effect {
                        worst.push((
                            err / tolerance,
                            d.address.clone(),
                            t,
                            have[0],
                            buf[0],
                            sent[j].0,
                        ));
                        if worst.len() > 400 {
                            worst.sort_by(|a, b| b.0.total_cmp(&a.0));
                            worst.truncate(5);
                        }
                    }
                    let e = if in_effect { &mut inside } else { &mut outside }
                        .entry(class)
                        .or_default();
                    e.tolerance = tolerance;
                    e.points += 1;
                    if err > e.max {
                        e.max = err;
                        e.at_max = (d.address.clone(), t);
                    }
                    e.over_tolerance += usize::from(err > tolerance);
                    e.over_double += usize::from(err > 2.0 * tolerance);
                }
            }
            t += CHECK_STEP_SECONDS;
        }
    }
    println!("\nsent values vs the raw recorded motion, {title} (no operator take-over simulated)");
    for (name, table) in [("outside effects", &outside), ("inside effects", &inside)] {
        println!("  {name}");
        if table.is_empty() {
            println!("    (none)");
        }
        for (class, e) in table {
            println!(
                "    {class:<12} {:>9} points  tolerance {:>6.3}  max {:>7.3}  beyond tol {:>6.2} %  beyond 2x tol {:>6.2} %  (max at {} {:.2} s)",
                e.points,
                e.tolerance,
                e.max,
                e.over_tolerance as f64 * 100.0 / e.points.max(1) as f64,
                e.over_double as f64 * 100.0 / e.points.max(1) as f64,
                e.at_max.0,
                e.at_max.1
            );
        }
    }
    if show_worst {
        worst.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("  worst points outside effects (error in tolerances, address, song time, sent, raw, active from):");
        for (e, a, t, sent, raw, from) in worst.iter().take(5) {
            println!("    {e:>6.1}x {a} at {t:.3} s: sent {sent:.3}, raw {raw:.3}, active from {from:.3}");
        }
    }
    println!(
        "  options/text: {:.3} % of {option_points} points differ",
        option_bad as f64 * 100.0 / option_points.max(1) as f64
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: replay_budget <takes.json> [song]");
        std::process::exit(2);
    };
    let wanted = args.next().map(|s| s.to_lowercase());
    let text = std::fs::read_to_string(&path).expect("read the takes file");
    let doc: Value = serde_json::from_str(&text).expect("takes file is JSON");
    let mut shown = 0;
    for take in doc["takes"].as_array().expect("takes") {
        let title = take["song_title"].as_str().unwrap_or("?");
        if wanted
            .as_ref()
            .is_some_and(|w| !title.to_lowercase().contains(w))
        {
            continue;
        }
        shown += 1;
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
        let fit = recorder.smooth(duration, &SmoothParams::default());
        let rows = smooth_rows(&fit.rows);
        let effects = fit.effects.clone();
        println!(
            "saved map: {} rows + {} effects = {} items (raw samples {})",
            rows.len(),
            effects.len(),
            rows.len() + effects.len(),
            fit.raw_samples
        );

        // ---- the simulated replay
        let mut engine = ReplayEngine::new(&rows, &effects);
        if let Some(ms) = std::env::var("BURST_LEAD_MAX_MS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
        {
            engine = engine.with_step_lead_max_ms(ms);
        }
        let started = Instant::now();
        let packets = simulate(&mut engine, 0.0, duration, LOOK_LEAD_SECONDS, &|_, _| false);
        let cpu = started.elapsed();
        println!(
            "\nsimulated replay 0 - {duration:.0} s: {} packets, CPU time {:.0} ms ({:.2} ms per song second, single thread, no I/O)",
            packets.len(),
            cpu.as_secs_f64() * 1000.0,
            cpu.as_secs_f64() * 1000.0 / duration.max(1.0)
        );

        if let Ok(spec) = std::env::var("BUDGET_TRACE") {
            // BUDGET_TRACE=<address>,<from>,<to>
            let parts: Vec<&str> = spec.split(',').collect();
            if let [address, from, to] = parts[..] {
                let (from, to): (f64, f64) = (from.parse().unwrap(), to.parse().unwrap());
                for p in packets
                    .iter()
                    .filter(|p| p.message.address == address && p.at >= from && p.at <= to)
                {
                    println!(
                        "  trace {:.3} (pos {:.3}) {:?} {:?}",
                        p.at, p.position, p.kind, p.message.args
                    );
                }
                for k in 0..=((to - from) / 0.1) as usize {
                    let t = from + k as f64 * 0.1;
                    println!(
                        "  timeline {t:.2}: {:?} moving {}",
                        engine.timeline().value_at(address, t),
                        engine.timeline().is_moving(address, t)
                    );
                }
            }
        }

        // ---- the seams between effects and the rows around them
        {
            let (base, changes) = recorder.raw();
            let data = prepare(&base, &changes, duration);
            let raw_series: BTreeMap<&str, Series> = data
                .iter()
                .filter_map(|d| Series::from_data(d).map(|s| (d.address.as_str(), s)))
                .collect();
            let mut jumps: Vec<(f64, f64, String, &str)> = Vec::new();
            let mut introduced = 0usize;
            for e in &effects {
                for m in &e.members {
                    for property in e
                        .cycle
                        .iter()
                        .map(|c| c.property.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                    {
                        let address = format!("/fixture/{}/overrides/{property}", m.fixture);
                        let tolerance = tolerance_for(&address, 1.0);
                        for (seam, side) in [(e.start_seconds, "start"), (e.end_seconds, "end")] {
                            let (Some(before), Some(after)) = (
                                engine.timeline().playback_at(&address, seam - 1e-6),
                                engine.timeline().playback_at(&address, seam + 1e-6),
                            ) else {
                                continue;
                            };
                            let jump = before
                                .iter()
                                .zip(&after)
                                .map(|(a, b)| match (a, b) {
                                    (FixtureArg::Float(a), FixtureArg::Float(b)) => (a - b).abs(),
                                    _ => 0.0,
                                })
                                .fold(0.0, f64::max);
                            // what the recorded motion itself does across the same 50 ms
                            let raw_jump = raw_series.get(address.as_str()).map_or(0.0, |series| {
                                let (mut a, mut b) = (vec![0.0; series.dim], vec![0.0; series.dim]);
                                series.at(seam - 0.025, &mut a);
                                series.at(seam + 0.025, &mut b);
                                a.iter()
                                    .zip(&b)
                                    .map(|(a, b)| (a - b).abs())
                                    .fold(0.0, f64::max)
                            });
                            if jump - raw_jump > tolerance {
                                introduced += 1;
                            }
                            jumps.push((jump / tolerance, seam, address.clone(), side));
                        }
                    }
                }
            }
            let over = jumps.iter().filter(|j| j.0 > 1.0).count();
            let over_double = jumps.iter().filter(|j| j.0 > 2.0).count();
            jumps.sort_by(|a, b| b.0.total_cmp(&a.0));
            println!(
                "\neffect seams (value of the rows just before/after the effect against the effect, per member and property): {} seams, {over} beyond the tolerance, {over_double} beyond 2x, {introduced} jump more than the recorded motion does there by over a tolerance; largest: {}",
                jumps.len(),
                jumps
                    .iter()
                    .take(3)
                    .map(|(j, t, a, side)| format!("{j:.1}x tol at {t:.2} s ({side}) {a}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }

        // ---- packets per second
        let seconds = duration.ceil() as usize + 1;
        let mut per_second = vec![0usize; seconds];
        for p in &packets {
            let i = (p.at.floor().max(0.0) as usize).min(seconds - 1);
            per_second[i] += 1;
        }
        let counts: Vec<f64> = per_second.iter().map(|c| *c as f64).collect();
        let mut sorted = counts.clone();
        sorted.sort_by(f64::total_cmp);
        let mean = counts.iter().sum::<f64>() / counts.len() as f64;
        let above = |limit: usize| per_second.iter().filter(|c| **c > limit).count();
        println!(
            "packets/s over {} s: mean {:.1}, p99 {:.0}, max {:.0}; seconds above 150/s: {}, above 100/s: {}, above 20/s: {}, idle seconds (0 packets): {}",
            seconds,
            mean,
            percentile(&sorted, 0.99),
            sorted.last().copied().unwrap_or(0.0),
            above(150),
            above(100),
            above(20),
            per_second.iter().filter(|c| **c == 0).count()
        );
        let by_kind = |k: Kind| packets.iter().filter(|p| p.kind == k).count();
        let effect_total = by_kind(Kind::Effect);
        let reduced = packets.iter().filter(|p| p.reduced).count();
        println!(
            "by kind: {} steps, {} ramp values, {} effect values",
            by_kind(Kind::Step),
            by_kind(Kind::Ramp),
            effect_total
        );
        println!(
            "share of values sent at the reduced rate: {:.1} % of effect values ({reduced} of {effect_total}), {:.1} % of all packets (rate lowered below {:.0} Hz; floor {:.0} Hz; final effect interval {:.3} s)",
            if effect_total > 0 { reduced as f64 * 100.0 / effect_total as f64 } else { 0.0 },
            reduced as f64 * 100.0 / packets.len().max(1) as f64,
            1.0 / FAST_TICK_SECONDS,
            8.0,
            engine.effect_interval()
        );
        let mut waits: Vec<f64> = packets
            .iter()
            .filter(|p| p.kind == Kind::Step)
            .map(|p| (p.at - (p.position - LOOK_LEAD_SECONDS)) * 1000.0)
            .collect();
        waits.sort_by(f64::total_cmp);
        println!(
            "steps behind the pacing queue (ms between the tick and the packet): median {:.0}, p99 {:.0}, max {:.0}",
            percentile(&waits, 0.5),
            percentile(&waits, 0.99),
            waits.last().copied().unwrap_or(0.0)
        );
        // Lateness against the row: the tick that sends a single step runs when the song
        // position (time + look lead) reaches the row, so a packet is on time when it leaves
        // at `position - lead`.
        {
            let mut late: Vec<(f64, &SentPacket)> = packets
                .iter()
                .filter(|p| p.kind == Kind::Step)
                .map(|p| ((p.at - (p.position - LOOK_LEAD_SECONDS)) * 1000.0, p))
                .collect();
            late.sort_by(|a, b| a.0.total_cmp(&b.0));
            let summary = |label: &str, ms: &[f64]| {
                let n = ms.len().max(1) as f64;
                println!(
                    "{label}: median {:.0}, p99 {:.0}, max {:.0}, earliest {:.0}; later than 25 ms: {:.1} % ({} of {}), later than 60 ms: {}, earlier than 100 ms: {:.1} % ({})",
                    percentile(ms, 0.5),
                    percentile(ms, 0.99),
                    ms.last().copied().unwrap_or(0.0),
                    ms.first().copied().unwrap_or(0.0),
                    ms.iter().filter(|m| **m > 25.0).count() as f64 * 100.0 / n,
                    ms.iter().filter(|m| **m > 25.0).count(),
                    ms.len(),
                    ms.iter().filter(|m| **m > 60.0).count(),
                    ms.iter().filter(|m| **m < -100.0).count() as f64 * 100.0 / n,
                    ms.iter().filter(|m| **m < -100.0).count()
                );
            };
            // The first look of the song (the tick at position 0) cannot start early: nothing
            // exists before position 0. It is reported on its own.
            let is_start = |p: &SentPacket| p.position <= LOOK_LEAD_SECONDS + 1e-3;
            let ms: Vec<f64> = late.iter().map(|l| l.0).collect();
            let rest: Vec<f64> = late
                .iter()
                .filter(|l| !is_start(l.1))
                .map(|l| l.0)
                .collect();
            let start_look: Vec<f64> = late.iter().filter(|l| is_start(l.1)).map(|l| l.0).collect();
            summary(
                "lateness of step values against their row (ms, negative = early), all",
                &ms,
            );
            summary("  without the first look of the song", &rest);
            println!(
                "  first look of the song (position 0, cannot start early): {} values, latest {:.0} ms",
                start_look.len(),
                start_look.last().copied().unwrap_or(0.0)
            );
            // the rows (song positions) with the latest values
            let mut per_row: BTreeMap<u64, (f64, f64, usize, String)> = BTreeMap::new();
            for (l, p) in &late {
                let e = per_row
                    .entry((p.position * 1000.0).round() as u64)
                    .or_insert((f64::INFINITY, f64::NEG_INFINITY, 0, String::new()));
                e.0 = e.0.min(*l);
                if *l > e.1 {
                    e.1 = *l;
                    e.3 = p.message.address.clone();
                }
                e.2 += 1;
            }
            let mut worst: Vec<_> = per_row.iter().collect();
            worst.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
            println!(
                "  worst rows (row time, values, earliest .. latest lateness ms, latest value):"
            );
            for (t, (lo, hi, n, a)) in worst.iter().take(4) {
                println!(
                    "    {:.3} s: {n} values, {lo:.0} .. {hi:.0} ms ({a})",
                    **t as f64 / 1000.0
                );
            }
            println!("  rows with 10 or more values (row time, values, earliest .. latest ms):");
            for (t, (lo, hi, n, _)) in per_row.iter().filter(|(_, v)| v.2 >= 10) {
                println!(
                    "    {:.3} s: {n} values, {lo:.0} .. {hi:.0} ms",
                    *t as f64 / 1000.0
                );
            }
        }
        let stats = engine.stats();
        println!(
            "ramp values that waited for budget: {} (each up to 50 ms of a ramp not updated); seconds with a ramp value waiting: {:.2}; step bursts started early: {}",
            stats.ramp_deferred, stats.ramp_skipped_seconds, stats.bursts_early
        );
        // Steps that share one tick queue behind each other: that part of the wait is the
        // 5 ms spacing itself (n steps take (n - 1) x 5 ms) and no queue order removes it.
        let mut bursts: Vec<f64> = {
            let mut by_tick: std::collections::BTreeMap<u64, usize> = Default::default();
            for p in packets.iter().filter(|p| p.kind == Kind::Step) {
                *by_tick.entry((p.position * 1e6) as u64).or_default() += 1;
            }
            by_tick.values().map(|n| *n as f64).collect()
        };
        bursts.sort_by(f64::total_cmp);
        println!(
            "steps sharing one tick (burst size): median {:.0}, p99 {:.0}, max {:.0}; floor of the wait from the 5 ms spacing alone for the largest burst: {:.0} ms",
            percentile(&bursts, 0.5),
            percentile(&bursts, 0.99),
            bursts.last().copied().unwrap_or(0.0),
            (bursts.last().copied().unwrap_or(1.0) - 1.0) * PACKET_GAP_SECONDS * 1000.0
        );
        // What the queue itself adds: the wait minus the 5 ms each earlier step of the same tick
        // takes. Zero means no ramp, effect or older step was ahead.
        let mut excess: Vec<f64> = {
            let mut seen: std::collections::BTreeMap<u64, usize> = Default::default();
            let mut steps: Vec<&SentPacket> =
                packets.iter().filter(|p| p.kind == Kind::Step).collect();
            steps.sort_by(|a, b| a.at.total_cmp(&b.at));
            steps
                .iter()
                .map(|p| {
                    let n = seen.entry((p.position * 1e6) as u64).or_default();
                    let ahead = *n as f64;
                    *n += 1;
                    ((p.at - (p.position - LOOK_LEAD_SECONDS)) - ahead * PACKET_GAP_SECONDS)
                        * 1000.0
                })
                .collect()
        };
        excess.sort_by(f64::total_cmp);
        println!(
            "steps, wait added by the queue beyond the spacing of steps of the same tick (ms): median {:.0}, p99 {:.0}, max {:.0}",
            percentile(&excess, 0.5),
            percentile(&excess, 0.99),
            excess.last().copied().unwrap_or(0.0)
        );
        let mut busiest: Vec<(usize, usize)> = per_second.iter().copied().enumerate().collect();
        busiest.sort_by_key(|b| std::cmp::Reverse(b.1));
        println!(
            "busiest seconds: {}",
            busiest
                .iter()
                .take(6)
                .map(|(s, c)| format!("{s}s:{c}"))
                .collect::<Vec<_>>()
                .join("  ")
        );

        // ---- deviation from the raw motion
        let (base, changes) = recorder.raw();
        let data = prepare(&base, &changes, duration);
        deviation(
            "as intended: each value active at the song position it was evaluated for",
            &packets,
            &data,
            engine.timeline(),
            &|p| p.position,
            true,
        );
        deviation(
            "as paced: each value active 45 ms after the tick that chose it plus its wait in the 5 ms pacing queue",
            &packets,
            &data,
            engine.timeline(),
            &|p| p.at + LOOK_LEAD_SECONDS,
            false,
        );
    }
    if shown == 0 {
        eprintln!("no take matched");
        std::process::exit(1);
    }
}
