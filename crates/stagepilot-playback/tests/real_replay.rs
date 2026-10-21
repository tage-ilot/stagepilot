//! Replays redacted REAL Playback captures (tests/fixtures/real/*.jsonl) through the Rust
//! normalizer and compares every event, message by message, with the Python normalizer's
//! output stored in `expected_events.json` (`tools/rust-playback/gen_real_replay.py`).

use serde_json::{json, Value};
use stagepilot_playback::normalizer::Normalizer;

fn canon(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(_, v)| !v.is_null() && **v != Value::Bool(false))
                .map(|(k, v)| (k.clone(), canon(v)))
                .collect(),
        ),
        Value::Number(n) => json!(n.as_f64().unwrap()),
        other => other.clone(),
    }
}

const FILES: &[(&str, &str)] = &[
    (
        "operator-25min.jsonl",
        include_str!("fixtures/real/operator-25min.jsonl"),
    ),
    (
        "operator-pad-off.jsonl",
        include_str!("fixtures/real/operator-pad-off.jsonl"),
    ),
    (
        "rig-commands.jsonl",
        include_str!("fixtures/real/rig-commands.jsonl"),
    ),
    (
        "operator-pad-section.jsonl",
        include_str!("fixtures/real/operator-pad-section.jsonl"),
    ),
];

#[test]
fn real_captures_match_python_event_streams() {
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/real/expected_events.json")).unwrap();
    let (mut frames, mut events) = (0usize, 0usize);
    for (name, body) in FILES {
        for fast in [false, true] {
            let key = format!("{name}|fast={fast}");
            let want = &expected[&key];
            let mut norm = Normalizer::new(fast);
            let mut i = 0;
            for line in body.lines() {
                let rec: Value = serde_json::from_str(line).unwrap();
                if rec["kind"] != "frame" {
                    continue;
                }
                let raw = serde_json::to_string(&rec["msg"]).unwrap();
                let got: Vec<Value> = norm
                    .feed(&raw, rec["t"].as_f64().unwrap())
                    .iter()
                    .map(|e| canon(&serde_json::to_value(e).unwrap()))
                    .collect();
                let exp: Vec<Value> = want["events"][i]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(canon)
                    .collect();
                assert_eq!(got, exp, "{key} frame {i}: {raw}");
                frames += 1;
                events += got.len();
                i += 1;
            }
            assert_eq!(i as u64, want["messages"].as_u64().unwrap(), "{key}");
        }
    }
    assert!(events > 250, "fixture too thin: {events} events");
    eprintln!("real replay: {frames} frames, {events} events: identical to Python");
}
