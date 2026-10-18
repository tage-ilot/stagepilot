//! Port of `backend/tests/test_plan_cache.py` (the file cache of the last-known-good service).

use chrono::{NaiveDate, TimeZone, Utc};
use stagepilot_core::state::{ServicePlan, Song};
use stagepilot_pco::plan_cache::{
    default_plan_cache_path, CachedServicePlan, FilePlanCacheStore, PlanCacheStore,
};
use std::collections::BTreeMap;

fn cached_plan() -> CachedServicePlan {
    let mut song = Song::new("item-1", "Holy Forever", Some(336), 1);
    song.source_song_id = Some("song-1".into());
    CachedServicePlan {
        schema_version: 1,
        plan: ServicePlan {
            id: "plan-1".into(),
            title: "Sunday Morning".into(),
            date: NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
            service_type: "Weekend Services".into(),
            service_type_id: Some("42".into()),
            service_times: vec!["09:00".into()],
            duration_source: "Planning Center scheduled item length".into(),
            songs: vec![song],
        },
        last_successful_refresh: Utc.with_ymd_and_hms(2026, 7, 14, 18, 0, 0).unwrap(),
    }
}

// test_default_cache_path_sits_beside_windows_settings: the Windows branch is chosen at compile
// time here (`cfg!(windows)`), so the Windows assertion runs on the Windows CI leg and the
// POSIX one (XDG_CONFIG_HOME) everywhere else.
#[test]
fn default_cache_path_sits_beside_the_settings_file() {
    let dir = tempfile::tempdir().unwrap();
    let key = if cfg!(windows) {
        "APPDATA"
    } else {
        "XDG_CONFIG_HOME"
    };
    let environ = BTreeMap::from([(key.to_string(), dir.path().display().to_string())]);
    assert_eq!(
        default_plan_cache_path(&environ),
        dir.path()
            .join("StagePilot")
            .join("last-known-good-service.json")
    );
}

#[test]
fn the_environment_override_wins() {
    let environ = BTreeMap::from([(
        "STAGEPILOT_PLAN_CACHE_PATH".to_string(),
        "/somewhere/cache.json".to_string(),
    )]);
    assert_eq!(
        default_plan_cache_path(&environ),
        std::path::PathBuf::from("/somewhere/cache.json")
    );
}

#[test]
fn cached_service_survives_a_new_store_instance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("last-known-good-service.json");
    FilePlanCacheStore::new(&path).save(&cached_plan()).unwrap();
    let restored = FilePlanCacheStore::new(&path).load().unwrap().unwrap();
    assert_eq!(restored, cached_plan());
    assert_eq!(restored.plan.songs[0].id, "item-1");
    assert_eq!(
        restored.plan.songs[0].source_song_id.as_deref(),
        Some("song-1")
    );
}

#[test]
fn corrupt_cache_fails_safely_without_being_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("last-known-good-service.json");
    std::fs::write(&path, "{not-json").unwrap();
    let error = FilePlanCacheStore::new(&path).load().unwrap_err();
    assert!(error.0.contains("corrupt or invalid"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not-json");
}

// test_atomic_cache_failure_preserves_previous_data: Python patches `os.replace`; here the final
// rename is made to fail by putting a non-empty directory at the target. The save must report the
// atomic-write error and leave no temporary file behind.
#[test]
fn atomic_cache_failure_reports_and_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("last-known-good-service.json");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), "x").unwrap();
    let error = FilePlanCacheStore::new(&path)
        .save(&cached_plan())
        .unwrap_err();
    assert!(error.0.contains("atomically"));
    assert!(path.join("keep").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}
