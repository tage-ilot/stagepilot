//! Keeping the two helpers (the Playback "Library Link" and the Lightkey helper) up to date: the words and shapes the window sees, and
//! the bundle StagePilot sends to a helper that is older than the one it carries.
//!
//! A helper installs an update only when the StagePilot project signed it (Ed25519 over the exact bytes of `SHA256SUMS`, the signature in
//! `SHA256SUMS.sig`). The signature check itself, and everything that decides when to send, lives in `stagepilot-server`; this file only
//! holds what both helper crates and the server share.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

/// The project's helper-signing public key: the raw 32 bytes of an Ed25519 public key, base64. The same constant is in both helpers
/// (`UPDATE_PUBLIC_KEY`) and in `scripts/helper_signing.mjs`; a test keeps the four equal. The private half is on the release machine only.
pub const HELPER_SIGNING_PUBLIC_KEY: &str = "Evky97ydM5zWJ02p6Wc7RguEHMglX3WRxiPrege4Op8=";

/// The 32 bytes of a public key given as base64, when it is exactly that.
pub fn public_key_bytes(text: &str) -> Option<[u8; 32]> {
    let bytes = STANDARD.decode(text.trim().as_bytes()).ok()?;
    <[u8; 32]>::try_from(bytes).ok()
}

/// What StagePilot is doing about a helper that is older than the one it carries. Left out of the status while there is nothing to do or say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HelperUpdateMode {
    /// The helper can update itself and StagePilot will do it by itself, when nothing is playing.
    Automatic,
    /// The update is being installed right now.
    Updating,
    /// The update was installed a moment ago (said once).
    Updated,
    /// Someone has to run the setup command on the helper's computer: the helper is too old to update itself, the bundle is not signed, the
    /// automatic update is switched off in the settings file, or it failed three times.
    Manual,
}

/// The state of the automatic update of one helper (`helper.update` in the status the window reads).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HelperUpdateStatus {
    pub mode: HelperUpdateMode,
    /// The version StagePilot carries: what the helper is becoming, or just became.
    pub version: String,
    /// One plain sentence for the operator, when there is something to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub message: Option<String>,
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

/// `true` when `running` is a version below `bundled`. A running version that is not `X.Y.Z` is not trusted: it counts as older.
pub fn version_is_older(running: Option<&str>, bundled: &str) -> bool {
    let Some(bundled) = parse_version(bundled) else {
        return false;
    };
    running
        .and_then(parse_version)
        .is_none_or(|found| found < bundled)
}

/// The files of one helper's release that StagePilot carries, and the proof that the project signed them.
#[derive(Debug, Clone)]
pub struct UpdateBundle {
    /// The version of the files (what the helper becomes).
    pub version: String,
    /// The files that go to the helper: the name it expects and the bytes.
    pub files: Vec<(&'static str, &'static [u8])>,
    /// The exact bytes of `SHA256SUMS`, which is what the signature covers.
    pub sums: &'static [u8],
    /// `SHA256SUMS.sig`: base64 of the 64-byte Ed25519 signature (empty while the bundle is not signed).
    pub signature: &'static [u8],
}

impl UpdateBundle {
    /// The signature text, trimmed. Empty: not signed.
    pub fn signature_text(&self) -> String {
        String::from_utf8_lossy(self.signature).trim().to_string()
    }

    /// The raw 64 signature bytes, when the text is base64 of exactly that many.
    pub fn signature_bytes(&self) -> Option<Vec<u8>> {
        let text = self.signature_text();
        let bytes = STANDARD.decode(text.as_bytes()).ok()?;
        (bytes.len() == 64 && STANDARD.encode(&bytes) == text).then_some(bytes)
    }

    /// The JSON body of `POST /update`: `{"files": {name: base64}, "sums": "<SHA256SUMS text>", "sig": "<base64>"}`. `None` when
    /// `SHA256SUMS` is not text (it always is).
    pub fn request_body(&self) -> Option<Vec<u8>> {
        let sums = std::str::from_utf8(self.sums).ok()?;
        let mut files = Map::new();
        for (name, bytes) in &self.files {
            files.insert((*name).to_string(), Value::String(STANDARD.encode(bytes)));
        }
        serde_json::to_vec(&json!({
            "files": files,
            "sums": sums,
            "sig": self.signature_text(),
        }))
        .ok()
    }

    /// The names and SHA-256 digests (lower-case hex) that `SHA256SUMS` lists.
    pub fn listed(&self) -> Vec<(String, String)> {
        String::from_utf8_lossy(self.sums)
            .lines()
            .filter_map(|line| {
                let (hash, name) = line.trim().split_once(char::is_whitespace)?;
                let name = name.trim_start_matches([' ', '*']);
                (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                    .then(|| (hash.to_ascii_lowercase(), name.to_string()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers_and_a_bad_one_counts_as_older() {
        assert_eq!(parse_version("0.4.0"), Some([0, 4, 0]));
        assert!(version_is_older(Some("0.3.0"), "0.4.0"));
        assert!(version_is_older(Some("0.9.9"), "0.10.0"));
        assert!(!version_is_older(Some("0.4.0"), "0.4.0"));
        assert!(!version_is_older(Some("0.10.0"), "0.9.0"));
        assert!(!version_is_older(Some("1.0.0"), "0.4.0"));
        for bad in ["", "0.4", "v0.4.0", "0.4.0.1", "x"] {
            assert!(version_is_older(Some(bad), "0.4.0"), "{bad:?}");
        }
        assert!(version_is_older(None, "0.4.0"));
        assert!(!version_is_older(Some("0.1.0"), "garbage"));
    }

    #[test]
    fn the_pinned_public_key_is_32_bytes() {
        assert_eq!(
            public_key_bytes(HELPER_SIGNING_PUBLIC_KEY).map(|k| k.len()),
            Some(32)
        );
        assert_eq!(public_key_bytes("AAAA"), None);
        assert_eq!(public_key_bytes(""), None);
    }

    #[test]
    fn the_request_body_carries_every_file_the_sums_and_the_signature() {
        let bundle = UpdateBundle {
            version: "0.4.0".into(),
            files: vec![("playback_library.py", b"print(1)\n".as_slice())],
            sums: b"abc  playback_library.py\n",
            signature: b"AAAA\n",
        };
        let body: Value = serde_json::from_slice(&bundle.request_body().unwrap()).unwrap();
        assert_eq!(body["files"]["playback_library.py"], json!("cHJpbnQoMSkK"));
        assert_eq!(body["sums"], json!("abc  playback_library.py\n"));
        assert_eq!(body["sig"], json!("AAAA"));
        assert_eq!(body.as_object().unwrap().len(), 3);
    }

    #[test]
    fn a_signature_is_64_bytes_of_canonical_base64_or_nothing() {
        let sig = |text: &'static [u8]| UpdateBundle {
            version: "1.0.0".into(),
            files: vec![],
            sums: b"",
            signature: text,
        };
        let good = STANDARD.encode([7u8; 64]);
        let text: &'static [u8] = Box::leak(format!("{good}\n").into_bytes().into_boxed_slice());
        assert_eq!(sig(text).signature_bytes(), Some(vec![7u8; 64]));
        assert_eq!(sig(b"").signature_bytes(), None);
        assert_eq!(sig(b"\n").signature_bytes(), None);
        assert_eq!(sig(b"AAAA").signature_bytes(), None);
        let long: &'static [u8] =
            Box::leak(STANDARD.encode([7u8; 65]).into_bytes().into_boxed_slice());
        assert_eq!(sig(long).signature_bytes(), None);
    }
}
