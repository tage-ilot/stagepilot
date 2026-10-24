//! Injects the application version at build time (M1).
//!
//! The workspace crates are all `0.0.0`, so `CARGO_PKG_VERSION` is useless. The single source of
//! truth is the desktop shell's `tauri.conf.json`, which `scripts/set-release-version.mjs` writes
//! on release. `STAGEPILOT_VERSION` in the environment overrides it (CI, packagers); with neither
//! the build falls back to `0.0.0-dev` so a stray build can never claim to be a release.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=STAGEPILOT_VERSION");
    let config = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .join("../../desktop/src-tauri/tauri.conf.json");
    println!("cargo:rerun-if-changed={}", config.display());

    let from_env = std::env::var("STAGEPILOT_VERSION")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let from_config = || -> Option<String> {
        let text = std::fs::read_to_string(&config).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        value.get("version")?.as_str().map(str::to_string)
    };
    let version = from_env
        .or_else(from_config)
        .unwrap_or_else(|| "0.0.0-dev".to_string());
    assert!(
        !version.is_empty()
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')),
        "STAGEPILOT_VERSION {version:?} is not a version string"
    );
    println!("cargo:rustc-env=STAGEPILOT_VERSION={version}");
}
