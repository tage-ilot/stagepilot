//! The optional Playback library setting: off by default, absent from older files, and a saved file
//! that never had it saves back byte for byte the same.

use serde_json::{json, Value};
use stagepilot_core::config::{clean_playback_library_address, PlaybackLibrarySettings};
use stagepilot_core::settings::PersistentSettings;

fn load(value: Value) -> Result<PersistentSettings, Vec<Value>> {
    PersistentSettings::from_value(value)
        .map(|(settings, _)| settings)
        .map_err(|errors| {
            errors
                .iter()
                .map(|e| serde_json::to_value(e).unwrap())
                .collect()
        })
}

#[test]
fn an_older_file_without_it_loads_off_and_saves_back_without_it() {
    let file = json!({"schema_version": 2, "timezone": "America/Los_Angeles"});
    let settings = load(file).unwrap();
    assert_eq!(
        settings.playback_library,
        PlaybackLibrarySettings::default()
    );
    assert!(!settings.playback_library.enabled);
    let saved = settings.to_file_value();
    assert!(saved.get("playback_library").is_none(), "{saved}");
    // And what was saved loads to the same settings.
    assert_eq!(load(saved).unwrap(), settings);
    // The same for the defaults.
    assert!(PersistentSettings::default()
        .to_file_value()
        .get("playback_library")
        .is_none());
}

#[test]
fn the_setting_round_trips_and_the_address_is_cleaned_like_the_lightkey_helper_address() {
    let file = json!({"schema_version": 2, "playback_library": {"enabled": true, "address": " 10.20.0.102:8780/ "}});
    let settings = load(file).unwrap();
    assert!(settings.playback_library.enabled);
    assert_eq!(settings.playback_library.address, "http://10.20.0.102:8780");
    let saved = settings.to_file_value();
    assert_eq!(
        saved["playback_library"],
        json!({"enabled": true, "address": "http://10.20.0.102:8780"})
    );
    assert_eq!(load(saved).unwrap(), settings);
    // Switched off but with an address kept: still saved, so the address is not lost.
    let kept = load(
        json!({"schema_version": 2, "playback_library": {"enabled": false, "address": "10.0.0.9"}}),
    )
    .unwrap();
    assert_eq!(
        kept.to_file_value()["playback_library"],
        json!({"enabled": false, "address": "http://10.0.0.9"})
    );
    // An empty object is the default.
    assert!(load(json!({"schema_version": 2, "playback_library": {}}))
        .unwrap()
        .playback_library
        .is_default());
}

#[test]
fn a_bad_address_or_an_unknown_key_makes_the_file_invalid() {
    let errors = load(json!({"schema_version": 2, "playback_library": {"enabled": true, "address": "10.0.0.5/x"}})).unwrap_err();
    assert!(
        errors[0]["msg"]
            .as_str()
            .unwrap()
            .contains("host or host:port"),
        "{errors:?}"
    );
    assert_eq!(errors[0]["loc"], json!(["playback_library", "address"]));
    assert!(load(
        json!({"schema_version": 2, "playback_library": {"enabled": true, "waveforms": true}})
    )
    .is_err());
}

#[test]
fn the_address_rule_is_the_lightkey_one() {
    use stagepilot_core::config::clean_lightkey_api_url;
    for text in [
        "",
        "10.0.0.5",
        "10.0.0.5:8780",
        "http://host.local/",
        "https://h:1",
        " h ",
        "a b",
        "h/p",
        "http://",
        "x://",
    ] {
        assert_eq!(
            clean_playback_library_address(text).is_ok(),
            clean_lightkey_api_url(text).is_ok(),
            "{text:?}"
        );
        assert_eq!(
            clean_playback_library_address(text).ok(),
            clean_lightkey_api_url(text).ok(),
            "{text:?}"
        );
    }
}
