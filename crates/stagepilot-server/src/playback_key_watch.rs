//! `playback-keys.json`: the keys StagePilot last saw for the songs of Playback's setlists, so it can say when a key
//! was changed (`stagepilot_playback::key_watch` is the table; this is where it lives on disk).
//!
//! Written the way `service-pin.json` is (temp file, fsync, rename), beside the settings file. A missing or damaged
//! file is ignored (once in the log), and a file that cannot be written is a log line and a try again at the next
//! change: it never fails a request. The file is written only when a song was seen for the first time or its key
//! changed, never on an ordinary read. Only a song's Playback number, key names and times are in it.

use chrono::{DateTime, Utc};
use stagepilot_core::settings::default_settings_path;
use stagepilot_playback::key_watch::KeyWatch;
use stagepilot_playback::library::LibrarySong;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `STAGEPILOT_PLAYBACK_KEYS_PATH`, else `playback-keys.json` beside the settings file.
pub fn default_key_watch_path(environ: &BTreeMap<String, String>) -> PathBuf {
    if let Some(value) = environ
        .get("STAGEPILOT_PLAYBACK_KEYS_PATH")
        .filter(|v| !v.is_empty())
    {
        return PathBuf::from(value);
    }
    default_settings_path(environ).with_file_name("playback-keys.json")
}

/// The table, loaded once from its file (when it has one) and saved when it changes.
pub struct KeyWatchStore {
    path: Option<PathBuf>,
    watch: KeyWatch,
    loaded: bool,
    /// A change is not on disk yet (the last write failed).
    unsaved: bool,
    warned: bool,
}

impl KeyWatchStore {
    /// Held in memory only (tests, demo): nothing is read or written.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            watch: KeyWatch::default(),
            loaded: true,
            unsaved: false,
            warned: false,
        }
    }

    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            watch: KeyWatch::default(),
            loaded: false,
            unsaved: false,
            warned: false,
        }
    }

    fn warn(&mut self, what: &str) {
        if !self.warned {
            self.warned = true;
            tracing::warn!(reason = what, "playback_key_memory_problem");
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
            Ok(text) => match KeyWatch::from_json(&text) {
                Ok(watch) => self.watch = watch,
                Err(_) => self.warn("the file is damaged and is ignored"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => self.warn("the file cannot be read and is ignored"),
        }
    }

    /// Notes the keys of a setlist just read and puts the recorded changes on its songs. Saves only when something
    /// new was learned.
    pub fn observe(&mut self, songs: &mut [LibrarySong], now: DateTime<Utc>) {
        self.load_once();
        if self.watch.observe(songs, now) {
            self.unsaved = true;
        }
        if self.unsaved && self.path.is_some() {
            match self.save() {
                Ok(()) => self.unsaved = false,
                Err(()) => self.warn("the file could not be written"),
            }
        }
    }

    /// Forgets everything, in memory and on disk (another Playback computer, or the Library Link was forgotten).
    pub fn clear(&mut self) {
        self.watch = KeyWatch::default();
        self.loaded = true;
        self.unsaved = false;
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn len(&self) -> usize {
        self.watch.len()
    }

    pub fn is_empty(&self) -> bool {
        self.watch.is_empty()
    }

    fn save(&self) -> Result<(), ()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|_| ())?;
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
            file.write_all(self.watch.to_json().as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)
        };
        write().map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use serde_json::json;
    use stagepilot_playback::library::parse_setlist;

    fn at(hours: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap() + Duration::hours(hours)
    }

    fn songs(keys: &[(i64, &str)]) -> Vec<LibrarySong> {
        let items: Vec<_> = keys
            .iter()
            .map(|(id, key)| json!({"setlist_song_id": id, "title": format!("Song {id}"), "key": {"name": key}}))
            .collect();
        parse_setlist(&json!({"items": items})).unwrap().songs
    }

    #[test]
    fn a_change_is_kept_across_a_restart_and_the_file_holds_no_title() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-keys.json");
        let mut first = KeyWatchStore::file(&path);
        first.observe(&mut songs(&[(11, "G")]), at(0));
        first.observe(&mut songs(&[(11, "Ab")]), at(1));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("Song 11"));
        let mut restarted = KeyWatchStore::file(&path);
        let mut read = songs(&[(11, "Ab")]);
        restarted.observe(&mut read, at(2));
        assert_eq!(read[0].key_was.as_deref(), Some("G"));
        assert_eq!(
            read[0].key_changed_at.as_deref(),
            Some("2026-11-20T01:00:00Z")
        );
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["playback-keys.json".to_string()]);
    }

    #[test]
    fn nothing_is_written_when_nothing_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-keys.json");
        let mut store = KeyWatchStore::file(&path);
        assert!(!path.exists(), "an empty table is not written");
        store.observe(&mut songs(&[(11, "G")]), at(0));
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        for hour in 1..5 {
            store.observe(&mut songs(&[(11, "G")]), at(hour));
        }
        assert!(
            !path.exists(),
            "the same keys again are not a reason to write"
        );
        store.observe(&mut songs(&[(11, "A")]), at(6));
        assert!(path.exists());
    }

    #[test]
    fn a_damaged_file_is_ignored_and_replaced_at_the_next_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-keys.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        let mut store = KeyWatchStore::file(&path);
        let mut read = songs(&[(11, "G")]);
        store.observe(&mut read, at(0));
        assert_eq!(read[0].key_was, None);
        assert!(KeyWatch::from_json(&std::fs::read_to_string(&path).unwrap()).is_ok());
    }

    #[test]
    fn a_file_that_cannot_be_written_never_fails_the_read() {
        let dir = tempfile::tempdir().unwrap();
        // The parent is a file, so the directory cannot be made.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let mut store = KeyWatchStore::file(blocker.join("playback-keys.json"));
        store.observe(&mut songs(&[(11, "G")]), at(0));
        let mut read = songs(&[(11, "Ab")]);
        store.observe(&mut read, at(1));
        assert_eq!(
            read[0].key_was.as_deref(),
            Some("G"),
            "the change is still known in memory"
        );
    }

    #[test]
    fn clear_forgets_in_memory_and_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-keys.json");
        let mut store = KeyWatchStore::file(&path);
        store.observe(&mut songs(&[(11, "G")]), at(0));
        assert_eq!(store.len(), 1);
        store.clear();
        assert!(store.is_empty() && !path.exists());
    }

    #[test]
    fn the_file_sits_beside_the_settings_unless_told_otherwise() {
        let mut env = BTreeMap::new();
        env.insert(
            "STAGEPILOT_SETTINGS_PATH".to_string(),
            "/data/sp/settings.json".to_string(),
        );
        assert_eq!(
            default_key_watch_path(&env),
            PathBuf::from("/data/sp/playback-keys.json")
        );
        env.insert(
            "STAGEPILOT_PLAYBACK_KEYS_PATH".to_string(),
            "/elsewhere/keys.json".to_string(),
        );
        assert_eq!(
            default_key_watch_path(&env),
            PathBuf::from("/elsewhere/keys.json")
        );
    }
}
