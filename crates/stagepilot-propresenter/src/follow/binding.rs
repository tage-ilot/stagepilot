//! Where a song lives in ProPresenter: the binding of a plan song (by its MultiTracks number, Playback's `content_id`) to the
//! playlist item and the `Playback` copy of its file (design `SPEC.md`, rule 7).
//!
//! * (a) **Remembered** when the Presenter Link adds the song to a playlist ([`BindingStore`], persisted beside the settings
//!   file; a missing or damaged file means nothing is remembered).
//! * (b) **Found on demand** in the ACTIVE playlist ([`find_in_active_playlist`]): the playlist item whose presentation file
//!   (read through the Presenter Link host, local or helper) carries that MultiTracks number and a selected `Playback`
//!   arrangement.
//!
//! The arrangement hash is part of the binding: a file whose arrangement is not the one that was remembered is not followed.

use super::{MapError, SongMap};
use crate::link::api::{ApiError, ProPresenterApi};
use crate::link::{LibraryFile, LinkError, ProPresenterHost, PLAYBACK_LIBRARY};
use crate::pro_file::Song;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A plan song's place in ProPresenter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// The MultiTracks number (Playback's `content_id`).
    pub content_id: u64,
    pub playlist_id: String,
    /// The item's position in the playlist when it was seen (it is looked up again before the item is triggered).
    pub item_index: usize,
    /// The presentation's uuid: what `GET /v1/presentation/active` says while the song is on screen.
    pub presentation_uuid: String,
    /// The song file in the `Playback` library, with `.pro`.
    pub file: String,
    /// `SongMap::arrangement_hash` of the file when the song was bound.
    pub arrangement_hash: String,
    /// The cue count of each group of the file, in file order (what the hash depends on, kept to say what changed).
    #[serde(default)]
    pub group_cues: Vec<usize>,
}

/// What is remembered between runs.
pub trait BindingStore: Send + Sync {
    /// Every remembered binding by MultiTracks number. A file that is missing or damaged remembers nothing.
    fn all(&self) -> BTreeMap<u64, Binding>;
    /// Remembers (or replaces) the binding of a song.
    fn remember(&self, binding: Binding) -> Result<(), String>;
    fn forget(&self, content_id: u64);
}

/// Bindings kept in memory only (tests, the demo).
#[derive(Default)]
pub struct MemoryBindingStore {
    songs: Mutex<BTreeMap<u64, Binding>>,
}

impl BindingStore for MemoryBindingStore {
    fn all(&self) -> BTreeMap<u64, Binding> {
        self.songs.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn remember(&self, binding: Binding) -> Result<(), String> {
        self.songs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(binding.content_id, binding);
        Ok(())
    }

    fn forget(&self, content_id: u64) {
        self.songs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&content_id);
    }
}

#[derive(Serialize, Deserialize)]
struct FileBody {
    schema_version: u8,
    songs: Vec<Binding>,
}

/// `presenter-follow-bindings.json` beside the settings file, replaced atomically (temp file, then rename). Holds ids, file
/// names and counts, never a lyric and never a key.
pub struct FileBindingStore {
    path: PathBuf,
    warned: Mutex<bool>,
}

impl FileBindingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            warned: Mutex::new(false),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn write(&self, all: &BTreeMap<u64, Binding>) -> Result<(), String> {
        let body = FileBody {
            schema_version: 1,
            songs: all.values().cloned().collect(),
        };
        let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("the folder could not be made: {e}"))?;
        }
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, text).map_err(|e| format!("the file could not be written: {e}"))?;
        std::fs::rename(&temp, &self.path).map_err(|e| format!("the file could not be saved: {e}"))
    }
}

impl BindingStore for FileBindingStore {
    fn all(&self) -> BTreeMap<u64, Binding> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(_) => return BTreeMap::new(),
        };
        match serde_json::from_str::<FileBody>(&text) {
            Ok(body) => body.songs.into_iter().map(|b| (b.content_id, b)).collect(),
            Err(_) => {
                let mut warned = self.warned.lock().unwrap_or_else(|p| p.into_inner());
                if !*warned {
                    *warned = true;
                    tracing::warn!("presenter_follow_bindings_unreadable");
                }
                BTreeMap::new()
            }
        }
    }

    fn remember(&self, binding: Binding) -> Result<(), String> {
        let mut all = self.all();
        all.insert(binding.content_id, binding);
        self.write(&all)
    }

    fn forget(&self, content_id: u64) {
        let mut all = self.all();
        if all.remove(&content_id).is_some() {
            let _ = self.write(&all);
        }
    }
}

/// Why a song could not be bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookError {
    /// ProPresenter's API did not answer or answered oddly.
    Api(ApiError),
    /// The ProPresenter computer's files could not be read.
    Host(LinkError),
}

impl LookError {
    pub fn words(&self) -> String {
        match self {
            Self::Api(e) => e.words(),
            Self::Host(e) => e.words(),
        }
    }
}

impl From<ApiError> for LookError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

impl From<LinkError> for LookError {
    fn from(e: LinkError) -> Self {
        Self::Host(e)
    }
}

/// What is read of a `Playback` song file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub multitracks_id: Option<u64>,
    pub map: Result<SongMap, MapError>,
}

/// The song files already read, by name, valid while the file's size and time stay the same. One cache serves every look.
#[derive(Default)]
pub struct FileCache {
    parsed: HashMap<String, (u64, i64, Arc<ParsedFile>)>,
    listing: Vec<LibraryFile>,
}

impl FileCache {
    /// Lists the `Playback` library once (so the next reads know which files changed).
    pub async fn refresh(&mut self, host: &dyn ProPresenterHost) -> Result<(), LinkError> {
        self.listing = host.list_library(PLAYBACK_LIBRARY).await?;
        let names: BTreeSet<&str> = self.listing.iter().map(|f| f.name.as_str()).collect();
        self.parsed.retain(|name, _| names.contains(name.as_str()));
        Ok(())
    }

    pub fn has_file(&self, file: &str) -> bool {
        self.listing.iter().any(|f| f.name == file)
    }

    /// The parsed file; read through the host only when it is new or changed. `Ok(None)` when the library has no such file
    /// or the bytes are not a song.
    pub async fn parsed(
        &mut self,
        host: &dyn ProPresenterHost,
        file: &str,
    ) -> Result<Option<Arc<ParsedFile>>, LinkError> {
        let Some(listed) = self.listing.iter().find(|f| f.name == file) else {
            return Ok(None);
        };
        let (size, mtime) = (listed.size, listed.mtime);
        if let Some((s, m, parsed)) = self.parsed.get(file) {
            if *s == size && *m == mtime {
                return Ok(Some(parsed.clone()));
            }
        }
        let bytes = host.read_file(PLAYBACK_LIBRARY, file).await?;
        let Ok(song) = Song::read(&bytes) else {
            return Ok(None);
        };
        let parsed = Arc::new(ParsedFile {
            multitracks_id: song.multitracks_id,
            map: SongMap::from_song(&song),
        });
        self.parsed
            .insert(file.to_string(), (size, mtime, parsed.clone()));
        Ok(Some(parsed))
    }
}

/// One presentation item of a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistItem {
    pub index: usize,
    /// The song's name: the file without `.pro`.
    pub name: String,
    pub presentation_uuid: String,
}

/// The presentation items of a playlist read from `GET /v1/playlist/{id}` (headers and placeholders left out).
pub fn playlist_items(playlist: &Value) -> Vec<PlaylistItem> {
    let Some(items) = playlist.get("items").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(position, item)| {
            let kind = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("presentation");
            if kind != "presentation" {
                return None;
            }
            let id = item.get("id")?;
            let name = id.get("name")?.as_str()?.to_string();
            let presentation_uuid = item
                .get("presentation_info")
                .and_then(|p| p.get("presentation_uuid"))
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                .or_else(|| id.get("uuid").and_then(Value::as_str))
                .filter(|u| !u.is_empty())?
                .to_string();
            let index = id
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(position);
            Some(PlaylistItem {
                index,
                name,
                presentation_uuid,
            })
        })
        .collect()
}

/// The bindings of the song items of one playlist that are `Playback` copies StagePilot can follow: the file is in the
/// `Playback` library, carries a MultiTracks number and has the `Playback` arrangement selected. `only` limits it to these
/// file names (with `.pro`). The first item wins when a song is in the playlist twice.
pub async fn bindings_in_playlist(
    host: &dyn ProPresenterHost,
    api: &dyn ProPresenterApi,
    cache: &mut FileCache,
    playlist_id: &str,
    only: Option<&BTreeSet<String>>,
) -> Result<Vec<Binding>, LookError> {
    let playlist = api.playlist(playlist_id).await?;
    bindings_of_items(host, cache, playlist_id, &playlist_items(&playlist), only).await
}

/// [`bindings_in_playlist`] for items already read.
pub async fn bindings_of_items(
    host: &dyn ProPresenterHost,
    cache: &mut FileCache,
    playlist_id: &str,
    items: &[PlaylistItem],
    only: Option<&BTreeSet<String>>,
) -> Result<Vec<Binding>, LookError> {
    let mut out: Vec<Binding> = Vec::new();
    for item in items {
        let file = format!("{}.pro", item.name);
        if only.is_some_and(|set| !set.contains(&file)) || !cache.has_file(&file) {
            continue;
        }
        let Some(parsed) = cache.parsed(host, &file).await? else {
            continue;
        };
        let (Some(content_id), Ok(map)) = (parsed.multitracks_id, parsed.map.as_ref()) else {
            continue;
        };
        if out.iter().any(|b| b.content_id == content_id) {
            continue;
        }
        out.push(Binding {
            content_id,
            playlist_id: playlist_id.to_string(),
            item_index: item.index,
            presentation_uuid: item.presentation_uuid.clone(),
            file,
            arrangement_hash: map.arrangement_hash(),
            group_cues: map.groups().iter().map(|g| g.cues).collect(),
        });
    }
    Ok(out)
}

/// The binding of one song found in the ACTIVE playlist (the one ProPresenter triggered last). `Ok(None)` when there is no
/// active playlist or the song is not in it as a followable `Playback` copy.
pub async fn find_in_active_playlist(
    host: &dyn ProPresenterHost,
    api: &dyn ProPresenterApi,
    cache: &mut FileCache,
    content_id: u64,
) -> Result<Option<Binding>, LookError> {
    let Some(active) = api.active_playlist().await? else {
        return Ok(None);
    };
    let found = bindings_in_playlist(host, api, cache, &active.playlist_uuid, None).await?;
    Ok(found.into_iter().find(|b| b.content_id == content_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: u64) -> Binding {
        Binding {
            content_id: id,
            playlist_id: "PL".into(),
            item_index: 3,
            presentation_uuid: "PU".into(),
            file: "Song.pro".into(),
            arrangement_hash: "abc".into(),
            group_cues: vec![1, 4],
        }
    }

    #[test]
    fn the_file_store_remembers_across_instances_and_replaces_by_song_number() {
        let dir = std::env::temp_dir().join(format!("sp-binding-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("presenter-follow-bindings.json");
        let store = FileBindingStore::new(&path);
        assert!(store.all().is_empty(), "a missing file remembers nothing");
        store.remember(binding(7)).unwrap();
        let mut other = binding(8);
        other.file = "Other.pro".into();
        store.remember(other.clone()).unwrap();
        let mut moved = binding(7);
        moved.item_index = 9;
        store.remember(moved.clone()).unwrap();
        let again = FileBindingStore::new(&path).all();
        assert_eq!(again.len(), 2);
        assert_eq!(again[&7], moved);
        assert_eq!(again[&8], other);
        store.forget(8);
        assert_eq!(FileBindingStore::new(&path).all().len(), 1);
        // A damaged file remembers nothing and is replaced by the next write.
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(store.all().is_empty());
        store.remember(binding(1)).unwrap();
        assert_eq!(store.all().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_saved_file_holds_ids_and_counts_only() {
        let dir = std::env::temp_dir().join(format!("sp-binding-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("b.json");
        FileBindingStore::new(&path).remember(binding(5)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for word in [
            "content_id",
            "playlist_id",
            "presentation_uuid",
            "arrangement_hash",
        ] {
            assert!(text.contains(word), "{word}");
        }
        for word in ["key", "secret", "password", "lyric"] {
            assert!(!text.to_lowercase().contains(word), "{word}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn playlist_items_skip_headers_and_placeholders_and_read_the_presentation_uuid() {
        let playlist = serde_json::json!({"id": {"uuid": "PL"}, "items": [
            {"id": {"uuid": "", "name": "Worship", "index": 0}, "type": "header"},
            {"id": {"uuid": "I1", "name": "Song A", "index": 1}, "type": "presentation",
             "presentation_info": {"presentation_uuid": "P1", "arrangement_name": "Playback"}},
            {"id": {"uuid": "I2", "name": "Gone", "index": 2}, "type": "placeholder"},
            {"id": {"uuid": "I3", "name": "Song B", "index": 3}, "type": "presentation"}
        ]});
        assert_eq!(
            playlist_items(&playlist),
            vec![
                PlaylistItem {
                    index: 1,
                    name: "Song A".into(),
                    presentation_uuid: "P1".into()
                },
                PlaylistItem {
                    index: 3,
                    name: "Song B".into(),
                    presentation_uuid: "I3".into()
                }
            ]
        );
        assert!(playlist_items(&serde_json::json!({"items": 3})).is_empty());
    }
}
