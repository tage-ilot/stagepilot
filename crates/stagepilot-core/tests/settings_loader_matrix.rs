//! M4/t6: the settings loader never destroys data. Every case of the rc.4 audit matrix is a
//! permanent test: the original bytes always survive, unknown keys are kept, damaged or newer
//! files are quarantined, and loading never writes.

use serde_json::{json, Value};
use stagepilot_core::settings::*;
use std::path::{Path, PathBuf};
use tracing::Level;

mod support;
use support::Capture as Logs;

/// Run `f` and return what it returned plus everything it logged.
fn logged<T>(f: impl FnOnce() -> T) -> (T, Logs) {
    let logs = Logs::start();
    let out = f();
    (out, logs)
}

// ---- helpers ----------------------------------------------------------------------------------

struct Case {
    _dir: tempfile::TempDir,
    path: PathBuf,
    original: Vec<u8>,
}

fn case(bytes: &[u8]) -> Case {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, bytes).unwrap();
    Case {
        _dir: dir,
        path,
        original: bytes.to_vec(),
    }
}

fn valid_value() -> Value {
    let mut value = PersistentSettings::default().to_file_value();
    value["timezone"] = json!("America/Chicago");
    value["server_port"] = json!(9123);
    value
}

fn text(value: &Value) -> Vec<u8> {
    let mut s = serde_json::to_string_pretty(value).unwrap();
    s.push('\n');
    s.into_bytes()
}

impl Case {
    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
    fn quarantined(&self) -> Vec<PathBuf> {
        SettingsFileStore::new(&self.path).quarantine_files()
    }
    fn on_disk(&self) -> Vec<u8> {
        std::fs::read(&self.path).unwrap()
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.on_disk()).unwrap()
    }
}

/// The shared expectations for a file that cannot be used.
fn assert_quarantined(c: &Case, store: &SettingsFileStore, logs: &Logs, why: &str) {
    assert_eq!(
        c.on_disk(),
        c.original,
        "load must not touch the original ({why})"
    );
    let copies = c.quarantined();
    assert_eq!(copies.len(), 1, "{why}: {:?}", c.files());
    assert_eq!(std::fs::read(&copies[0]).unwrap(), c.original, "{why}");
    let name = copies[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with("settings.json.corrupt-"), "{name}");
    assert!(name.ends_with('Z'), "UTC timestamp expected: {name}");
    let errors = logs.at(Level::ERROR);
    assert_eq!(errors.len(), 1, "{why}: {errors:?}");
    assert!(errors[0].contains("settings_corrupt"), "{errors:?}");
    assert!(
        errors[0].contains(&c.path.display().to_string()),
        "the ERROR names the file: {errors:?}"
    );
    let degraded = store.degraded().expect("settings_degraded");
    assert_eq!(degraded.quarantine.as_deref(), Some(copies[0].as_path()));
}

// ---- the matrix -------------------------------------------------------------------------------

#[test]
fn a_valid_file_loads_without_writing_or_logging_a_problem() {
    let c = case(&text(&valid_value()));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    let settings = loaded.unwrap().unwrap();
    assert_eq!(settings.timezone, "America/Chicago");
    assert_eq!(c.on_disk(), c.original);
    assert_eq!(c.files(), ["settings.json"]);
    assert!(store.degraded().is_none());
    assert!(logs.at(Level::ERROR).is_empty() && logs.at(Level::WARN).is_empty());
}

#[test]
fn a_truncated_file_is_quarantined_and_defaults_are_used() {
    let full = text(&valid_value());
    let c = case(&full[..full.len() / 2]);
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    assert_eq!(
        loaded.unwrap_err().0,
        "Saved settings are corrupt or invalid; built-in defaults were used."
    );
    assert_quarantined(&c, &store, &logs, "truncated");
    assert!(!store.degraded().unwrap().write_protected);
}

#[test]
fn an_empty_file_is_quarantined() {
    let c = case(b"");
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    assert!(loaded.is_err());
    assert_quarantined(&c, &store, &logs, "empty");
}

#[test]
fn a_utf8_byte_order_mark_is_accepted_and_the_file_untouched() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend(text(&valid_value()));
    let c = case(&bytes);
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    assert_eq!(loaded.unwrap().unwrap().server_port, 9123);
    assert_eq!(c.on_disk(), c.original);
    assert_eq!(c.files(), ["settings.json"]);
    assert!(store.degraded().is_none());
    assert!(logs.at(Level::ERROR).is_empty());
}

#[test]
fn a_wrong_type_is_quarantined_and_the_log_never_carries_the_value() {
    let mut value = valid_value();
    value["server_port"] = json!("abc-secret-looking-value");
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    assert!(loaded.is_err());
    assert_quarantined(&c, &store, &logs, "wrong type");
    assert!(!logs.at(Level::ERROR)[0].contains("abc-secret-looking-value"));
    assert!(store.degraded().unwrap().reason.contains("server_port"));
}

#[test]
fn an_unknown_top_level_key_is_kept_with_one_warning_and_written_back() {
    let mut value = valid_value();
    value["future_feature"] = json!({"enabled": true, "n": [1, 2, 3]});
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    let settings = loaded.unwrap().unwrap();
    assert_eq!(settings.server_port, 9123, "the rest of the file is used");
    assert_eq!(c.on_disk(), c.original);
    assert_eq!(c.files(), ["settings.json"], "nothing quarantined");
    assert!(store.degraded().is_none());
    let warnings = logs.at(Level::WARN);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("future_feature"), "{warnings:?}");
    assert!(
        !warnings[0].contains("[1, 2, 3]"),
        "paths only, never values"
    );
    assert!(logs.at(Level::ERROR).is_empty());

    let mut changed = settings;
    changed.server_port = 9124;
    store.save(&changed).unwrap();
    let saved = c.json();
    assert_eq!(saved["future_feature"], value["future_feature"]);
    assert_eq!(saved["server_port"], 9124);
}

#[test]
fn an_unknown_nested_key_is_kept_at_its_place() {
    let mut value = valid_value();
    value["playback_api"]["future_nested"] = json!("keep me");
    value["lights"]["deeper"] = json!({"a": {"b": 1}});
    value["planning_center_length_undo"] = json!([{
        "service_type_id": "42", "plan_id": "p", "item_id": "i", "title": "t",
        "old_length": 200, "new_length": 210, "written_at": "2026-10-01T12:00:00Z",
        "pending_from_length": null, "future_undo_field": 7
    }]);
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    let settings = loaded.unwrap().unwrap();
    let warnings = logs.at(Level::WARN);
    assert_eq!(warnings.len(), 1, "one WARN for all keys: {warnings:?}");
    for path in [
        "playback_api.future_nested",
        "lights.deeper",
        "planning_center_length_undo.0.future_undo_field",
    ] {
        assert!(warnings[0].contains(path), "{path} in {warnings:?}");
    }
    store.save(&settings).unwrap();
    assert_eq!(c.json(), value, "key set and values identical after a save");
}

#[test]
fn a_newer_schema_is_quarantined_and_never_overwritten() {
    let mut value = valid_value();
    value["schema_version"] = json!(3);
    value["brand_new_section"] = json!({"x": 1});
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    let error = loaded.unwrap_err();
    assert!(
        error.0.contains("newer version of StagePilot"),
        "{}",
        error.0
    );
    assert_quarantined(&c, &store, &logs, "future schema");
    let degraded = store.degraded().unwrap();
    assert!(degraded.write_protected);
    assert!(degraded.reason.contains("schema_version 3"));
    // A save is refused and the file stays as it was.
    let refusal = store.save(&PersistentSettings::default()).unwrap_err();
    assert!(refusal.0.contains("not overwritten"), "{}", refusal.0);
    assert_eq!(c.on_disk(), c.original);
    assert!(!store.backup_path().exists());
}

#[test]
fn binary_garbage_is_quarantined() {
    let garbage: Vec<u8> = (0..512u32).map(|i| (i * 37 % 251) as u8 | 0x80).collect();
    let c = case(&garbage);
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    assert!(loaded.is_err());
    assert_quarantined(&c, &store, &logs, "binary");
}

#[test]
fn a_json_array_is_quarantined() {
    let c = case(b"[1, 2, 3]\n");
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    assert!(loaded.is_err());
    assert_quarantined(&c, &store, &logs, "array");
}

#[test]
fn the_legacy_release_channel_is_dropped_silently() {
    let mut value = valid_value();
    value["release_channel"] = json!("beta");
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    let settings = loaded.unwrap().unwrap();
    assert!(logs.at(Level::WARN).is_empty() && logs.at(Level::ERROR).is_empty());
    assert_eq!(c.on_disk(), c.original, "load does not rewrite it");
    store.save(&settings).unwrap();
    assert!(c.json().get("release_channel").is_none());
}

// ---- loading never writes; the first overwrite keeps a copy ------------------------------------

#[test]
fn loading_a_v1_file_migrates_in_memory_and_the_next_save_writes_it() {
    let mut value = valid_value();
    value["schema_version"] = json!(1);
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let settings = store.load().unwrap().unwrap();
    assert_eq!(c.on_disk(), c.original, "loading never writes");
    assert_eq!(c.files(), ["settings.json"]);
    store.save(&settings).unwrap();
    assert_eq!(c.json()["schema_version"], 2);
    assert_eq!(
        std::fs::read(store.backup_path()).unwrap(),
        c.original,
        "the first overwrite keeps the original as .pre-rust.bak"
    );
}

#[test]
fn the_pre_rust_backup_is_made_once_and_never_replaced() {
    let c = case(&text(&valid_value()));
    let store = SettingsFileStore::new(&c.path);
    let mut settings = store.load().unwrap().unwrap();
    settings.server_port = 9200;
    store.save(&settings).unwrap();
    settings.server_port = 9201;
    store.save(&settings).unwrap();
    assert_eq!(std::fs::read(store.backup_path()).unwrap(), c.original);
}

#[test]
fn the_last_five_quarantine_copies_are_kept_and_an_identical_file_is_copied_once() {
    let c = case(b"{ broken 0");
    for n in 0..8 {
        std::fs::write(&c.path, format!("{{ broken {n}")).unwrap();
        let store = SettingsFileStore::new(&c.path);
        assert!(store.load().is_err());
        // The same broken file at the next launch does not add another copy.
        assert!(SettingsFileStore::new(&c.path).load().is_err());
    }
    let copies = c.quarantined();
    assert_eq!(copies.len(), 5, "{:?}", c.files());
    let kept: Vec<String> = copies
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();
    for n in 3..8 {
        assert!(
            kept.contains(&format!("{{ broken {n}")),
            "{kept:?} {:?}",
            c.files()
        );
    }
}

// ---- a real production file --------------------------------------------------------------------

#[test]
fn a_python_written_production_file_round_trips_with_the_same_keys_and_values() {
    let mut value = valid_value();
    value["log_level"] = json!("DEBUG");
    value["lan_access"] = json!(true);
    value["web_dashboard_pin_hash"] = json!("pbkdf2_sha256$1$AAAA$BBBB");
    value["playback_api"]["captured_version"] = json!(9);
    value["playback_api"]["setlist_id"] = json!(5);
    value["playback_api"]["song_order"] = json!([101, 102, 103, 104]);
    value["playback_api"]["song_lengths"] = json!([279.53, 265.85, 260.17, 410.83]);
    value["playback_api"]["captured_at"] = json!("2026-10-18T17:03:41.123456Z");
    value["playback_api"]["planning_center_update_service_type_id"] = json!("42");
    value["planning_center_length_undo"] = json!([{
        "service_type_id": "42", "plan_id": "p1", "item_id": "i1", "title": "Song",
        "old_length": 275, "new_length": 280, "written_at": "2026-10-18T17:10:00Z",
        "pending_from_length": null
    }]);
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    let settings = loaded.unwrap().unwrap();
    assert!(logs.at(Level::WARN).is_empty() && logs.at(Level::ERROR).is_empty());
    store.save(&settings).unwrap();
    let after = c.json();
    assert_eq!(after, value, "same key set, same values");
    let keys = |v: &Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(keys(&after), keys(&value));
    assert_eq!(after["playback_api"]["captured_version"], 9);
}

// ---- the service reports it --------------------------------------------------------------------

#[test]
fn the_settings_service_reports_a_degraded_start() {
    let c = case(b"not json at all");
    let service = SettingsService::new(
        Box::new(SettingsFileStore::new(&c.path)),
        Box::new(MemoryCredentialStore::default()),
        Default::default(),
    );
    let (runtime, _) = logged(|| service.load());
    assert_eq!(runtime.bind_port, 8765, "defaults");
    assert!(service.degraded().is_some());
    assert!(service.warning().unwrap().contains("built-in defaults"));
    assert_eq!(c.on_disk(), c.original);
}

#[allow(dead_code)]
fn _unused(_: &Path) {}

#[test]
fn a_skip_for_a_song_that_is_not_in_the_saved_order_does_not_spoil_the_file() {
    // rc.5 on the rig: a scan of a new setlist left a skipped song from the old one. The loader
    // judged the whole `playback_api` section invalid, called the file corrupt and the app ran on
    // defaults (no song order, no modes) until the file was edited by hand.
    let mut value = valid_value();
    value["timezone"] = json!("America/Los_Angeles");
    value["playback_api"]["song_order"] = json!([11, 12, 13]);
    value["playback_api"]["skipped_song_ids"] = json!([99, 12]);
    let c = case(&text(&value));
    let store = SettingsFileStore::new(&c.path);
    let (loaded, logs) = logged(|| store.load());
    let settings = loaded
        .expect("a stale skip must not make the file unreadable")
        .unwrap();
    assert_eq!(
        settings.timezone, "America/Los_Angeles",
        "the rest of the file is used"
    );
    assert_eq!(settings.playback_api.song_order, vec![11, 12, 13]);
    assert_eq!(
        settings.playback_api.skipped_song_ids,
        vec![12],
        "only the stale skip is dropped"
    );
    assert!(store.degraded().is_none(), "not degraded");
    assert!(
        logs.at(Level::ERROR).is_empty(),
        "no settings_corrupt error"
    );
    assert_eq!(c.on_disk(), c.original, "loading never writes");
    assert_eq!(c.files(), ["settings.json"], "nothing quarantined");
}
