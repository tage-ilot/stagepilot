//! The Lightkey helper (`huntrw6/lightkey-api`) files that ship inside this binary, so the
//! "Set up the Lighting computer" box can hand a version-matched installer to the Lighting Mac
//! without internet. Refresh them with `node scripts/vendor-lightkey-helper.mjs <tag>`.

use stagepilot_core::helper_update::UpdateBundle;

/// The five files the install box serves, and nothing else. Order is the order the wrapper
/// downloads them in.
pub const FILE_NAMES: [&str; 5] = [
    "install.sh",
    "lightkey_api.py",
    "lightkey_cues.py",
    "uninstall.sh",
    "SHA256SUMS",
];

const INSTALL_SH: &[u8] = include_bytes!("../assets/lightkey-helper/install.sh");
const LIGHTKEY_API_PY: &[u8] = include_bytes!("../assets/lightkey-helper/lightkey_api.py");
const LIGHTKEY_CUES_PY: &[u8] = include_bytes!("../assets/lightkey-helper/lightkey_cues.py");
const UNINSTALL_SH: &[u8] = include_bytes!("../assets/lightkey-helper/uninstall.sh");
const SHA256SUMS: &[u8] = include_bytes!("../assets/lightkey-helper/SHA256SUMS");
const VERSION: &str = include_str!("../assets/lightkey-helper/VERSION");
const SHA256SUMS_SIG: &[u8] = include_bytes!("../assets/lightkey-helper/SHA256SUMS.sig");

/// The files an update replaces on the Lighting computer. The helper takes exactly these names; it writes the cue reader only where it
/// is installed. The installer, the uninstaller and the wrapper are not updated this way.
pub const UPDATE_FILE_NAMES: [&str; 2] = ["lightkey_api.py", "lightkey_cues.py"];

/// What StagePilot sends to an older helper that can update itself: the two scripts, `SHA256SUMS` and the project's signature of it
/// (`SHA256SUMS.sig`, written on the release machine by `node scripts/sign_helpers.mjs --key <pem>`; empty in a working tree).
pub fn update_bundle() -> UpdateBundle {
    UpdateBundle {
        version: bundled_version().to_string(),
        files: vec![
            ("lightkey_api.py", LIGHTKEY_API_PY),
            ("lightkey_cues.py", LIGHTKEY_CUES_PY),
        ],
        sums: SHA256SUMS,
        signature: SHA256SUMS_SIG,
    }
}

/// The bytes of a served file, by exact name.
pub fn file(name: &str) -> Option<&'static [u8]> {
    match name {
        "install.sh" => Some(INSTALL_SH),
        "lightkey_api.py" => Some(LIGHTKEY_API_PY),
        "lightkey_cues.py" => Some(LIGHTKEY_CUES_PY),
        "uninstall.sh" => Some(UNINSTALL_SH),
        "SHA256SUMS" => Some(SHA256SUMS),
        _ => None,
    }
}

/// `Content-Type` of a served file.
pub fn content_type(name: &str) -> &'static str {
    match name {
        "lightkey_api.py" | "lightkey_cues.py" => "text/x-python; charset=utf-8",
        "install.sh" | "uninstall.sh" => "text/x-shellscript; charset=utf-8",
        _ => "text/plain; charset=utf-8",
    }
}

/// The helper version bundled here (`0.4.3`), from the `VERSION` file the vendor script writes.
pub fn bundled_version() -> &'static str {
    VERSION.trim()
}

/// `true` when `text` is `X.Y.Z`.
pub fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `(sha256 hex, file name)` for each line of the embedded `SHA256SUMS`.
pub fn checksums() -> Vec<(String, String)> {
    String::from_utf8_lossy(SHA256SUMS)
        .lines()
        .filter_map(|line| {
            let (hash, name) = line.trim().split_once(char::is_whitespace)?;
            let name = name.trim_start_matches([' ', '*']);
            (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| (hash.to_ascii_lowercase(), name.to_string()))
        })
        .collect()
}

/// `true` when the bundled `install.sh` understands `--allow-for <ip>` (helper 0.5.0 and later).
pub fn installer_supports_allow_for() -> bool {
    String::from_utf8_lossy(INSTALL_SH).contains("--allow-for")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha256(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    #[test]
    fn every_embedded_file_matches_the_embedded_checksum_list() {
        let sums = checksums();
        for name in FILE_NAMES.iter().filter(|n| **n != "SHA256SUMS") {
            let want = sums
                .iter()
                .find(|(_, n)| n == name)
                .unwrap_or_else(|| panic!("{name} is not in SHA256SUMS"));
            assert_eq!(sha256(file(name).unwrap()), want.0, "{name}");
        }
        assert_eq!(
            sums.len(),
            5,
            "SHA256SUMS lists the four files and VERSION (so the version is signed too)"
        );
        let mut names: Vec<&str> = sums.iter().map(|(_, n)| n.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "VERSION",
                "install.sh",
                "lightkey_api.py",
                "lightkey_cues.py",
                "uninstall.sh"
            ]
        );
        assert_eq!(
            sha256(VERSION.as_bytes()),
            sums.iter().find(|(_, n)| n == "VERSION").unwrap().0
        );
    }

    #[test]
    fn the_update_bundle_is_the_two_scripts_and_the_list_that_covers_them() {
        let bundle = update_bundle();
        assert_eq!(bundle.version, bundled_version());
        let names: Vec<&str> = bundle.files.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, UPDATE_FILE_NAMES);
        let listed = bundle.listed();
        for (name, bytes) in &bundle.files {
            let want = listed.iter().find(|(_, n)| n == name).expect("listed");
            assert_eq!(sha256(bytes), want.0, "{name}");
        }
        assert_eq!(
            bundle.sums, SHA256SUMS,
            "the exact bytes the signature covers"
        );
    }

    #[test]
    fn the_version_file_parses() {
        assert!(is_version(bundled_version()), "{:?}", bundled_version());
        assert!(!is_version("0.4"));
        assert!(!is_version("0.4.x"));
        assert!(!is_version("v0.4.3"));
    }

    #[test]
    fn the_version_matches_the_helper_itself() {
        let source = String::from_utf8_lossy(file("lightkey_api.py").unwrap()).to_string();
        assert!(
            source.contains(&format!("__version__ = \"{}\"", bundled_version())),
            "VERSION and lightkey_api.py disagree"
        );
    }

    #[test]
    fn only_the_five_files_exist() {
        for name in FILE_NAMES {
            assert!(file(name).is_some(), "{name}");
        }
        for name in ["VERSION", "../install.sh", "config.json", "", "install.sh/"] {
            assert!(file(name).is_none(), "{name:?}");
        }
    }
}
