//! F-G: a damaged song's lighting data never makes the whole settings file unloadable.
//!
//! The fixture is a trimmed copy of the settings the iMac saved on 2026-11-08 (rc.9): song
//! "Ever Be" has an effect member (`Z1`, effect 5, period 8.003 s) whose offset is 8.025 s, which
//! is past the period and failed validation, so the whole file was refused and every setting went
//! back to the defaults. Hosts were replaced by documentation addresses; there are no secrets.

use serde_json::{json, Value};
use stagepilot_core::config::{check_cue_map, SongLightingCueMap};
use stagepilot_core::settings::{prepare_cue_map_for_save, PersistentSettings, SettingsStore};
use stagepilot_core::settings_file::{SettingsFileStore, SONG_ASIDE_KEEP};

const FIXTURE: &str = include_str!("fixtures/settings_ever_be_offset.json");
const EVER_BE: &str = "28682835";
const OTHER: &str = "23667144";

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn store_with(value: &Value) -> (tempfile::TempDir, SettingsFileStore) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    let store = SettingsFileStore::new(&path);
    (dir, store)
}

fn names(dir: &tempfile::TempDir) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn the_real_file_is_invalid_under_the_loaders_rules_before_the_repair() {
    // (a) The check proves something: the settings rules reject the file as it was saved.
    let errors = PersistentSettings::from_value_ref(&fixture()).unwrap_err();
    let loc: Vec<String> = errors[0]
        .loc
        .iter()
        .map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_string))
        .collect();
    assert_eq!(
        loc.join("."),
        "lights.cue_maps.28682835.effects.5.offset_seconds"
    );
    assert_eq!(errors[0].kind, "less_than_equal");
    assert_eq!(errors.len(), 1, "that single value was the only problem");
}

#[test]
fn the_file_loads_with_the_offset_in_phase_and_everything_else_unchanged() {
    // (b)
    let (dir, store) = store_with(&fixture());
    let settings = store.load().unwrap().expect("loads");
    assert!(store.degraded().is_none());
    assert!(store.notices().is_empty());
    let ever = &settings.lights.cue_maps[EVER_BE];
    let z1 = ever.effects[5]
        .members
        .iter()
        .find(|m| m.fixture == "Z1")
        .unwrap();
    assert_eq!(z1.offset_seconds, 0.022);
    // no quarantine, no set-aside copy
    assert_eq!(names(&dir), ["settings.json"]);

    store.save(&settings).unwrap();
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();

    // Every other section and song equals the original.
    let original = fixture();
    for (key, value) in original.as_object().unwrap() {
        if key == "lights" {
            continue;
        }
        assert_eq!(&saved[key], value, "section {key}");
    }
    assert_eq!(
        saved["lights"]["cue_maps"][OTHER],
        original["lights"]["cue_maps"][OTHER]
    );
    let mut expected_ever = original["lights"]["cue_maps"][EVER_BE].clone();
    expected_ever["effects"][5]["members"][1]["offset_seconds"] = json!(0.022);
    assert_eq!(saved["lights"]["cue_maps"][EVER_BE], expected_ever);

    // Byte for byte: the same file with only that one value corrected by hand saves identically.
    let by_hand = FIXTURE.replace("\"offset_seconds\": 8.025", "\"offset_seconds\": 0.022");
    assert_ne!(by_hand, FIXTURE);
    let hand: Value = serde_json::from_str(&by_hand).unwrap();
    let (dir2, store2) = store_with(&hand);
    store2.save(&store2.load().unwrap().unwrap()).unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("settings.json")).unwrap(),
        std::fs::read(dir2.path().join("settings.json")).unwrap()
    );
}

fn unrepairable() -> Value {
    let mut v = fixture();
    // a cycle step past its period cannot be repaired without changing the look
    v["lights"]["cue_maps"][EVER_BE]["effects"][5]["cycle"][2]["at_seconds"] = json!(99.0);
    v
}

#[test]
fn an_unrepairable_song_is_set_aside_and_everything_else_loads() {
    // (c)
    let (dir, store) = store_with(&unrepairable());
    let settings = store.load().unwrap().expect("loads");
    assert!(store.degraded().is_none(), "the file itself is in use");
    assert!(!settings.lights.cue_maps.contains_key(EVER_BE));
    let original = fixture();
    let expected_other: SongLightingCueMap =
        serde_json::from_value(original["lights"]["cue_maps"][OTHER].clone()).unwrap();
    assert_eq!(settings.lights.cue_maps[OTHER], expected_other);
    assert_eq!(settings.server_port, 8765);
    assert_eq!(settings.timezone, "America/Los_Angeles");

    // the notice, in plain words
    assert_eq!(
        store.notices(),
        ["The lighting data for Ever Be was damaged and set aside; record it again."]
    );

    // the song's own copy next to the file
    let asides = store.song_aside_files();
    assert_eq!(asides.len(), 1);
    let name = asides[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("settings.json.song-28682835-") && name.ends_with(".json"),
        "{name}"
    );
    let copy: Value = serde_json::from_slice(&std::fs::read(&asides[0]).unwrap()).unwrap();
    assert_eq!(copy["song_title"], "Ever Be");
    assert_eq!(copy["effects"][5]["cycle"][2]["at_seconds"], json!(99.0));
    // no whole-file quarantine
    assert!(!names(&dir).iter().any(|n| n.contains(".corrupt-")));

    // a write after the load does not bring the bad song back
    store.save(&settings).unwrap();
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();
    assert!(saved["lights"]["cue_maps"].get(EVER_BE).is_none());
    assert_eq!(
        saved["lights"]["cue_maps"][OTHER],
        original["lights"]["cue_maps"][OTHER]
    );
    let reloaded = store.load().unwrap().unwrap();
    assert!(!reloaded.lights.cue_maps.contains_key(EVER_BE));
    assert!(
        store.notices().is_empty(),
        "nothing is set aside the second time"
    );
}

#[test]
fn invalid_values_outside_the_cue_maps_still_refuse_the_whole_file() {
    let mut v = unrepairable();
    v["server_port"] = json!(0);
    let (dir, store) = store_with(&v);
    assert!(store.load().is_err());
    assert!(store.degraded().is_some());
    assert!(names(&dir).iter().any(|n| n.contains(".corrupt-")));
    assert!(store.song_aside_files().is_empty());

    // bad song AND a bad lights value outside the cue maps: still the old behaviour
    let mut v = unrepairable();
    v["lights"]["osc_port"] = json!(0);
    let (dir, store) = store_with(&v);
    assert!(store.load().is_err());
    assert!(names(&dir).iter().any(|n| n.contains(".corrupt-")));
}

#[test]
fn only_the_newest_five_set_aside_copies_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    for round in 0..(SONG_ASIDE_KEEP + 3) {
        std::fs::write(&path, serde_json::to_vec(&unrepairable()).unwrap()).unwrap();
        let store = SettingsFileStore::new(&path);
        store.load().unwrap().unwrap();
        assert_eq!(
            store.song_aside_files().len(),
            (round + 1).min(SONG_ASIDE_KEEP)
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn saving_repairs_the_exact_ever_be_effect_and_refuses_what_cannot_be_repaired() {
    let original = fixture();
    let mut map: SongLightingCueMap =
        serde_json::from_value(original["lights"]["cue_maps"][EVER_BE].clone()).unwrap();
    // as the recorder wrote it: valid on its own terms only after the repair
    assert!(check_cue_map(&map).is_err());
    prepare_cue_map_for_save(&mut map).expect("repaired");
    assert!(check_cue_map(&map).is_ok());
    let z1 = map.effects[5]
        .members
        .iter()
        .find(|m| m.fixture == "Z1")
        .unwrap();
    assert_eq!(z1.offset_seconds, 0.022);

    let mut broken: SongLightingCueMap =
        serde_json::from_value(unrepairable()["lights"]["cue_maps"][EVER_BE].clone()).unwrap();
    let error = prepare_cue_map_for_save(&mut broken).unwrap_err();
    assert!(
        error.contains("Ever Be") && error.contains("Record the song again"),
        "{error}"
    );
}
