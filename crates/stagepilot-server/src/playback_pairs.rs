//! `playback-pairs.json`: the plan songs a person paired with a Playback song of another name, by hand.
//!
//! Beside the settings file (like `presenter-follow-bindings.json`), written as `service-pin.json` is (a temporary file,
//! fsync, rename). Only a person's confirmation (`PUT /api/v1/playback-library/pairs`) ever adds an entry; nothing in
//! StagePilot guesses one, and the file is never written on an ordinary read.
//!
//! * The key of an entry is the plan song's normalised title (`stagepilot_playback::library::normalise_title`) and, when
//!   Planning Center names the song (its song number), that number. The same song in a later week has the same key, so its
//!   pair is still there while both songs exist.
//! * The value is the Playback song's MultiTracks number (`content_id`) and the title Playback had when it was confirmed.
//! * A missing, unreadable or damaged file is no pairs (one warning in the log); the next confirmation writes it again.
//! * It holds titles, song numbers and nothing secret. At most [`MAX_PAIRS`] entries are kept (the oldest go first).

use serde::{Deserialize, Serialize};
use stagepilot_core::settings::default_settings_path;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Most pairs kept; the one confirmed longest ago goes first.
pub const MAX_PAIRS: usize = 500;
const MAX_TEXT: usize = 200;

/// `STAGEPILOT_PLAYBACK_PAIRS_PATH`, else `playback-pairs.json` beside the settings file.
pub fn default_pairs_path(environ: &BTreeMap<String, String>) -> PathBuf {
    if let Some(value) = environ
        .get("STAGEPILOT_PLAYBACK_PAIRS_PATH")
        .filter(|v| !v.is_empty())
    {
        return PathBuf::from(value);
    }
    default_settings_path(environ).with_file_name("playback-pairs.json")
}

/// One confirmed pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPair {
    /// The plan song's normalised title.
    pub title_key: String,
    /// Planning Center's number for the song, when the plan has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_center_id: Option<String>,
    /// The Playback song's MultiTracks number.
    pub content_id: u64,
    /// The Playback song's title when the pair was confirmed (for people reading the file; the number decides).
    pub playback_title: String,
}

impl StoredPair {
    fn is_for(&self, title_key: &str, planning_center_id: Option<&str>) -> bool {
        self.title_key == title_key && self.planning_center_id.as_deref() == planning_center_id
    }
}

#[derive(Serialize, Deserialize)]
struct FileBody {
    schema_version: u8,
    pairs: Vec<StoredPair>,
}

/// The pairs, loaded once from their file (when there is one) and saved whenever a person confirms or undoes one.
pub struct PairStore {
    path: Option<PathBuf>,
    pairs: Vec<StoredPair>,
    loaded: bool,
    warned: bool,
}

impl PairStore {
    /// Held in memory only (tests, demo): nothing is read or written.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            pairs: Vec::new(),
            loaded: true,
            warned: false,
        }
    }

    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            pairs: Vec::new(),
            loaded: false,
            warned: false,
        }
    }

    fn load_once(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(path) = self.path.clone() else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<FileBody>(&text) {
                Ok(body) => {
                    self.pairs = body
                        .pairs
                        .into_iter()
                        .filter(|p| !p.title_key.is_empty())
                        .map(clean)
                        .take(MAX_PAIRS)
                        .collect();
                }
                Err(_) => self.warn("the file is damaged and is ignored"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => self.warn("the file cannot be read and is ignored"),
        }
    }

    fn warn(&mut self, what: &str) {
        if !self.warned {
            self.warned = true;
            tracing::warn!(reason = what, "playback_pairs_problem");
        }
    }

    /// The pair confirmed for this plan song, if any.
    pub fn find(
        &mut self,
        title_key: &str,
        planning_center_id: Option<&str>,
    ) -> Option<StoredPair> {
        self.load_once();
        self.pairs
            .iter()
            .find(|p| p.is_for(title_key, planning_center_id))
            .cloned()
    }

    pub fn len(&mut self) -> usize {
        self.load_once();
        self.pairs.len()
    }

    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    /// Keeps `pair` (replacing an earlier one for the same plan song). Nothing changes when it cannot be written.
    pub fn confirm(&mut self, pair: StoredPair) -> Result<(), String> {
        self.load_once();
        let pair = clean(pair);
        let mut next: Vec<StoredPair> = self
            .pairs
            .iter()
            .filter(|p| !p.is_for(&pair.title_key, pair.planning_center_id.as_deref()))
            .cloned()
            .collect();
        next.push(pair);
        if next.len() > MAX_PAIRS {
            next.drain(..next.len() - MAX_PAIRS);
        }
        self.save(&next)?;
        self.pairs = next;
        Ok(())
    }

    /// Forgets the pair of this plan song. `Ok(false)` when there was none.
    pub fn undo(
        &mut self,
        title_key: &str,
        planning_center_id: Option<&str>,
    ) -> Result<bool, String> {
        self.load_once();
        let next: Vec<StoredPair> = self
            .pairs
            .iter()
            .filter(|p| !p.is_for(title_key, planning_center_id))
            .cloned()
            .collect();
        if next.len() == self.pairs.len() {
            return Ok(false);
        }
        self.save(&next)?;
        self.pairs = next;
        Ok(true)
    }

    fn save(&self, pairs: &[StoredPair]) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let body = FileBody {
            schema_version: 1,
            pairs: pairs.to_vec(),
        };
        let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|_| {
            "The pair could not be saved: its folder could not be made.".to_string()
        })?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        let write = || -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)
        };
        write().map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
            "The pair could not be saved: the file could not be written.".to_string()
        })
    }
}

fn clean(mut pair: StoredPair) -> StoredPair {
    pair.title_key = pair.title_key.chars().take(MAX_TEXT).collect();
    pair.playback_title = pair.playback_title.chars().take(MAX_TEXT).collect();
    pair.planning_center_id = pair
        .planning_center_id
        .map(|id| id.chars().take(MAX_TEXT).collect::<String>())
        .filter(|id| !id.is_empty());
    pair
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(title: &str, pco: Option<&str>, content_id: u64) -> StoredPair {
        StoredPair {
            title_key: title.into(),
            planning_center_id: pco.map(str::to_string),
            content_id,
            playback_title: "Seen".into(),
        }
    }

    #[test]
    fn a_pair_survives_a_restart_and_an_undo_is_kept_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-pairs.json");
        let mut first = PairStore::file(&path);
        assert!(
            first.is_empty() && !path.exists(),
            "nothing is written by reading"
        );
        first
            .confirm(pair("oceans", Some("77"), 5_000_001))
            .unwrap();
        first.confirm(pair("holy", None, 5_000_002)).unwrap();
        let mut again = PairStore::file(&path);
        assert_eq!(again.len(), 2);
        assert_eq!(
            again.find("oceans", Some("77")).unwrap().content_id,
            5_000_001
        );
        assert!(
            again.find("oceans", None).is_none(),
            "another key is another song"
        );
        assert!(again.find("oceans", Some("78")).is_none());
        assert_eq!(again.undo("oceans", Some("77")), Ok(true));
        assert_eq!(again.undo("oceans", Some("77")), Ok(false));
        let mut third = PairStore::file(&path);
        assert!(third.find("oceans", Some("77")).is_none());
        assert_eq!(third.len(), 1);
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["playback-pairs.json".to_string()],
            "no temporary file is left"
        );
    }

    #[test]
    fn confirming_again_replaces_the_pair_of_the_same_song() {
        let mut store = PairStore::in_memory();
        store.confirm(pair("oceans", None, 1)).unwrap();
        store.confirm(pair("oceans", None, 2)).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.find("oceans", None).unwrap().content_id, 2);
    }

    #[test]
    fn a_missing_or_damaged_file_is_no_pairs_and_the_next_confirmation_repairs_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-pairs.json");
        assert!(PairStore::file(&path).is_empty());
        for bad in [
            "",
            "not json",
            "{\"schema_version\":1}",
            "[1,2]",
            "{\"pairs\":[{}]}",
        ] {
            std::fs::write(&path, bad).unwrap();
            let mut store = PairStore::file(&path);
            assert!(store.is_empty(), "{bad:?}");
            store.confirm(pair("oceans", None, 9)).unwrap();
            assert_eq!(PairStore::file(&path).len(), 1, "{bad:?}");
        }
    }

    #[test]
    fn a_file_that_cannot_be_written_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be: the rename fails.
        let path = dir.path().join("playback-pairs.json");
        std::fs::create_dir(&path).unwrap();
        let mut store = PairStore::file(&path);
        assert!(store.confirm(pair("oceans", None, 1)).is_err());
        assert!(store.is_empty(), "a pair that was not saved is not kept");
    }

    #[test]
    fn at_most_the_newest_pairs_are_kept() {
        let mut store = PairStore::in_memory();
        for n in 0..(MAX_PAIRS as u64 + 3) {
            store.confirm(pair(&format!("song {n}"), None, n)).unwrap();
        }
        assert_eq!(store.len(), MAX_PAIRS);
        assert!(store.find("song 0", None).is_none());
        assert!(store
            .find(&format!("song {}", MAX_PAIRS + 2), None)
            .is_some());
    }

    #[test]
    fn the_file_holds_no_secret_and_only_titles_and_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-pairs.json");
        let mut store = PairStore::file(&path);
        store.confirm(pair("oceans", Some("77"), 5)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["pairs"][0]["content_id"], 5);
        assert_eq!(value["pairs"][0]["planning_center_id"], "77");
        assert_eq!(value["schema_version"], 1);
    }
}
