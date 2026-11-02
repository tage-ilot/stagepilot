//! Loudness on a song's cue map: optional, old files load and re-save unchanged, caps validate.
use stagepilot_core::config::{
    LightkeySettings, LightsSettings, SongLightingCueMap, SongLoudness, MAX_LOUDNESS_TAKES,
    MAX_LOUDNESS_VALUES,
};

const OLD: &str = r#"{"song_key":"s","song_title":"Old","cues":[],"osc_cues":[]}"#;

#[test]
fn old_cue_map_loads_without_loudness_and_resaves_unchanged() {
    let map: SongLightingCueMap = serde_json::from_str(OLD).unwrap();
    assert!(map.loudness.is_none());
    let again: serde_json::Value = serde_json::to_value(&map).unwrap();
    assert!(
        again.get("loudness").is_none(),
        "absent stays absent: {again}"
    );
}

#[test]
fn loudness_round_trips() {
    let text = r#"{"song_key":"s","song_title":"T","cues":[],"osc_cues":[],"loudness":{"step_seconds":0.25,"values":[0,1,255],"takes":2}}"#;
    let map: SongLightingCueMap = serde_json::from_str(text).unwrap();
    assert_eq!(map.loudness.as_ref().unwrap().values, vec![0, 1, 255]);
    assert_eq!(serde_json::to_string(&map).unwrap(), text);
}

fn lights_with(loudness: SongLoudness) -> LightsSettings {
    let mut map: SongLightingCueMap = serde_json::from_str(OLD).unwrap();
    map.loudness = Some(loudness);
    let mut lights = LightsSettings::default();
    lights.cue_maps.insert("s".into(), map);
    lights
}

fn errors_of(loudness: SongLoudness) -> usize {
    let mut errors = Vec::new();
    lights_with(loudness).validate(&[], &mut errors);
    errors.len()
}

#[test]
fn validation_caps() {
    let ok = SongLoudness {
        step_seconds: 0.25,
        values: vec![1; MAX_LOUDNESS_VALUES],
        takes: MAX_LOUDNESS_TAKES,
    };
    assert_eq!(errors_of(ok), 0);
    let long = SongLoudness {
        step_seconds: 0.25,
        values: vec![1; MAX_LOUDNESS_VALUES + 1],
        takes: 1,
    };
    assert_eq!(errors_of(long), 1);
    let takes = SongLoudness {
        step_seconds: 0.25,
        values: vec![1],
        takes: MAX_LOUDNESS_TAKES + 1,
    };
    assert_eq!(errors_of(takes), 1);
    let step = SongLoudness {
        step_seconds: 0.0,
        values: vec![1],
        takes: 1,
    };
    assert_eq!(errors_of(step), 1);
}

#[test]
fn audio_device_setting_is_optional_and_omitted_when_unset() {
    let settings: LightkeySettings =
        serde_json::from_str(r#"{"api_url":"","replay":true}"#).unwrap();
    assert!(settings.audio_device.is_none());
    assert_eq!(
        serde_json::to_string(&settings).unwrap(),
        r#"{"api_url":"","replay":true}"#
    );
    let named: LightkeySettings = serde_json::from_str(r#"{"audio_device":"Scarlett"}"#).unwrap();
    assert_eq!(named.audio_device.as_deref(), Some("Scarlett"));
}
