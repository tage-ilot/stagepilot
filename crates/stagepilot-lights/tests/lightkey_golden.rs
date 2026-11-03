//! Pure Lightkey rules checked against `tests/fixtures/lightkey_golden.json`. The cue rules the
//! file once covered (state_at, replay_plan, operator hold...) are gone with cue replay; the file
//! still serves as an old-format take that must keep loading.

use serde_json::Value;
use stagepilot_core::config::{clean_lightkey_api_url, LightkeySettings, LightsSettings};
use stagepilot_lights::lightkey::{
    effective_api_url, summarize, take_is_worth_keeping, LightkeyTake,
};

const GOLDEN_TEXT: &str = include_str!("fixtures/lightkey_golden.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN_TEXT).expect("golden json")
}

#[test]
fn an_old_take_with_cue_data_still_loads_and_is_written_back_without_it() {
    let mut take: LightkeyTake = serde_json::from_value(golden()["take"].clone()).expect("take");
    take.normalise_and_validate().expect("valid take");
    assert!(take.fixtures.is_none());
    let written = serde_json::to_value(&take).unwrap();
    assert!(written.get("initial").is_none() && written.get("edges").is_none());
    // a take with no fixture track is "no fixture capture"
    assert_eq!(summarize(&take).fixture_count, None);
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
