//! Port of `backend/tests/test_playback_settings.py`: the Playback settings block, its
//! validation, the v1 -> v2 migration and persistence. Files are written and read through the
//! real `SettingsFileStore`.

use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use stagepilot_core::config::{IntegrationModes, MidiSource, PlaybackApiSettings, SetlistId};
use stagepilot_core::settings::{
    MemoryCredentialStore, PersistentSettings, SettingsFileStore, SettingsService, SettingsStore,
};
use std::collections::BTreeMap;

fn read(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn parse(value: Value) -> Result<PersistentSettings, ()> {
    PersistentSettings::from_value(value)
        .map(|(settings, _)| settings)
        .map_err(|_| ())
}

fn playback(value: Value) -> Result<PlaybackApiSettings, ()> {
    parse(json!({"schema_version": 2, "playback_api": value})).map(|s| s.playback_api)
}

#[test]
fn new_defaults() {
    let saved = PersistentSettings::default();
    assert_eq!(saved.schema_version, 2);
    assert_eq!(saved.integration_modes.midi_source, MidiSource::PlaybackApi);
    assert_eq!(
        saved.playback_api,
        PlaybackApiSettings {
            enabled: true,
            host: None,
            port: 8080,
            auto_scan: true,
            ..PlaybackApiSettings::default()
        }
    );
    assert!(saved.playback_api.fast_transport);
}

#[test]
fn v1_migration_preserves_values_and_is_idempotent() {
    for source in ["real", "simulated"] {
        for transport in ["local", "network"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            let mut original = serde_json::to_value(PersistentSettings::default()).unwrap();
            original["schema_version"] = json!(1);
            original.as_object_mut().unwrap().remove("playback_api");
            original.as_object_mut().unwrap().remove("network_midi");
            original["integration_modes"]["midi_source"] = json!(source);
            original["midi"]["enabled"] = json!(true);
            original["midi"]["transport"] = json!(transport);
            original["midi"]["input_name"] = json!("Example MIDI");
            original["midi"]["channel"] = json!(7);
            original["midi"]["note"] = json!(83);
            original["midi"]["debounce_ms"] = json!(432);
            original["midi"]["mappings"]["stop_timer"] = json!(127);
            std::fs::write(&path, original.to_string()).unwrap();
            let store = SettingsFileStore::new(&path);
            let migrated = store.load().unwrap().unwrap();
            // An explicit MIDI choice (real + chosen input) stays; the old default moves.
            let expected = if source == "real" {
                MidiSource::Real
            } else {
                MidiSource::PlaybackApi
            };
            assert_eq!(
                migrated.integration_modes.midi_source, expected,
                "{source}/{transport}"
            );
            let migrated_json = serde_json::to_value(&migrated).unwrap();
            assert_eq!(migrated_json["midi"], original["midi"]);
            for key in [
                "lights",
                "planning_center",
                "timezone",
                "propresenter",
                "onboarding",
            ] {
                assert_eq!(migrated_json[key], original[key], "{key}");
            }
            // M4: loading never writes; the migration reaches the disk with the next save.
            assert_eq!(read(&path)["schema_version"], 1);
            store.save(&migrated).unwrap();
            assert_eq!(read(&path)["schema_version"], 2);
            let encoded = std::fs::read(&path).unwrap();
            assert_eq!(store.load().unwrap().unwrap(), migrated);
            assert_eq!(std::fs::read(&path).unwrap(), encoded, "idempotent");
            assert_eq!(parse(migrated_json).unwrap(), migrated);
        }
    }
}

#[test]
fn explicit_v2_selection_and_playback_fields_survive_restart() {
    for source in [
        MidiSource::Simulated,
        MidiSource::Real,
        MidiSource::PlaybackApi,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let store = SettingsFileStore::new(&path);
        let mut saved = PersistentSettings {
            integration_modes: IntegrationModes {
                midi_source: source,
                ..IntegrationModes::default()
            },
            playback_api: PlaybackApiSettings {
                enabled: false,
                host: Some("192.0.2.10".into()),
                port: 8090,
                auto_scan: false,
                song_order: vec![101, 202],
                captured_version: Some(11),
                captured_at: Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()),
                ..PlaybackApiSettings::default()
            },
            ..PersistentSettings::default()
        };
        saved.midi.enabled = true;
        store.save(&saved).unwrap();
        let service = SettingsService::new(
            Box::new(SettingsFileStore::new(&path)),
            Box::new(MemoryCredentialStore::default()),
            BTreeMap::new(),
        );
        let runtime = service.load();
        assert_eq!(runtime.integration_modes.midi_source, source);
        assert_eq!(service.snapshot().playback_api, saved.playback_api);
        assert_eq!(service.snapshot(), saved);
        service.save(service.snapshot()).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), saved);
    }
}

#[test]
fn unsupported_schema_is_rejected() {
    for version in [json!(0), json!(3), json!(true), json!("1")] {
        assert!(
            parse(json!({"schema_version": version.clone()})).is_err(),
            "{version}"
        );
    }
}

#[test]
fn manual_host_is_normalised() {
    for (host, expected) in [
        ("", None),
        ("  ", None),
        (" 192.0.2.10 ", Some("192.0.2.10")),
    ] {
        assert_eq!(
            playback(json!({"host": host})).unwrap().host.as_deref(),
            expected,
            "{host:?}"
        );
    }
}

#[test]
fn invalid_port_is_rejected() {
    for port in [0, 65536] {
        assert!(playback(json!({"port": port})).is_err(), "{port}");
    }
}

#[test]
fn unversioned_file_migrates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{"integration_modes":{"midi_source":"real"}}"#).unwrap();
    let store = SettingsFileStore::new(&path);
    let saved = store.load().unwrap().unwrap();
    assert_eq!(saved.integration_modes.midi_source, MidiSource::PlaybackApi);
    // M4: loading never writes; the next save persists the migration.
    assert!(read(&path).get("schema_version").is_none());
    store.save(&saved).unwrap();
    assert_eq!(read(&path)["schema_version"], 2);
}

#[test]
fn invalid_discovered_order_is_rejected() {
    let too_long: Vec<i64> = (0..201).collect();
    for order in [
        json!([true]),
        json!(["101"]),
        json!([101, 101]),
        json!(too_long),
    ] {
        assert!(playback(json!({"song_order": order.clone()})).is_err());
    }
}

#[test]
fn old_format_files_untouched_default_moves_and_explicit_midi_stays() {
    let cases = [
        (
            "old_default",
            json!({"schema_version": 1, "integration_modes": {"midi_source": "simulated"}}),
            MidiSource::PlaybackApi,
        ),
        (
            "real_no_input",
            json!({"schema_version": 1, "integration_modes": {"midi_source": "real"},
                "midi": {"enabled": true}}),
            MidiSource::PlaybackApi,
        ),
        (
            "explicit",
            json!({"schema_version": 1, "integration_modes": {"midi_source": "real"},
                "midi": {"enabled": true, "input_name": "Example MIDI"}}),
            MidiSource::Real,
        ),
    ];
    for (name, payload, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{name}.json"));
        std::fs::write(&path, payload.to_string()).unwrap();
        let store = SettingsFileStore::new(&path);
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.integration_modes.midi_source, expected, "{name}");
        // M4: loading never writes. Saved as v2, the choice is never re-migrated on the next start.
        assert_eq!(read(&path)["schema_version"], 1);
        store.save(&loaded).unwrap();
        assert_eq!(read(&path)["schema_version"], 2);
        assert_eq!(
            SettingsFileStore::new(&path).load().unwrap().unwrap(),
            loaded
        );
    }
}

#[test]
fn old_playback_settings_load_new_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        json!({"schema_version": 2,
            "playback_api": {"enabled": true, "song_order": [101, 202], "captured_version": 11}})
        .to_string(),
    )
    .unwrap();
    let loaded = SettingsFileStore::new(&path).load().unwrap().unwrap();
    assert!(loaded.playback_api.fast_transport);
    assert_eq!(loaded.playback_api.setlist_id, None);
    assert_eq!(loaded.playback_api.song_order, vec![101, 202]);
}

#[test]
fn fast_transport_and_identity_persist() {
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsFileStore::new(dir.path().join("settings.json"));
    let settings = PersistentSettings {
        playback_api: PlaybackApiSettings {
            fast_transport: false,
            setlist_id: Some(SetlistId::Int(93_000_001)),
            song_order: vec![101],
            captured_version: Some(11),
            ..PlaybackApiSettings::default()
        },
        ..PersistentSettings::default()
    };
    store.save(&settings).unwrap();
    assert_eq!(store.load().unwrap().unwrap(), settings);
}
