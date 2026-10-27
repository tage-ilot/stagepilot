//! The shared golden vectors of the Lightkey rules: `tests/fixtures/lightkey_golden.json` is
//! what Python's `tests/test_lightkey_core.py` runs; this file runs the SAME file, unchanged.
//! Mapping to the Python tests: `docs/archive/rust-migration/lightkey-test-map.md`.

use serde_json::Value;
use stagepilot_core::config::{clean_lightkey_api_url, LightkeySettings, LightsSettings};
use stagepilot_lights::lightkey::{
    build_take_edges, effective_api_url, is_addressable, managed_cues, osc_address, replay_plan,
    state_at, take_is_worth_keeping, LightkeyEdge, LightkeyTake, OperatorHold, Verb,
};
use std::collections::BTreeSet;

const GOLDEN_TEXT: &str = include_str!("fixtures/lightkey_golden.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN_TEXT).expect("golden json")
}

fn take() -> LightkeyTake {
    let mut take: LightkeyTake = serde_json::from_value(golden()["take"].clone()).expect("take");
    take.normalise_and_validate().expect("valid take");
    take
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn set(value: &Value) -> BTreeSet<String> {
    strings(value).into_iter().collect()
}

fn verb(text: &str) -> Verb {
    match text {
        "activate" => Verb::Activate,
        "deactivate" => Verb::Deactivate,
        other => panic!("unknown verb {other}"),
    }
}

#[test]
fn state_at_matches_every_golden_case() {
    let take = take();
    let cases = golden()["state_at"].as_array().unwrap().clone();
    assert!(cases.len() >= 6);
    for case in cases {
        let position = case["position"].as_f64().unwrap();
        assert_eq!(
            state_at(&take, position),
            set(&case["active"]),
            "position {position}"
        );
    }
}

#[test]
fn managed_cues_match_golden() {
    assert_eq!(managed_cues(&take()), set(&golden()["managed"]));
}

#[test]
fn replay_plan_matches_every_golden_case() {
    let take = take();
    let cases = golden()["replay_plan"].as_array().unwrap().clone();
    assert!(cases.len() >= 3);
    for case in cases {
        let position = case["position"].as_f64().unwrap();
        let (sync_now, later) = replay_plan(&take, position);
        let got_sync: Vec<(String, Verb)> =
            sync_now.iter().map(|c| (c.cue.clone(), c.verb)).collect();
        let want_sync: Vec<(String, Verb)> = case["sync_now"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p[0].as_str().unwrap().to_string(),
                    verb(p[1].as_str().unwrap()),
                )
            })
            .collect();
        assert_eq!(got_sync, want_sync, "sync_now at {position}");
        assert!(sync_now.iter().all(|c| c.at_seconds == position));
        let got_later: Vec<(f64, String, Verb)> = later
            .iter()
            .map(|c| (c.at_seconds, c.cue.clone(), c.verb))
            .collect();
        let want_later: Vec<(f64, String, Verb)> = case["later"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p[0].as_f64().unwrap(),
                    p[1].as_str().unwrap().to_string(),
                    verb(p[2].as_str().unwrap()),
                )
            })
            .collect();
        assert_eq!(got_later, want_later, "later at {position}");
    }
}

#[test]
fn unaddressable_cue_names_are_dropped_not_sent() {
    assert!(!is_addressable("Cue/Odd"));
    assert!(!is_addressable(""));
    assert!(!is_addressable("tab\there"));
    assert!(is_addressable("Warm White, 70%"));
    let (sync_now, later) = replay_plan(&take(), 0.0);
    assert!(sync_now
        .iter()
        .chain(later.iter())
        .all(|c| c.cue != "Cue/Odd"));
}

#[test]
fn osc_addresses_match_golden_and_never_use_toggle() {
    for case in golden()["osc_address"].as_array().unwrap() {
        let (cue, verb_text, expected) = (
            case[0].as_str().unwrap(),
            case[1].as_str().unwrap(),
            case[2].as_str().unwrap(),
        );
        assert_eq!(osc_address(cue, verb(verb_text)), expected);
    }
    assert!(!osc_address("x", Verb::Activate).contains("toggle"));
}

#[test]
fn build_take_edges_matches_every_golden_case() {
    let cases = golden()["build_edges"].as_array().unwrap().clone();
    assert!(cases.len() >= 6);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let baseline = strings(&case["baseline"]);
        let observed: Vec<(f64, Vec<String>, Vec<String>)> = case["observed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| (o[0].as_f64().unwrap(), strings(&o[1]), strings(&o[2])))
            .collect();
        let duration = case["duration"].as_f64().unwrap();
        let (initial, edges) =
            build_take_edges(baseline.iter(), &observed, duration).expect("builds");
        assert_eq!(initial, strings(&case["initial"]), "{name}: initial");
        let want: Vec<LightkeyEdge> = case["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| serde_json::from_value(e.clone()).unwrap())
            .collect();
        assert_eq!(edges, want, "{name}: edges");
    }
}

#[test]
fn worth_keeping_matches_golden() {
    for case in golden()["worth_keeping"].as_array().unwrap() {
        let (covered, duration, expected) = (
            case[0].as_f64().unwrap(),
            case[1].as_f64().unwrap(),
            case[2].as_bool().unwrap(),
        );
        assert_eq!(
            take_is_worth_keeping(covered, duration),
            expected,
            "{covered}/{duration}"
        );
    }
}

#[test]
fn operator_hold_matches_every_golden_case() {
    for case in golden()["operator_hold"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut hold = OperatorHold::new();
        for step in case["steps"].as_array().unwrap() {
            let (kind, cue, now) = (
                step[0].as_str().unwrap(),
                step[1].as_str().unwrap(),
                step[2].as_f64().unwrap(),
            );
            match kind {
                "sent" => hold.note_sent(cue, now),
                "change" => assert_eq!(
                    hold.note_change(cue, now),
                    step[3].as_bool().unwrap(),
                    "{name}: change {cue} at {now}"
                ),
                "held" => assert_eq!(
                    hold.is_held(cue, now),
                    step[3].as_bool().unwrap(),
                    "{name}: held {cue} at {now}"
                ),
                other => panic!("unknown step {other}"),
            }
        }
    }
}

#[test]
fn held_cues_lists_only_live_holds_sorted() {
    let mut hold = OperatorHold::new();
    hold.note_change("B", 10.0);
    hold.note_change("A", 11.0);
    assert_eq!(hold.held_cues(12.0), vec!["A", "B"]);
    assert_eq!(hold.held_cues(13.5), vec!["A"]);
    assert!(hold.held_cues(20.0).is_empty());
}

#[test]
fn edge_rules() {
    assert!(LightkeyEdge::new(1.0, vec![], vec![]).is_err());
    assert!(LightkeyEdge::new(1.0, vec!["A".into()], vec!["A".into()]).is_err());
    assert_eq!(
        LightkeyEdge::new(1.23456, vec!["A".into()], vec![])
            .unwrap()
            .at_seconds,
        1.235
    );
    assert!(LightkeyEdge::new(-1.0, vec!["A".into()], vec![]).is_err());
    assert!(LightkeyEdge::new(86_400.0, vec!["A".into()], vec![]).is_err());
}

#[test]
fn take_sorts_its_edges() {
    let take = LightkeyTake::new(
        "k".into(),
        "t".into(),
        "2026-10-20T00:00:00Z".parse().unwrap(),
        10.0,
        vec![],
        vec![
            LightkeyEdge::new(5.0, vec!["B".into()], vec![]).unwrap(),
            LightkeyEdge::new(1.0, vec!["A".into()], vec![]).unwrap(),
        ],
    )
    .unwrap();
    let times: Vec<f64> = take.edges.iter().map(|e| e.at_seconds).collect();
    assert_eq!(times, vec![1.0, 5.0]);
}

#[test]
fn effective_api_url_cases() {
    let cases = [
        ("", "10.30.0.211", "http://10.30.0.211:8770"),
        ("10.30.0.211", "x", "http://10.30.0.211:8770"),
        ("10.30.0.211:9000/", "x", "http://10.30.0.211:9000"),
        ("https://lights.local", "x", "https://lights.local:8770"),
    ];
    for (raw, host, expected) in cases {
        let settings = LightkeySettings {
            api_url: clean_lightkey_api_url(raw).expect("valid"),
            replay: true,
        };
        assert_eq!(effective_api_url(&settings, host), expected, "{raw:?}");
    }
}

#[test]
fn api_url_rejects_paths_and_spaces() {
    for raw in ["http://a/b", "a b", "ftp://x/y z"] {
        assert!(clean_lightkey_api_url(raw).is_err(), "{raw:?}");
    }
    assert_eq!(clean_lightkey_api_url("  ").unwrap(), "");
    assert_eq!(
        clean_lightkey_api_url(" 10.0.0.5:8770/ ").unwrap(),
        "http://10.0.0.5:8770"
    );
}

#[test]
fn replay_defaults_on_and_old_settings_load_with_replay_on() {
    assert!(LightkeySettings::default().replay);
    let old = serde_json::json!({"enabled": true, "method": "osc", "osc_host": "1.2.3.4", "cue_maps": {}});
    let loaded: LightsSettings = serde_json::from_value(old).unwrap();
    assert_eq!(
        loaded.lightkey,
        LightkeySettings {
            api_url: String::new(),
            replay: true
        }
    );
    // A partial block keeps the other field's default.
    let partial: LightsSettings =
        serde_json::from_value(serde_json::json!({"lightkey": {"api_url": "10.0.0.5"}})).unwrap();
    assert!(partial.lightkey.replay);
}
