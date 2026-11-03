//! Cue-map revisions (U-01) and the size/latency of a settings file holding a whole service's
//! worth of captured looks (K-01). The measurement prints its numbers (`--nocapture`) and
//! only asserts generous bounds, so it is a tripwire, not a benchmark.

use stagepilot_core::config::*;
use stagepilot_core::settings::*;
use std::collections::BTreeMap;
use std::time::Instant;
use uuid::Uuid;

fn row(at: f64, messages: usize, tag: usize) -> OscLightingCue {
    OscLightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        address: "/stagepilot/look".into(),
        args: vec![],
        label: format!("Look - {messages} fixtures"),
        color: None,
        messages: (0..messages)
            .map(|i| OscMessage {
                address: format!("/fixture/Moving Head {}/overrides/dimmer", (tag + i) % 60),
                args: vec![OscArgument::Float(0.5 + (i as f64) * 0.001)],
            })
            .collect(),
        source: OscCueSource::Captured,
        edited: false,
    }
}

fn song(key: &str, rows: Vec<OscLightingCue>) -> SongLightingCueMap {
    SongLightingCueMap {
        song_key: key.into(),
        song_title: key.into(),
        cues: vec![],
        osc_cues: rows,
        loudness: None,
        captured_edited: false,
    }
}

fn service(dir: &std::path::Path) -> SettingsService {
    SettingsService::new(
        Box::new(SettingsFileStore::new(dir.join("settings.json"))),
        Box::new(MemoryCredentialStore::default()),
        BTreeMap::new(),
    )
}

#[test]
fn every_save_that_changes_a_song_bumps_its_revision_and_other_saves_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path());
    let mut lights = LightsSettings::default();
    lights
        .cue_maps
        .insert("a".into(), song("a", vec![row(1.0, 2, 0)]));
    service.persist_lights(lights.clone()).unwrap();
    let rev = |s: &SettingsService, k: &str| s.snapshot().lights.cue_revisions.get(k).copied();
    assert_eq!(rev(&service, "a"), Some(1));
    // Same rows again: no change, no bump (even if the caller's copy carries no revisions).
    service.persist_lights(lights.clone()).unwrap();
    assert_eq!(rev(&service, "a"), Some(1));
    // A different row list bumps it.
    lights.cue_maps.get_mut("a").unwrap().osc_cues = vec![row(2.0, 1, 1)];
    service.persist_lights(lights.clone()).unwrap();
    assert_eq!(rev(&service, "a"), Some(2));
    // Another song does not touch it.
    lights
        .cue_maps
        .insert("b".into(), song("b", vec![row(1.0, 1, 2)]));
    service.persist_lights(lights.clone()).unwrap();
    assert_eq!(rev(&service, "a"), Some(2));
    assert_eq!(rev(&service, "b"), Some(1));
    // Removing a song counts as a change too, and the number is kept.
    lights.cue_maps.remove("b");
    service.persist_lights(lights).unwrap();
    assert_eq!(rev(&service, "b"), Some(2));
}

#[test]
fn forty_dense_songs_settings_file_size_and_save_latency() {
    // About the dense song: 5,904 messages at Normal, rows of up to 40 fixtures.
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path());
    let mut lights = LightsSettings::default();
    for n in 0..40 {
        let rows: Vec<OscLightingCue> = (0..148)
            .map(|i| row(f64::from(i) * 0.5, 40, i as usize))
            .collect();
        let key = format!("song-{n:02}");
        lights.cue_maps.insert(key.clone(), song(&key, rows));
    }
    let messages: usize = lights
        .cue_maps
        .values()
        .flat_map(|m| &m.osc_cues)
        .map(|c| c.messages.len())
        .sum();
    let started = Instant::now();
    service.persist_lights(lights.clone()).unwrap();
    let first_save = started.elapsed();
    let bytes = std::fs::metadata(dir.path().join("settings.json"))
        .unwrap()
        .len();
    // One edit in one song: the whole file is rewritten.
    lights.cue_maps.get_mut("song-07").unwrap().osc_cues.pop();
    let started = Instant::now();
    service.persist_lights(lights).unwrap();
    let edit_save = started.elapsed();
    let started = Instant::now();
    let payload = serde_json::to_vec(&service.snapshot()).unwrap().len();
    let serialise = started.elapsed();
    println!(
        "K-01 40 songs, {messages} messages: settings.json {} KiB ({:.1} MiB); first save {:?}, one-edit save {:?}; GET settings payload {} KiB, serialised in {:?}",
        bytes / 1024,
        bytes as f64 / 1_048_576.0,
        first_save,
        edit_save,
        payload / 1024,
        serialise
    );
    assert!(
        edit_save.as_secs_f64() < 5.0,
        "a one-edit save took {edit_save:?}"
    );
}
