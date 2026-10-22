//! Golden relay packets written by the real `lightkey-api` 0.4 encoder (Python), decoded by the
//! Rust client. This is the cross-language check: if either side changes the wire format, this
//! fails. `tests/fixtures/relay_packets.json` is regenerated from `lightkey_api.osc_bundle`.

use serde_json::Value;
use stagepilot_lights::lightkey_fixtures::{parse_packet, parse_pong, FixtureArg, PacketKind};

fn cases() -> Vec<Value> {
    let text = include_str!("fixtures/relay_packets.json");
    let file: Value = serde_json::from_str(text).unwrap();
    file["cases"].as_array().unwrap().clone()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

fn arg_json(arg: &FixtureArg) -> Value {
    match arg {
        FixtureArg::Float(f) => Value::from(*f),
        FixtureArg::Int(i) => Value::from(*i),
        FixtureArg::Text(t) => Value::from(t.clone()),
        FixtureArg::Bool(b) => Value::from(*b),
    }
}

#[test]
fn every_golden_packet_decodes_to_what_the_python_encoder_meant() {
    let cases = cases();
    assert!(cases.len() >= 6);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let data = unhex(case["hex"].as_str().unwrap());
        let expect = &case["expect"];
        if let Some(pong) = expect.get("pong") {
            let (t1, t2, t3) = parse_pong(&data).unwrap_or_else(|| panic!("{name}: pong"));
            let want: Vec<f64> = pong
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            assert_eq!([t1, t2, t3].to_vec(), want, "{name}");
            continue;
        }
        let packet = parse_packet(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(packet.seq, expect["seq"].as_i64().unwrap(), "{name}");
        assert_eq!(packet.part, expect["part"].as_i64().unwrap(), "{name}");
        assert_eq!(packet.parts, expect["parts"].as_i64().unwrap(), "{name}");
        let kind = match expect["kind"].as_str().unwrap() {
            "event" => PacketKind::Event,
            "keyframe" => PacketKind::Keyframe,
            _ => PacketKind::Hello,
        };
        assert_eq!(packet.kind, kind, "{name}");
        assert!(
            (packet.time - expect["time"].as_f64().unwrap()).abs() < 1e-6,
            "{name}: time {}",
            packet.time
        );
        let got: Vec<Value> = packet
            .commands
            .iter()
            .map(|c| {
                Value::Array(vec![
                    Value::from(c.a.clone()),
                    Value::Array(c.v.iter().map(arg_json).collect()),
                ])
            })
            .collect();
        let want = expect["commands"].as_array().unwrap();
        assert_eq!(got.len(), want.len(), "{name}");
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g[0], w[0], "{name}: address");
            let (gv, wv) = (g[1].as_array().unwrap(), w[1].as_array().unwrap());
            assert_eq!(gv.len(), wv.len(), "{name}: argument count");
            for (a, b) in gv.iter().zip(wv) {
                match (a.as_f64(), b.as_f64(), a.is_i64() == b.is_i64()) {
                    // float32 on the wire: compare to 6 digits, and an int must stay an int
                    (Some(x), Some(y), true) => assert!((x - y).abs() < 1e-6, "{name}: {x} vs {y}"),
                    _ => assert_eq!(a, b, "{name}"),
                }
            }
        }
        if let Some(h) = expect.get("hello") {
            let hello = packet.hello.unwrap();
            assert_eq!(hello.version, h["version"].as_i64().unwrap() as i32);
            assert_eq!(hello.stream, h["stream"].as_i64().unwrap());
            assert_eq!(hello.project, h["project"].as_str().unwrap());
            assert_eq!(hello.fixtures, h["fixtures"].as_i64().unwrap());
        }
    }
}
