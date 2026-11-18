//! Which songs of the Playback library had their key changed lately.
//!
//! The helper says a song's `key` and `key_original` but not WHEN the key was changed, and Playback has no change time
//! to read. So StagePilot notices for itself: every time a setlist's songs are read, each song's key is compared with
//! the key StagePilot saw for the same Playback setlist song (`setlist_song_id`) the time before.
//!
//! * Never seen: the key is remembered, nothing is recorded (the history is unknown, so there is no "was").
//! * Same key: nothing happens.
//! * Another key: the old key is recorded as `key_was`, the moment as `key_changed_at`, and the new key is remembered.
//!   A change back to an earlier key is a change like any other.
//!
//! This file is the table and its JSON; it has no clock of its own (the caller passes `now`) and no file access. The
//! server keeps the table in `playback-keys.json` next to the other state files. Only the song's Playback number, key
//! names and times are held here: no title, no plan, no address.

use crate::library::LibrarySong;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Songs remembered at most; the one seen longest ago goes first.
pub const MAX_WATCHED: usize = 500;
/// A song not read for this many days is forgotten when the table is next written.
pub const FORGET_AFTER_DAYS: i64 = 7;

const SCHEMA_VERSION: u32 = 1;
/// A key name is a few characters; the helper's text is cut to 200 elsewhere, this one only guards the file.
const MAX_KEY_CHARS: usize = 40;

/// What is remembered about one song.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    setlist_song_id: i64,
    /// The key last seen.
    key: String,
    /// The last time a read had this song (kept in memory on every read; saved with the next write).
    seen_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_was: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_changed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize)]
struct File {
    schema_version: u32,
    songs: Vec<Entry>,
}

/// Why a saved table was not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyWatch {
    songs: BTreeMap<i64, Entry>,
}

impl KeyWatch {
    /// Reads a saved table. A damaged or unknown one is `Err` (the caller starts with an empty table).
    pub fn from_json(text: &str) -> Result<Self, Unreadable> {
        let file: File = serde_json::from_str(text).map_err(|_| Unreadable)?;
        if file.schema_version != SCHEMA_VERSION {
            return Err(Unreadable);
        }
        let mut watch = Self::default();
        for entry in file.songs {
            if entry.key.is_empty() || entry.key.chars().count() > MAX_KEY_CHARS {
                continue;
            }
            watch.songs.insert(entry.setlist_song_id, entry);
        }
        watch.keep_newest(MAX_WATCHED);
        Ok(watch)
    }

    pub fn to_json(&self) -> String {
        let file = File {
            schema_version: SCHEMA_VERSION,
            songs: self.songs.values().cloned().collect(),
        };
        let mut text = serde_json::to_string_pretty(&file).unwrap_or_default();
        text.push('\n');
        text
    }

    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    /// Compares every song with what was remembered, records the changes, and puts `key_was` and `key_changed_at` on
    /// the songs that have a recorded change (however old: the window decides what to show). Songs without a number
    /// or a key are left alone. Returns whether the table changed in a way worth saving (a song seen for the first
    /// time, or a new key); seeing the same keys again changes nothing to save.
    pub fn observe(&mut self, songs: &mut [LibrarySong], now: DateTime<Utc>) -> bool {
        let mut changed = false;
        for song in songs.iter_mut() {
            let (Some(id), Some(key)) = (song.setlist_song_id, song.key.as_ref()) else {
                continue;
            };
            let entry = match self.songs.get_mut(&id) {
                None => self.songs.entry(id).or_insert_with(|| {
                    changed = true;
                    Entry {
                        setlist_song_id: id,
                        key: key.clone(),
                        seen_at: now,
                        key_was: None,
                        key_changed_at: None,
                    }
                }),
                Some(entry) => {
                    if entry.key != *key {
                        entry.key_was = Some(std::mem::replace(&mut entry.key, key.clone()));
                        entry.key_changed_at = Some(now);
                        changed = true;
                    }
                    entry.seen_at = now;
                    entry
                }
            };
            song.key_was = entry.key_was.clone();
            song.key_changed_at = entry
                .key_changed_at
                .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true));
        }
        if changed {
            self.tidy(now);
        }
        changed
    }

    /// Forgets the songs not read for a week and keeps the 500 read most recently.
    fn tidy(&mut self, now: DateTime<Utc>) {
        let oldest = now - Duration::days(FORGET_AFTER_DAYS);
        self.songs.retain(|_, entry| entry.seen_at >= oldest);
        self.keep_newest(MAX_WATCHED);
    }

    fn keep_newest(&mut self, most: usize) {
        if self.songs.len() <= most {
            return;
        }
        let mut by_age: Vec<(DateTime<Utc>, i64)> = self
            .songs
            .values()
            .map(|entry| (entry.seen_at, entry.setlist_song_id))
            .collect();
        by_age.sort();
        let drop = self.songs.len() - most;
        for (_, id) in by_age.into_iter().take(drop) {
            self.songs.remove(&id);
        }
    }

    /// The key remembered for a song (tests).
    #[doc(hidden)]
    pub fn key_of(&self, id: i64) -> Option<&str> {
        self.songs.get(&id).map(|e| e.key.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::parse_setlist;
    use chrono::TimeZone;
    use serde_json::json;

    fn at(hours: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap() + Duration::hours(hours)
    }

    /// Songs the way the helper's setlist parses, with these (id, key) pairs.
    fn songs(keys: &[(i64, Option<&str>)]) -> Vec<LibrarySong> {
        let items: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, (id, key))| {
                let mut item =
                    json!({"setlist_song_id": id, "order": i + 1, "title": format!("Song {id}")});
                if let Some(key) = key {
                    item["key"] = json!({"name": key});
                }
                item
            })
            .collect();
        parse_setlist(&json!({"id": 1, "title": "Set", "items": items}))
            .unwrap()
            .songs
    }

    #[test]
    fn a_song_seen_for_the_first_time_is_remembered_with_no_change() {
        let mut watch = KeyWatch::default();
        let mut read = songs(&[(11, Some("G")), (12, Some("A"))]);
        assert!(watch.observe(&mut read, at(0)));
        assert!(read
            .iter()
            .all(|s| s.key_was.is_none() && s.key_changed_at.is_none()));
        assert_eq!(watch.len(), 2);
    }

    #[test]
    fn the_same_key_again_changes_nothing_to_save() {
        let mut watch = KeyWatch::default();
        watch.observe(&mut songs(&[(11, Some("G"))]), at(0));
        let mut read = songs(&[(11, Some("G"))]);
        assert!(!watch.observe(&mut read, at(5)));
        assert_eq!(
            (read[0].key_was.clone(), read[0].key_changed_at.clone()),
            (None, None)
        );
    }

    #[test]
    fn another_key_records_what_it_was_and_when_and_keeps_saying_so() {
        let mut watch = KeyWatch::default();
        watch.observe(&mut songs(&[(11, Some("G")), (12, Some("A"))]), at(0));
        let mut read = songs(&[(11, Some("Ab")), (12, Some("A"))]);
        assert!(watch.observe(&mut read, at(3)));
        assert_eq!(read[0].key_was.as_deref(), Some("G"));
        assert_eq!(
            read[0].key_changed_at.as_deref(),
            Some("2026-11-20T03:00:00Z")
        );
        assert_eq!(read[1].key_was, None);
        // Later reads of the unchanged key carry the same record, and save nothing.
        let mut later = songs(&[(11, Some("Ab"))]);
        assert!(!watch.observe(&mut later, at(30)));
        assert_eq!(later[0].key_was.as_deref(), Some("G"));
        assert_eq!(
            later[0].key_changed_at.as_deref(),
            Some("2026-11-20T03:00:00Z")
        );
    }

    #[test]
    fn a_change_back_is_a_new_change() {
        let mut watch = KeyWatch::default();
        watch.observe(&mut songs(&[(11, Some("G"))]), at(0));
        watch.observe(&mut songs(&[(11, Some("Ab"))]), at(1));
        let mut back = songs(&[(11, Some("G"))]);
        assert!(watch.observe(&mut back, at(2)));
        assert_eq!(back[0].key_was.as_deref(), Some("Ab"));
        assert_eq!(
            back[0].key_changed_at.as_deref(),
            Some("2026-11-20T02:00:00Z")
        );
    }

    #[test]
    fn songs_with_no_number_or_no_key_are_left_alone() {
        let mut watch = KeyWatch::default();
        let mut read = songs(&[(11, None), (12, Some("A"))]);
        read[1].setlist_song_id = None;
        assert!(!watch.observe(&mut read, at(0)));
        assert!(watch.is_empty());
        // A song whose key goes missing keeps what was remembered for when the key comes back.
        watch.observe(&mut songs(&[(11, Some("G"))]), at(0));
        assert!(!watch.observe(&mut songs(&[(11, None)]), at(1)));
        assert_eq!(watch.key_of(11), Some("G"));
    }

    #[test]
    fn the_table_survives_a_save_and_a_load() {
        let mut watch = KeyWatch::default();
        watch.observe(&mut songs(&[(11, Some("G"))]), at(0));
        watch.observe(&mut songs(&[(11, Some("Ab"))]), at(1));
        let text = watch.to_json();
        assert!(text.contains("\"setlist_song_id\": 11"));
        assert!(!text.contains("Song 11"), "no title is saved");
        let loaded = KeyWatch::from_json(&text).unwrap();
        assert_eq!(loaded, watch);
        let mut read = songs(&[(11, Some("Ab"))]);
        assert!(!loaded.clone().observe(&mut read, at(2)));
        assert_eq!(read[0].key_was.as_deref(), Some("G"));
    }

    #[test]
    fn a_damaged_or_foreign_file_is_not_used() {
        for text in [
            "",
            "not json",
            "[]",
            "{\"schema_version\": 1}",
            "{\"schema_version\": 2, \"songs\": []}",
            "{\"schema_version\": 1, \"songs\": [{\"setlist_song_id\": \"x\"}]}",
        ] {
            assert_eq!(KeyWatch::from_json(text), Err(Unreadable), "{text:?}");
        }
        // An entry with no key is skipped, the rest is kept.
        let text = "{\"schema_version\": 1, \"songs\": [\
            {\"setlist_song_id\": 1, \"key\": \"\", \"seen_at\": \"2026-11-20T00:00:00Z\"},\
            {\"setlist_song_id\": 2, \"key\": \"G\", \"seen_at\": \"2026-11-20T00:00:00Z\"}]}";
        assert_eq!(KeyWatch::from_json(text).unwrap().len(), 1);
    }

    #[test]
    fn at_most_500_songs_are_kept_and_the_one_read_longest_ago_goes_first() {
        let mut watch = KeyWatch::default();
        for id in 0..500 {
            watch.observe(
                &mut songs(&[(id, Some("G"))]),
                at(0) + Duration::minutes(id),
            );
        }
        assert_eq!(watch.len(), 500);
        watch.observe(
            &mut songs(&[(1000, Some("G"))]),
            at(0) + Duration::minutes(500),
        );
        assert_eq!(watch.len(), 500);
        assert!(watch.key_of(0).is_none(), "the oldest is gone");
        assert!(watch.key_of(1).is_some());
        assert!(watch.key_of(1000).is_some());
        // A file with too many songs is cut to the newest when read.
        let big = File {
            schema_version: 1,
            songs: (0..600)
                .map(|id| Entry {
                    setlist_song_id: id,
                    key: "G".into(),
                    seen_at: at(0) + Duration::minutes(id),
                    key_was: None,
                    key_changed_at: None,
                })
                .collect(),
        };
        let loaded = KeyWatch::from_json(&serde_json::to_string(&big).unwrap()).unwrap();
        assert_eq!(
            (
                loaded.len(),
                loaded.key_of(99).is_none(),
                loaded.key_of(100).is_some()
            ),
            (500, true, true)
        );
    }

    #[test]
    fn a_song_not_read_for_a_week_is_dropped_when_the_table_is_next_written() {
        let mut watch = KeyWatch::default();
        watch.observe(&mut songs(&[(11, Some("G")), (12, Some("A"))]), at(0));
        // Song 12 is read every day; song 11 is not read again.
        for day in 1..=8 {
            watch.observe(&mut songs(&[(12, Some("A"))]), at(day * 24));
        }
        assert!(
            watch.key_of(11).is_some(),
            "nothing was written, so nothing was dropped"
        );
        assert!(watch.observe(&mut songs(&[(13, Some("D"))]), at(8 * 24 + 1)));
        assert!(watch.key_of(11).is_none());
        assert!(watch.key_of(12).is_some());
        assert!(watch.key_of(13).is_some());
    }
}
