//! `test_lights_settings.py`: lights output settings and per-song cues survive a settings-service
//! restart, and `STAGEPILOT_LIGHTS_*` variables override the saved output settings.

use stagepilot_core::config::{LightingCue, LightsSettings, SongLightingCueMap};
use stagepilot_core::settings::*;
use std::collections::BTreeMap;
use uuid::Uuid;

fn configured_lights() -> LightsSettings {
    let mut lights = LightsSettings {
        enabled: true,
        output_name: Some("StagePilot to Lightkey".into()),
        channel: 4,
        pulse_ms: 125,
        ..LightsSettings::default()
    };
    lights.cue_maps.insert(
        "song-1".into(),
        SongLightingCueMap {
            song_key: "song-1".into(),
            song_title: "Holy Forever".into(),
            cues: vec![LightingCue {
                id: Uuid::new_v4(),
                at_seconds: 65.0,
                note: 72,
                velocity: 110,
                label: "First chorus".into(),
                color: None,
            }],
            osc_cues: vec![],
        },
    );
    lights
}

fn persistent() -> PersistentSettings {
    PersistentSettings {
        lights: configured_lights(),
        ..PersistentSettings::default()
    }
}

#[test]
fn lights_output_and_song_cues_survive_settings_service_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("StagePilot").join("settings.json");
    let service = |path: &std::path::Path| {
        SettingsService::new(
            Box::new(SettingsFileStore::new(path)),
            Box::new(MemoryCredentialStore::default()),
            BTreeMap::new(),
        )
    };
    let first = service(&path);
    first.load();
    first.save(persistent()).unwrap();

    let runtime = service(&path).load();

    assert!(runtime.lights.enabled);
    assert_eq!(
        runtime.lights.output_name.as_deref(),
        Some("StagePilot to Lightkey")
    );
    assert_eq!(runtime.lights.channel, 4);
    assert_eq!(runtime.lights.pulse_ms, 125);
    let cue = &runtime.lights.cue_maps["song-1"].cues[0];
    assert_eq!(cue.at_seconds, 65.0);
    assert_eq!(cue.note, 72);
    assert_eq!(cue.velocity, 110);
    assert_eq!(cue.label, "First chorus");
}

#[test]
fn lights_environment_values_override_saved_output_settings() {
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsFileStore::new(dir.path().join("settings.json"));
    store.save(&persistent()).unwrap();
    let environ: BTreeMap<String, String> = [
        ("STAGEPILOT_LIGHTS_ENABLED", "true"),
        ("STAGEPILOT_LIGHTS_OUTPUT_NAME", "Rehearsal Lightkey"),
        ("STAGEPILOT_LIGHTS_CHANNEL", "7"),
        ("STAGEPILOT_LIGHTS_PULSE_MS", "250"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let service = SettingsService::new(
        Box::new(store),
        Box::new(MemoryCredentialStore::default()),
        environ,
    );

    let runtime = service.load();

    assert_eq!(
        runtime.lights.output_name.as_deref(),
        Some("Rehearsal Lightkey")
    );
    assert_eq!(runtime.lights.channel, 7);
    assert_eq!(runtime.lights.pulse_ms, 250);
    assert!(runtime.lights.cue_maps.contains_key("song-1"));
}
