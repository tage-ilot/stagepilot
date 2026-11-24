//! Write what a saved song plays, on a 25 ms grid, for `scripts/lightkey-fidelity/compare.py --saved`.
//!
//! ```text
//! cargo run --release -p stagepilot-lights --example dump_song -- cue_map.json out.json [FROM_S [TO_S]]
//! ```
//!
//! `cue_map.json` is a song's saved cue map (`osc_cues` and `effects`). The values come from the
//! same [`Timeline::playback_at`] the replay engine plays (rows, ramps, effects, seams blended), so
//! the comparison judges the song as the replay would send it. `out.json` is
//! `{"step_seconds", "from_seconds", "to_seconds", "addresses": {address: [args or null, ...]}}`
//! with sample `k` at song second `from_seconds + k * step_seconds` (`null` = no value yet). The
//! cue map is read only; nothing but `out.json` is written.

use serde_json::{json, Map, Value};
use stagepilot_core::config::SongLightingCueMap;
use stagepilot_lights::lightkey_curve::Timeline;
use stagepilot_lights::lightkey_fixtures::FixtureArg;
use std::process::exit;

const STEP_SECONDS: f64 = 0.025;

fn arg(value: &FixtureArg) -> Value {
    match value {
        FixtureArg::Bool(b) => json!(b),
        FixtureArg::Int(i) => json!(i),
        FixtureArg::Float(f) => json!(f),
        FixtureArg::Text(s) => json!(s),
    }
}

fn number(text: Option<&String>, default: f64) -> f64 {
    match text {
        None => default,
        Some(t) => t.parse().unwrap_or_else(|_| {
            eprintln!("not a number of seconds: {t}");
            exit(2)
        }),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: dump_song cue_map.json out.json [FROM_S [TO_S]]");
        exit(2);
    }
    let song: SongLightingCueMap = match std::fs::read(&args[1])
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
    {
        Ok(song) => song,
        Err(e) => {
            eprintln!("cannot read {}: {e}", args[1]);
            exit(1);
        }
    };
    let timeline = Timeline::new(&song.osc_cues, &song.effects);
    let from = number(args.get(3), 0.0);
    let last_row = song
        .osc_cues
        .iter()
        .map(|c| c.at_seconds + c.fade_seconds)
        .fold(0.0, f64::max);
    let to = number(args.get(4), last_row.max(from));
    if to < from {
        eprintln!("TO_S ({to}) is before FROM_S ({from})");
        exit(2);
    }
    let samples = ((to - from) / STEP_SECONDS).round() as usize + 1;
    let mut addresses = Map::new();
    for address in timeline.addresses() {
        let column: Vec<Value> = (0..samples)
            .map(|k| {
                let at = from + k as f64 * STEP_SECONDS;
                timeline
                    .playback_at(address, at)
                    .map_or(Value::Null, |v| Value::Array(v.iter().map(arg).collect()))
            })
            .collect();
        addresses.insert(address.to_string(), Value::Array(column));
    }
    let count = addresses.len();
    let out = json!({
        "song_title": song.song_title,
        "step_seconds": STEP_SECONDS,
        "from_seconds": from,
        "to_seconds": from + (samples - 1) as f64 * STEP_SECONDS,
        "addresses": addresses,
    });
    if let Err(e) = std::fs::write(&args[2], serde_json::to_vec(&out).unwrap_or_default()) {
        eprintln!("cannot write {}: {e}", args[2]);
        exit(1);
    }
    println!(
        "{}: {} addresses, {} samples ({from} s to {to} s)",
        song.song_title, count, samples
    );
}
