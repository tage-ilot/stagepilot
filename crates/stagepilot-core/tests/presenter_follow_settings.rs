//! The follow-Playback setting (`propresenter.presenter_follow`): absent by default and from older files (which save back
//! unchanged), read as off, range-checked like every other setting, kept when a newer file has more in it, nothing secret.

use serde_json::{json, Value};
use stagepilot_core::config::{PresenterFollowSettings, ProPresenterSettings};
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
fn an_older_file_without_it_reads_as_off_and_saves_back_without_it() {
    let file = json!({"schema_version": 2, "propresenter": {"enabled": true, "host": "10.0.0.5", "port": 1025}});
    let settings = load(file).unwrap();
    assert_eq!(
        settings.propresenter.presenter_follow,
        PresenterFollowSettings {
            enabled: false,
            offset_ms: 0
        }
    );
    let saved = settings.to_file_value();
    assert!(
        saved["propresenter"].get("presenter_follow").is_none(),
        "{saved}"
    );
    assert_eq!(load(saved).unwrap(), settings);
    assert!(
        PersistentSettings::default().to_file_value()["propresenter"]
            .get("presenter_follow")
            .is_none()
    );
    assert!(ProPresenterSettings::default()
        .presenter_follow
        .is_default());
}

#[test]
fn the_switch_and_the_offset_round_trip() {
    let file = json!({"schema_version": 2, "propresenter": {"presenter_follow": {"enabled": true, "offset_ms": -350}}});
    let settings = load(file).unwrap();
    assert!(settings.propresenter.presenter_follow.enabled);
    assert_eq!(settings.propresenter.presenter_follow.offset_ms, -350);
    let saved = settings.to_file_value();
    assert_eq!(
        saved["propresenter"]["presenter_follow"],
        json!({"enabled": true, "offset_ms": -350})
    );
    assert_eq!(load(saved).unwrap(), settings);
}

#[test]
fn a_half_written_section_fills_in_the_defaults() {
    let only_offset = load(
        json!({"schema_version": 2, "propresenter": {"presenter_follow": {"offset_ms": 500}}}),
    )
    .unwrap();
    assert!(!only_offset.propresenter.presenter_follow.enabled);
    assert_eq!(only_offset.propresenter.presenter_follow.offset_ms, 500);
    let only_switch =
        load(json!({"schema_version": 2, "propresenter": {"presenter_follow": {"enabled": true}}}))
            .unwrap();
    assert!(only_switch.propresenter.presenter_follow.enabled);
    assert_eq!(only_switch.propresenter.presenter_follow.offset_ms, 0);
    // An empty section is the default and is left out when saving.
    let empty =
        load(json!({"schema_version": 2, "propresenter": {"presenter_follow": {}}})).unwrap();
    assert!(empty.propresenter.presenter_follow.is_default());
    assert!(empty.to_file_value()["propresenter"]
        .get("presenter_follow")
        .is_none());
}

#[test]
fn the_offset_stays_within_two_seconds_either_way() {
    for ok in [-2000, -1, 0, 1, 2000] {
        let file =
            json!({"schema_version": 2, "propresenter": {"presenter_follow": {"offset_ms": ok}}});
        assert!(load(file).is_ok(), "{ok}");
    }
    for bad in [-2001, 2001, 100_000] {
        let errors = load(
            json!({"schema_version": 2, "propresenter": {"presenter_follow": {"offset_ms": bad}}}),
        )
        .unwrap_err();
        assert_eq!(
            errors[0]["loc"],
            json!(["propresenter", "presenter_follow", "offset_ms"]),
            "{bad}"
        );
    }
}

#[test]
fn things_that_are_not_a_switch_or_a_whole_number_are_refused_with_their_place() {
    for (key, bad) in [
        ("enabled", json!("yes")),
        ("enabled", json!(1)),
        ("offset_ms", json!("100")),
        ("offset_ms", json!(1.5)),
        ("offset_ms", Value::Null),
    ] {
        let errors = load(
            json!({"schema_version": 2, "propresenter": {"presenter_follow": {key: bad.clone()}}}),
        )
        .unwrap_err();
        let loc = errors[0]["loc"].to_string();
        assert!(
            loc.contains("presenter_follow") && loc.contains(key),
            "{key}={bad}: {loc}"
        );
    }
    assert!(load(json!({"schema_version": 2, "propresenter": {"presenter_follow": 5}})).is_err());
}

#[test]
fn it_is_saved_with_the_settings_and_holds_nothing_secret() {
    let mut settings = PersistentSettings::default();
    settings.propresenter.presenter_follow = PresenterFollowSettings {
        enabled: true,
        offset_ms: 120,
    };
    let saved = settings.to_file_value();
    assert_eq!(
        saved["propresenter"]["presenter_follow"],
        json!({"enabled": true, "offset_ms": 120})
    );
}
