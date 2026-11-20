//! The optional ProPresenter Link setting (`propresenter.link.data_folder`): absent by default and from older files (which save back
//! unchanged), normalised and validated like every other setting, kept when a newer file has more in it, and nothing secret.

use serde_json::{json, Value};
use stagepilot_core::config::{ProPresenterLinkSettings, ProPresenterSettings};
use stagepilot_core::settings::{PersistentSettings, RuntimeSettings};

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
fn an_older_file_without_it_loads_empty_and_saves_back_without_it() {
    let file = json!({"schema_version": 2, "propresenter": {"enabled": true, "host": "10.0.0.5", "port": 1025}});
    let settings = load(file).unwrap();
    assert_eq!(
        settings.propresenter.link,
        ProPresenterLinkSettings::default()
    );
    assert!(settings.propresenter.link.data_folder.is_none());
    let saved = settings.to_file_value();
    assert!(saved["propresenter"].get("link").is_none(), "{saved}");
    assert_eq!(load(saved).unwrap(), settings);
    assert!(
        PersistentSettings::default().to_file_value()["propresenter"]
            .get("link")
            .is_none()
    );
    assert!(ProPresenterSettings::default().link.is_default());
}

#[test]
fn the_folder_round_trips_is_trimmed_and_an_empty_one_is_none() {
    let file = json!({"schema_version": 2, "propresenter": {"link": {"data_folder": "  /Volumes/Show/ProPresenter  "}}});
    let settings = load(file).unwrap();
    assert_eq!(
        settings.propresenter.link.data_folder.as_deref(),
        Some("/Volumes/Show/ProPresenter")
    );
    let saved = settings.to_file_value();
    assert_eq!(
        saved["propresenter"]["link"],
        json!({"data_folder": "/Volumes/Show/ProPresenter"})
    );
    assert_eq!(load(saved).unwrap(), settings);
    let empty =
        load(json!({"schema_version": 2, "propresenter": {"link": {"data_folder": "   "}}}))
            .unwrap();
    assert_eq!(empty.propresenter.link.data_folder, None);
    assert!(
        empty.to_file_value()["propresenter"].get("link").is_none(),
        "an empty section is left out"
    );
    for ok in [
        "/Users/x/Documents/ProPresenter",
        "~/Documents/ProPresenter",
        "C:\\Users\\x\\ProPresenter",
        "D:/PP",
        "\\\\server\\share\\PP",
    ] {
        let file = json!({"schema_version": 2, "propresenter": {"link": {"data_folder": ok}}});
        assert!(load(file).is_ok(), "{ok}");
    }
}

#[test]
fn a_folder_that_is_not_a_full_path_is_refused_with_its_place_in_the_file() {
    for bad in ["relative/path", "ProPresenter", "..", "/tmp/a\nb", "x"] {
        let errors =
            load(json!({"schema_version": 2, "propresenter": {"link": {"data_folder": bad}}}))
                .unwrap_err();
        assert_eq!(
            errors[0]["loc"],
            json!(["propresenter", "link", "data_folder"]),
            "{bad:?}"
        );
        assert!(
            errors[0]["msg"].as_str().unwrap().contains("full path"),
            "{bad:?}"
        );
    }
    let too_long = format!("/{}", "a".repeat(1100));
    assert!(load(
        json!({"schema_version": 2, "propresenter": {"link": {"data_folder": too_long}}})
    )
    .is_err());
}

#[test]
fn a_newer_file_with_more_in_the_section_still_loads() {
    let file = json!({"schema_version": 2, "propresenter": {"link": {"data_folder": "/x", "future": true}}});
    let settings = load(file).unwrap();
    assert_eq!(
        settings.propresenter.link.data_folder.as_deref(),
        Some("/x")
    );
}

#[test]
fn the_runtime_settings_carry_the_section_and_it_holds_nothing_secret() {
    let mut persistent = PersistentSettings::default();
    persistent.propresenter.link.data_folder = Some("/Volumes/x/PP".into());
    assert_eq!(
        RuntimeSettings::from_persistent(&persistent)
            .propresenter
            .link
            .data_folder
            .as_deref(),
        Some("/Volumes/x/PP")
    );
    let text = serde_json::to_string(&persistent.to_file_value())
        .unwrap()
        .to_lowercase();
    assert!(!text.contains("key") || !text.contains("propresenter-link-key"));
    let fields: Vec<String> = serde_json::to_value(&persistent.propresenter.link)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(fields, ["data_folder"], "the only thing in it");
}
