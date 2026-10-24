//! M1: the backend reports the real application version, not a hard-coded one.

use stagepilot_core::settings::{RuntimeSettings, APP_VERSION};
use std::path::PathBuf;

fn tauri_conf_version() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../desktop/src-tauri/tauri.conf.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("tauri.conf.json")).unwrap();
    value["version"].as_str().expect("version").to_string()
}

#[test]
fn app_version_is_the_release_version() {
    // An explicit STAGEPILOT_VERSION (packagers, CI) wins over tauri.conf.json at build time.
    let expected = std::env::var("STAGEPILOT_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(tauri_conf_version);
    assert_eq!(APP_VERSION, expected);
    assert_ne!(
        APP_VERSION, "2.41.1",
        "the Python-era version leaked back in"
    );
    assert_ne!(
        APP_VERSION, "0.0.0",
        "CARGO_PKG_VERSION of the workspace is not a version"
    );
}

#[test]
fn runtime_settings_carry_the_app_version() {
    assert_eq!(RuntimeSettings::demo().version, APP_VERSION);
}

fn repo_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// t5: every place the release version lives agrees with the one the backend reports, so a
/// release script that misses a file fails the ordinary test run instead of shipping.
#[test]
fn every_version_source_agrees_with_the_injected_version() {
    let expected = tauri_conf_version();
    let cargo = repo_file("desktop/src-tauri/Cargo.toml");
    let cargo_version = cargo
        .lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|l| l.split('"').next())
        .expect("desktop Cargo.toml version");
    assert_eq!(cargo_version, expected, "desktop/src-tauri/Cargo.toml");
    for file in ["frontend/package.json", "desktop/package.json"] {
        let value: serde_json::Value = serde_json::from_str(&repo_file(file)).unwrap();
        assert_eq!(value["version"].as_str(), Some(expected.as_str()), "{file}");
    }
    if std::env::var("STAGEPILOT_VERSION").map_or(true, |v| v.trim().is_empty()) {
        assert_eq!(APP_VERSION, expected);
    }
}
