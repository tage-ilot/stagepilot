//! One song at the per-song caps (R7 of the big-songs work): the validation, the settings save
//! and load, and the size of the file. The timings are printed (`--nocapture`) and asserted
//! against generous bounds (the budgets are 1.5 s to save and 3 s to load on an optimised build;
//! these run unoptimised on a CI runner that may be five times slower).
//!
//! Quick mode: with `STAGEPILOT_QUICK_SONG_TESTS=1` (set by the pull-request run of `rust.yml`
//! only) the save-and-load budget test uses a song with a tenth of the rows and effects, and a
//! tenth of the file-size and time bounds; the cap-boundary test always runs in full (it is
//! cheap, and it is the one that proves the real caps). With the variable unset both run in full,
//! as on main, on a manual dispatch and in a release. The switch is read in `quick`.

use stagepilot_core::config::*;
use stagepilot_core::settings::*;
use std::collections::BTreeMap;
use std::time::Instant;
use uuid::Uuid;

#[path = "support/quick_song_tests.rs"]
mod quick;

/// A song exactly at the caps: rows, messages, effects and cycle steps.
pub fn song_at_the_caps() -> SongLightingCueMap {
    song_at_a_share_of_the_caps(1)
}

/// A song with `1/divisor` of the caps' rows and effects (the same rows and effects, so the same
/// shape): `divisor` 1 is a song exactly at the caps.
fn song_at_a_share_of_the_caps(divisor: usize) -> SongLightingCueMap {
    let per_row = MAX_OSC_MESSAGES_PER_SONG / MAX_OSC_ROWS_PER_SONG;
    assert_eq!(per_row * MAX_OSC_ROWS_PER_SONG, MAX_OSC_MESSAGES_PER_SONG);
    let rows: Vec<OscLightingCue> = (0..MAX_OSC_ROWS_PER_SONG / divisor)
        .map(|r| OscLightingCue {
            id: Uuid::new_v4(),
            at_seconds: (r % 20_000) as f64 * 0.01,
            address: "/stagepilot/look".into(),
            args: vec![],
            label: format!("Look - {per_row} fixtures"),
            color: None,
            messages: (0..per_row)
                .map(|i| OscMessage {
                    address: format!("/fixture/Moving Head {}/overrides/dimmer", (r + i) % 66),
                    args: vec![OscArgument::Float(((r * 7 + i) % 1000) as f64 / 1000.0)],
                })
                .collect(),
            source: OscCueSource::Captured,
            edited: false,
            fade_seconds: if r % 3 == 0 { 0.5 } else { 0.0 },
        })
        .collect();
    let steps_each = MAX_EFFECT_CYCLE_STEPS_PER_SONG / MAX_EFFECTS_PER_SONG;
    assert_eq!(
        steps_each * MAX_EFFECTS_PER_SONG,
        MAX_EFFECT_CYCLE_STEPS_PER_SONG
    );
    let effects: Vec<LightingEffect> = (0..MAX_EFFECTS_PER_SONG / divisor)
        .map(|e| LightingEffect {
            id: Uuid::new_v4(),
            label: format!("Loop {e}"),
            start_seconds: (e % 100) as f64,
            end_seconds: (e % 100) as f64 + 200.0,
            period_seconds: 10.0,
            members: (0..4)
                .map(|m| EffectMember {
                    fixture: format!("Moving Head {}", (e + m) % 66),
                    offset_seconds: m as f64 * 0.1,
                })
                .collect(),
            cycle: (0..steps_each)
                .map(|s| EffectStep {
                    at_seconds: s as f64 * 10.0 / steps_each as f64,
                    property: "dimmer".into(),
                    value: vec![(s % 10) as f64 / 10.0],
                    fade_seconds: 0.01,
                })
                .collect(),
            source: OscCueSource::Captured,
            edited: false,
        })
        .collect();
    SongLightingCueMap {
        song_key: "big".into(),
        song_title: "Big".into(),
        cues: vec![],
        osc_cues: rows,
        loudness: None,
        captured_edited: false,
        capture: None,
        effects,
    }
}

fn errors_of(map: SongLightingCueMap) -> Vec<String> {
    let mut lights = LightsSettings::default();
    lights.cue_maps.insert(map.song_key.clone(), map);
    lights.normalise();
    let mut errors = Vec::new();
    lights.validate(&[], &mut errors);
    errors.into_iter().map(|e| e.msg).collect()
}

#[test]
fn a_song_exactly_at_the_caps_is_accepted_and_one_over_is_refused() {
    let map = song_at_the_caps();
    let started = Instant::now();
    let errors = errors_of(map.clone());
    println!("validation of a song at the caps: {:?}", started.elapsed());
    assert!(errors.is_empty(), "{errors:?}");

    let mut over_rows = map.clone();
    let mut row = over_rows.osc_cues[0].clone();
    row.id = Uuid::new_v4();
    row.messages.clear();
    row.address = "/a".into();
    over_rows.osc_cues.push(row);
    let e = errors_of(over_rows);
    assert_eq!(e.len(), 1, "{e:?}");

    let mut over_messages = map.clone();
    let extra = over_messages.osc_cues[5].messages[0].clone();
    over_messages.osc_cues[5].messages.push(extra);
    assert_eq!(errors_of(over_messages).len(), 1);

    let mut over_effects = map.clone();
    let mut extra = over_effects.effects[0].clone();
    extra.cycle.clear();
    extra.cycle.push(over_effects.effects[0].cycle[0].clone());
    // the extra effect has one step: both the effect count and the step total are now over
    over_effects.effects.push(extra);
    assert!(!errors_of(over_effects).is_empty());

    let mut over_steps = map.clone();
    let step = over_steps.effects[0].cycle[0].clone();
    over_steps.effects[0].cycle.push(step);
    assert_eq!(errors_of(over_steps).len(), 1);
}

#[test]
fn a_song_at_the_caps_saves_and_loads_inside_the_budget() {
    // a tenth of the song in quick mode, with a tenth of the bounds
    let share = quick::pick(1, 10);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let make = || {
        SettingsService::new(
            Box::new(SettingsFileStore::new(&path)),
            Box::new(MemoryCredentialStore::default()),
            BTreeMap::new(),
        )
    };
    let service = make();
    let mut lights = LightsSettings::default();
    let map = song_at_a_share_of_the_caps(share);
    let compact = serde_json::to_vec(&map).unwrap().len();
    lights.cue_maps.insert("big".into(), map);

    let started = Instant::now();
    service.persist_lights(lights.clone()).unwrap();
    let first_save = started.elapsed();
    let bytes = std::fs::metadata(&path).unwrap().len();
    let started = Instant::now();
    service.persist_lights(lights).unwrap();
    let second_save = started.elapsed();

    let fresh = make();
    let started = Instant::now();
    let runtime = fresh.load();
    let load = started.elapsed();
    assert_eq!(
        runtime.lights.cue_maps["big"].osc_cues.len(),
        MAX_OSC_ROWS_PER_SONG / share
    );
    assert_eq!(
        runtime.lights.cue_maps["big"].effects.len(),
        MAX_EFFECTS_PER_SONG / share
    );
    let started = Instant::now();
    let snapshot = serde_json::to_vec(&fresh.snapshot()).unwrap().len();
    let api = started.elapsed();
    println!(
        "song at the caps: compact JSON {:.1} MiB, settings.json {:.1} MiB; save {first_save:?} (again {second_save:?}); load {load:?}; GET settings payload {:.1} MiB in {api:?}",
        compact as f64 / 1_048_576.0,
        bytes as f64 / 1_048_576.0,
        snapshot as f64 / 1_048_576.0,
    );
    assert!(
        bytes < 250 * 1_048_576 / share as u64,
        "settings.json is {bytes} bytes"
    );
    assert!(
        second_save.as_secs() < 60 / share as u64,
        "save took {second_save:?}"
    );
    assert!(load.as_secs() < 90 / share as u64, "load took {load:?}");
}
