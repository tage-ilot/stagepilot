//! The ProPresenter Link helper files that ship inside this binary, so the "Set up the ProPresenter computer" box can hand a
//! version-matched installer to another computer without internet. A copy of how the Library Link ships its helper
//! (`stagepilot-playback::helper_assets`): the helper is its own program with its own code, bundled here.
//!
//! Files (in `assets/propresenter-link/`): `propresenter_link.py`, `install.sh` and the `SHA256SUMS` that lists them
//! (`node scripts/sign_helpers.mjs --sums-only` writes it). `SHA256SUMS.sig` is the project's Ed25519 signature of
//! `SHA256SUMS`, written on the release machine by `node scripts/sign_helpers.mjs --key <pem>`; it is empty in a working tree,
//! and nothing is sent to a helper then. The screen scripts the helper runs are copies of `assets/gui/*` inside
//! `propresenter_link.py`; a test keeps them equal.

use stagepilot_core::helper_update::UpdateBundle;

/// The files the install box serves, and nothing else.
pub const FILE_NAMES: [&str; 3] = ["install.sh", "propresenter_link.py", "SHA256SUMS"];

/// The helper version bundled here. A test keeps it equal to `__version__` in `propresenter_link.py`.
pub const BUNDLED_HELPER_VERSION: &str = "0.1.0";

const INSTALL_SH: &[u8] = include_bytes!("../assets/propresenter-link/install.sh");
const PROPRESENTER_LINK_PY: &[u8] =
    include_bytes!("../assets/propresenter-link/propresenter_link.py");
const SHA256SUMS: &[u8] = include_bytes!("../assets/propresenter-link/SHA256SUMS");
const SHA256SUMS_SIG: &[u8] = include_bytes!("../assets/propresenter-link/SHA256SUMS.sig");

/// The one file an update replaces on the ProPresenter computer.
pub const UPDATE_FILE_NAMES: [&str; 1] = ["propresenter_link.py"];

/// What StagePilot sends to an older helper that can update itself.
pub fn update_bundle() -> UpdateBundle {
    UpdateBundle {
        version: BUNDLED_HELPER_VERSION.to_string(),
        files: vec![("propresenter_link.py", PROPRESENTER_LINK_PY)],
        sums: SHA256SUMS,
        signature: SHA256SUMS_SIG,
    }
}

/// The bytes of a served file, by exact name.
pub fn file(name: &str) -> Option<&'static [u8]> {
    match name {
        "install.sh" => Some(INSTALL_SH),
        "propresenter_link.py" => Some(PROPRESENTER_LINK_PY),
        "SHA256SUMS" => Some(SHA256SUMS),
        _ => None,
    }
}

/// `Content-Type` of a served file.
pub fn content_type(name: &str) -> &'static str {
    match name {
        "propresenter_link.py" => "text/x-python; charset=utf-8",
        "install.sh" => "text/x-shellscript; charset=utf-8",
        _ => "text/plain; charset=utf-8",
    }
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

/// `X.Y.Z` as three numbers, when `text` is exactly that.
pub fn parse_version(text: &str) -> Option<[u64; 3]> {
    let mut parts = text.trim().split('.');
    let mut out = [0u64; 3];
    for slot in &mut out {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// `true` when `version` is older than the bundled one. A version that is not `X.Y.Z` is not trusted: it counts as older.
pub fn is_older_than_bundled(version: Option<&str>) -> bool {
    let Some(bundled) = parse_version(BUNDLED_HELPER_VERSION) else {
        return false;
    };
    match version.and_then(parse_version) {
        Some(found) => found < bundled,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha256(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    #[test]
    fn every_bundled_file_matches_the_bundled_checksum_list() {
        let sums = checksums();
        for name in FILE_NAMES.iter().filter(|n| **n != "SHA256SUMS") {
            let want = sums
                .iter()
                .find(|(_, n)| n == name)
                .unwrap_or_else(|| panic!("{name} is not in SHA256SUMS"));
            assert_eq!(sha256(file(name).unwrap()), want.0, "{name}");
        }
        let mut names: Vec<&str> = sums.iter().map(|(_, n)| n.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["install.sh", "propresenter_link.py"]);
    }

    #[test]
    fn the_update_bundle_is_the_script_and_the_list_that_covers_it() {
        let bundle = update_bundle();
        assert_eq!(bundle.version, BUNDLED_HELPER_VERSION);
        let names: Vec<&str> = bundle.files.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, UPDATE_FILE_NAMES);
        let listed = bundle.listed();
        for (name, bytes) in &bundle.files {
            let want = listed.iter().find(|(_, n)| n == name).expect("listed");
            assert_eq!(sha256(bytes), want.0, "{name}");
        }
        assert_eq!(bundle.sums, SHA256SUMS);
    }

    #[test]
    fn the_version_constant_is_the_helpers_own_version() {
        let source = String::from_utf8_lossy(file("propresenter_link.py").unwrap()).to_string();
        let own = source
            .lines()
            .find_map(|line| {
                line.strip_prefix("__version__ = \"")
                    .and_then(|rest| rest.strip_suffix('"'))
            })
            .expect("propresenter_link.py has a __version__ line");
        assert_eq!(own, BUNDLED_HELPER_VERSION);
        assert!(parse_version(BUNDLED_HELPER_VERSION).is_some());
    }

    #[test]
    fn the_installer_takes_a_key_file_and_a_client_and_runs_offline() {
        let installer = String::from_utf8_lossy(file("install.sh").unwrap()).to_string();
        assert!(installer.contains("--key-file"));
        assert!(installer.contains("--allow-for"));
        assert!(installer.contains("SHA256SUMS"));
        assert!(!installer.contains("curl -fsSL"), "nothing is downloaded");
    }

    #[test]
    fn the_screen_scripts_inside_the_helper_are_the_bundled_files() {
        let helper = String::from_utf8_lossy(file("propresenter_link.py").unwrap()).to_string();
        for (variable, text) in [
            (
                "CHECK_ACCESS_APPLESCRIPT",
                crate::link::screen::CHECK_ACCESS,
            ),
            ("IMPORT_SONG_APPLESCRIPT", crate::link::screen::IMPORT_SONG),
            (
                "SELECT_LIBRARY_APPLESCRIPT",
                crate::link::screen::SELECT_LIBRARY,
            ),
            ("CLICK_SWIFT", crate::link::screen::CLICK_SWIFT),
        ] {
            assert!(
                helper.contains(&format!("{variable} = r'''{text}'''\n")),
                "{variable} differs from its file in assets/gui"
            );
        }
    }

    #[test]
    fn only_the_three_files_exist() {
        for name in FILE_NAMES {
            assert!(file(name).is_some(), "{name}");
        }
        for name in [
            "uninstall.sh",
            "config.json",
            "../install.sh",
            "",
            "INSTALL.SH",
        ] {
            assert!(file(name).is_none(), "{name:?}");
        }
    }

    #[test]
    fn versions_are_compared_as_numbers() {
        assert_eq!(parse_version("0.3.0"), Some([0, 3, 0]));
        for bad in ["", "0.3", "0.3.0.1", "v0.3.0", "0.x.1", "0..1", "-1.0.0"] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
        assert!(!is_older_than_bundled(Some(BUNDLED_HELPER_VERSION)));
        assert!(!is_older_than_bundled(Some("0.10.0")));
        assert!(is_older_than_bundled(Some("0.0.9")));
        assert!(is_older_than_bundled(None));
        assert!(is_older_than_bundled(Some("garbage")));
    }
}
