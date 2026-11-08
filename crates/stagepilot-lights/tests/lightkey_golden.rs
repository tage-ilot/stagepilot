//! Pure Lightkey rules checked against `tests/fixtures/lightkey_golden.json`.

use serde_json::Value;
use stagepilot_core::config::{
    clean_lightkey_api_url, LightkeyDetail, LightkeyMode, LightkeySettings, LightsSettings,
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
            mode: LightkeyMode::Auto,
            detail: LightkeyDetail::Normal,
            audio_device: None,
            found_url: String::new(),
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

#[test]
fn mode_defaults_to_auto_round_trips_and_is_left_out_while_auto() {
    use stagepilot_core::config::LightkeyMode;
    assert_eq!(LightkeySettings::default().mode, LightkeyMode::Auto);
    // An old file (no mode) loads as Auto and saves back without one.
    let old: LightkeySettings =
        serde_json::from_value(serde_json::json!({"replay": false})).unwrap();
    assert_eq!(old.mode, LightkeyMode::Auto);
    assert!(!old.replay, "the old replay switch keeps working");
    assert!(serde_json::to_value(&old).unwrap().get("mode").is_none());
    for (text, mode) in [
        ("replay", LightkeyMode::Replay),
        ("record", LightkeyMode::Record),
    ] {
        let loaded: LightkeySettings =
            serde_json::from_value(serde_json::json!({"mode": text})).unwrap();
        assert_eq!(loaded.mode, mode);
        assert_eq!(serde_json::to_value(&loaded).unwrap()["mode"], text);
    }
}

#[test]
fn a_cue_map_capture_status_is_optional_and_round_trips() {
    use stagepilot_core::config::{CaptureStatus, SongLightingCueMap};
    let old: SongLightingCueMap =
        serde_json::from_value(serde_json::json!({"song_key": "a", "song_title": "A"})).unwrap();
    assert!(old.capture.is_none());
    assert!(serde_json::to_value(&old).unwrap().get("capture").is_none());
    let with = SongLightingCueMap {
        capture: Some(CaptureStatus {
            complete: false,
            coverage_seconds: 12.5,
            recorded_at: "2026-11-02T00:00:00Z".into(),
            project: None,
            simplified: None,
        }),
        ..old
    };
    let back: SongLightingCueMap =
        serde_json::from_value(serde_json::to_value(&with).unwrap()).unwrap();
    assert_eq!(back, with);
}

#[test]
fn a_capture_project_is_optional_additive_and_round_trips() {
    use stagepilot_core::config::CaptureStatus;
    // a capture saved before this field existed loads with no project and saves back without one
    let old: CaptureStatus = serde_json::from_value(serde_json::json!({
        "complete": true, "coverage_seconds": 100.0, "recorded_at": "2026-11-02T00:00:00Z"
    }))
    .unwrap();
    assert_eq!(old.project, None);
    assert!(serde_json::to_value(&old).unwrap().get("project").is_none());
    // with a project it is stored under "project" and survives a round trip
    let with = CaptureStatus {
        project: Some("Show.lightkeyproj".into()),
        ..old
    };
    let value = serde_json::to_value(&with).unwrap();
    assert_eq!(value["project"], "Show.lightkeyproj");
    let back: CaptureStatus = serde_json::from_value(value).unwrap();
    assert_eq!(back, with);
    // a build that does not know the field (rc.5) must still load the file: no capture struct
    // on the settings path forbids unknown keys, so the extra key is simply ignored
    let mut lights = serde_json::json!({"enabled": false});
    lights["cue_maps"] = serde_json::json!({"s": {"song_key": "s", "song_title": "S", "capture": serde_json::to_value(&with).unwrap()}});
    let parsed: stagepilot_core::config::LightsSettings = serde_json::from_value(lights).unwrap();
    assert_eq!(
        parsed.cue_maps["s"]
            .capture
            .as_ref()
            .unwrap()
            .project
            .as_deref(),
        Some("Show.lightkeyproj")
    );
}

#[test]
fn a_capture_simplified_note_is_optional_additive_and_round_trips() {
    use stagepilot_core::config::{CaptureSimplified, CaptureStatus};
    // a capture saved before this field existed loads with no note and saves back without one
    let old: CaptureStatus = serde_json::from_value(serde_json::json!({
        "complete": true, "coverage_seconds": 100.0, "recorded_at": "2026-11-02T00:00:00Z",
        "project": "Show.lightkeyproj"
    }))
    .unwrap();
    assert_eq!(old.simplified, None);
    assert!(serde_json::to_value(&old)
        .unwrap()
        .get("simplified")
        .is_none());
    let with = CaptureStatus {
        simplified: Some(CaptureSimplified {
            stage: "thinned".into(),
            tolerance_scale: 1.0,
            thin_seconds: 0.5,
            trimmed: 0,
            kept_changes: 700,
            recorded_changes: 11_000,
        }),
        ..old
    };
    let value = serde_json::to_value(&with).unwrap();
    assert_eq!(value["simplified"]["stage"], "thinned");
    // the keys of the note an older build would not know are all inside `simplified`
    assert_eq!(value["project"], "Show.lightkeyproj");
    let back: CaptureStatus = serde_json::from_value(value).unwrap();
    assert_eq!(back, with);
}
