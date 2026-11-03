//! Pure Lightkey rules checked against `tests/fixtures/lightkey_golden.json`.

use serde_json::Value;
use stagepilot_core::config::{
    clean_lightkey_api_url, LightkeyDetail, LightkeySettings, LightsSettings,
};
use stagepilot_lights::lightkey::{effective_api_url, recording_is_worth_keeping};

const GOLDEN_TEXT: &str = include_str!("fixtures/lightkey_golden.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN_TEXT).expect("golden json")
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
            recording_is_worth_keeping(covered, duration),
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
            ..LightkeySettings::default()
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
            replay: true,
            detail: LightkeyDetail::Normal,
            audio_device: None,
        }
    );
    // A partial block keeps the other field's default.
    let partial: LightsSettings =
        serde_json::from_value(serde_json::json!({"lightkey": {"api_url": "10.0.0.5"}})).unwrap();
    assert!(partial.lightkey.replay);
}

#[test]
fn detail_defaults_to_normal_and_round_trips() {
    assert_eq!(LightkeySettings::default().detail, LightkeyDetail::Normal);
    assert_eq!(LightkeyDetail::Fine.interval_seconds(), 0.25);
    assert_eq!(LightkeyDetail::Normal.interval_seconds(), 0.5);
    assert_eq!(LightkeyDetail::Coarse.interval_seconds(), 1.0);
    for (detail, text) in [
        (LightkeyDetail::Fine, "fine"),
        (LightkeyDetail::Normal, "normal"),
        (LightkeyDetail::Coarse, "coarse"),
    ] {
        let json = serde_json::to_value(LightkeySettings {
            detail,
            ..Default::default()
        })
        .unwrap();
        // Normal is the default and is left out of the saved file; the others are written.
        if detail == LightkeyDetail::Normal {
            assert!(json.get("detail").is_none(), "{json}");
        } else {
            assert_eq!(json["detail"], text);
        }
        let back: LightkeySettings = serde_json::from_value(json).unwrap();
        assert_eq!(back.detail, detail);
    }
    // an older saved block without the field loads as Normal
    let old: LightkeySettings =
        serde_json::from_value(serde_json::json!({"api_url": "", "replay": true})).unwrap();
    assert_eq!(old.detail, LightkeyDetail::Normal);
}
