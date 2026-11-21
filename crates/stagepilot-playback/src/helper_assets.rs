//! The Playback helper (`huntrw6/playback-library`, the "Library Link") files that ship inside this
//! binary, so the "Set up the Playback computer" box can hand a version-matched installer to the
//! Playback Mac without internet. They are the three files of a release that the installer needs:
//! `playback_library.py`, `install.sh` and the `SHA256SUMS` that lists them (`uninstall.sh` is not
//! bundled). Refresh them from a release of the helper: copy `playback_library.py` and `install.sh`
//! into `assets/playback-helper/`, set [`BUNDLED_HELPER_VERSION`] to the helper's `__version__`,
//! run `node scripts/sign_helpers.mjs --sums-only` and the tests in this file.
//!
//! The same files, signed, are what StagePilot sends to a helper that is older (`update_bundle`):
//! `SHA256SUMS.sig` is the project's Ed25519 signature of `SHA256SUMS`, written on the release machine
//! by `node scripts/sign_helpers.mjs --key <pem>` (it is empty in a working tree: nothing is sent then).

use stagepilot_core::helper_update::UpdateBundle;

/// The three files the install box serves, and nothing else. `install.sh` runs offline when
/// `playback_library.py` and `SHA256SUMS` sit next to it.
pub const FILE_NAMES: [&str; 3] = ["install.sh", "playback_library.py", "SHA256SUMS"];

/// The helper version bundled here. A test keeps it equal to `__version__` in `playback_library.py`.
pub const BUNDLED_HELPER_VERSION: &str = "0.4.1";

const INSTALL_SH: &[u8] = include_bytes!("../assets/playback-helper/install.sh");
const PLAYBACK_LIBRARY_PY: &[u8] = include_bytes!("../assets/playback-helper/playback_library.py");
const SHA256SUMS: &[u8] = include_bytes!("../assets/playback-helper/SHA256SUMS");
const SHA256SUMS_SIG: &[u8] = include_bytes!("../assets/playback-helper/SHA256SUMS.sig");

/// The one file an update replaces on the Playback computer (the installer and the wrapper are not updated this way).
pub const UPDATE_FILE_NAMES: [&str; 1] = ["playback_library.py"];

/// What StagePilot sends to an older helper that can update itself: the script, `SHA256SUMS` and the project's signature of it.
pub fn update_bundle() -> UpdateBundle {
    UpdateBundle {
        version: BUNDLED_HELPER_VERSION.to_string(),
        files: vec![("playback_library.py", PLAYBACK_LIBRARY_PY)],
        sums: SHA256SUMS,
        signature: SHA256SUMS_SIG,
    }
}

/// The bytes of a served file, by exact name.
pub fn file(name: &str) -> Option<&'static [u8]> {
    match name {
        "install.sh" => Some(INSTALL_SH),
        "playback_library.py" => Some(PLAYBACK_LIBRARY_PY),
        "SHA256SUMS" => Some(SHA256SUMS),
        _ => None,
    }
}

/// `Content-Type` of a served file.
pub fn content_type(name: &str) -> &'static str {
    match name {
        "playback_library.py" => "text/x-python; charset=utf-8",
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

/// `true` when `version` is a version older than the bundled one. A version that is not `X.Y.Z`
/// is not trusted: it counts as older (the install box then offers the update).
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
        // The list names the files of the folder and nothing else (`node scripts/sign_helpers.mjs --sums-only` writes it).
        let mut names: Vec<&str> = sums.iter().map(|(_, n)| n.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["install.sh", "playback_library.py"]);
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
        assert_eq!(
            bundle.sums, SHA256SUMS,
            "the exact bytes the signature covers"
        );
    }

    #[test]
    fn the_version_constant_is_the_helpers_own_version() {
        let source = String::from_utf8_lossy(file("playback_library.py").unwrap()).to_string();
        let own = source
            .lines()
            .find_map(|line| {
                line.strip_prefix("__version__ = \"")
                    .and_then(|rest| rest.strip_suffix('"'))
            })
            .expect("playback_library.py has a __version__ line");
        assert_eq!(own, BUNDLED_HELPER_VERSION);
        assert!(parse_version(BUNDLED_HELPER_VERSION).is_some());
    }

    #[test]
    fn the_installer_takes_a_key_file_and_runs_offline() {
        let installer = String::from_utf8_lossy(file("install.sh").unwrap()).to_string();
        assert!(
            installer.contains("--key-file"),
            "the wrapper passes the key by file"
        );
        assert!(
            installer.contains("SHA256SUMS"),
            "the installer checks the sums itself"
        );
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
        assert_eq!(parse_version(" 1.10.2 "), Some([1, 10, 2]));
        for bad in ["", "0.3", "0.3.0.1", "v0.3.0", "0.x.1", "0..1", "-1.0.0"] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
        assert!(is_older_than_bundled(Some("0.2.0")));
        assert!(is_older_than_bundled(Some("0.3.9")));
        assert!(!is_older_than_bundled(Some(BUNDLED_HELPER_VERSION)));
        assert!(
            !is_older_than_bundled(Some("0.10.0")),
            "0.10.0 is newer than 0.3.0"
        );
        assert!(!is_older_than_bundled(Some("1.0.0")));
        assert!(
            is_older_than_bundled(None),
            "no version at all is not trusted"
        );
        assert!(is_older_than_bundled(Some("garbage")));
    }
}
