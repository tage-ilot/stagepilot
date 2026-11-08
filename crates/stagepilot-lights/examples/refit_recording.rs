//! Fit a kept raw recording again, offline, and print what each stage of the fallback ladder
//! does. For a song that was saved badly or not at all: the raw recording is in
//! `lightkey-recordings/` next to `settings.json` (newest five kept).
//!
//! ```text
//! cargo run --release -p stagepilot-lights --example refit_recording -- FILE [--old-caps]
//!     [--rows N] [--messages N] [--effects N] [--steps N] [--no-budget]
//! ```
//!
//! `FILE` is `<song key>-<UTC time>.json`. The caps are the timeline's own unless given
//! (`--old-caps` is the 1,000 rows / 15,000 messages of builds before the ladder). Every full fit
//! prints one line (stage, tolerance scale, thinning window, rows, messages, loops, loop steps,
//! milliseconds); then the stage the song ended at, how many items the trim dropped, the fit time
//! and the saved size. `--no-budget` runs the whole ladder however long it takes.
//!
//! The file is read only; nothing is written.

use stagepilot_lights::lightkey_fit::{fit_within_caps_budgeted, Caps, Size, FIT_BUDGET_MS};
use stagepilot_lights::lightkey_recording;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut caps = Caps::default();
    let mut budget = FIT_BUDGET_MS;
    while let Some(arg) = args.next() {
        let mut number = |name: &str| -> usize {
            args.next()
                .and_then(|v| v.replace('_', "").parse().ok())
                .unwrap_or_else(|| {
                    eprintln!("{name} needs a number");
                    std::process::exit(2)
                })
        };
        match arg.as_str() {
            "--old-caps" => caps = Caps::OLD,
            "--rows" => caps.rows = number("--rows"),
            "--messages" => caps.messages = number("--messages"),
            "--effects" => caps.effects = number("--effects"),
            "--steps" => caps.cycle_steps = number("--steps"),
            "--no-budget" => budget = u128::MAX,
            other if !other.starts_with("--") && path.is_none() => path = Some(other.to_string()),
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(path) = path else {
        eprintln!("usage: refit_recording FILE [--old-caps] [--rows N] [--messages N] [--effects N] [--steps N] [--no-budget]");
        std::process::exit(2);
    };
    let started = Instant::now();
    let recording = match lightkey_recording::load(std::path::Path::new(&path)) {
        Ok(recording) => recording,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let (song, duration) = (recording.song_key.clone(), recording.duration_seconds);
    let (baseline, changes) = recording.into_parts();
    println!(
        "{song}: {} addresses' worth of baseline, {} changes, {duration:.1} s (read in {:?})",
        baseline.len(),
        changes.len(),
        started.elapsed()
    );
    println!(
        "caps: {} rows, {} messages, {} effects, {} loop steps",
        caps.rows, caps.messages, caps.effects, caps.cycle_steps
    );
    let fitted = fit_within_caps_budgeted(&baseline, &changes, duration, &caps, budget);
    println!(
        "{:<9} {:>6} {:>6} {:>9} {:>10} {:>6} {:>7} {:>8}",
        "stage", "scale", "thin s", "rows", "messages", "loops", "steps", "ms"
    );
    for a in &fitted.attempts {
        println!(
            "{:<9} {:>6} {:>6} {:>9} {:>10} {:>6} {:>7} {:>8}",
            a.stage.as_str(),
            a.scale,
            a.thin_seconds,
            a.size.rows,
            a.size.messages,
            a.size.effects,
            a.size.cycle_steps,
            a.ms
        );
    }
    let size = Size::of_capture(&fitted.capture);
    println!(
        "saved at stage {}: scale x{}, thin {} s, trimmed {} items; {} rows, {} messages, {} loops, {} loop steps; kept {} of {} changes; {} fits in {} ms (inside the caps: {})",
        fitted.stage.as_str(),
        fitted.scale,
        fitted.thin_seconds,
        fitted.trimmed,
        size.rows,
        size.messages,
        size.effects,
        size.cycle_steps,
        fitted.kept_items,
        fitted.exact_items,
        fitted.fits,
        fitted.fit_ms,
        caps.fits(&size)
    );
}
