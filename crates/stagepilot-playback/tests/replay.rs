//! Replays synthetic wire traffic through the Rust normalizer and compares the event
//! stream, message by message, with the output the Python normalizer produced for the
//! same input (`tools/gen_replay.py`).

use serde_json::{json, Value};
use stagepilot_playback::normalizer::{Normalizer, SetlistId};

/// Python's `asdict` output with None/False dropped; numbers compared as f64.
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

#[test]
fn rust_normalizer_matches_python_event_stream() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/replay.json")).expect("fixture");
    let (mut messages, mut events) = (0usize, 0usize);
    for case in &cases {
        let fast = case["fast"].as_bool().unwrap();
        let seed = case["seed"].as_i64().unwrap();
        let mut norm = Normalizer::new(fast);
        let msgs = case["messages"].as_array().unwrap();
        let expected = case["events"].as_array().unwrap();
        for (i, m) in msgs.iter().enumerate() {
            let ts = m[0].as_f64().unwrap();
            let raw = m[1].as_str().unwrap();
            let got: Vec<Value> = norm
                .feed(raw, ts)
                .iter()
                .map(|e| canon(&serde_json::to_value(e).unwrap()))
                .collect();
            let want: Vec<Value> = expected[i].as_array().unwrap().iter().map(canon).collect();
            assert_eq!(got, want, "seed {seed} fast {fast} message {i}: {raw}");
            messages += 1;
            events += got.len();
        }
        let want_id = &case["setlist_id"];
        match (&norm.setlist_id, want_id) {
            (None, Value::Null) => {}
            (Some(SetlistId::Int(a)), Value::Number(b)) => assert_eq!(Some(*a), b.as_i64()),
            (Some(SetlistId::Str(a)), Value::String(b)) => assert_eq!(a, b),
            (g, w) => panic!("setlist id mismatch seed {seed}: {g:?} vs {w}"),
        }
    }
    assert!(events > 1000, "fixture too thin: {events} events");
    eprintln!(
        "replayed {} cases, {messages} messages, {events} events: identical",
        cases.len()
    );
}
