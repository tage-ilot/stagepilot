//! [`FakeApi`]: ProPresenter's HTTP API in memory, on top of a [`FakeHost`] (the files). It keeps the rules proven on a real
//! ProPresenter 19.0.1:
//!
//! * a library lists the `.pro` files of its folder as `<Song> - [ <Library> ]`;
//! * a file that was NOT there when the API started gets its id only when ProPresenter loads it: at once, after some reads, or
//!   only after the library was selected on screen ([`Loading`]);
//! * a playlist write replaces the whole list; a song item must carry the library entry's id twice and its display name
//!   without the suffix, else the WHOLE write is refused with 404 and nothing changes;
//! * the `arrangement_name` that is sent is ignored: a song reads back with the arrangement its file has selected.
//!
//! Everything can be made to fail on purpose (`set_down`, `fail_put`, `corrupt_reads`).

use super::{ApiError, Layers, LibraryEntry, LibraryRef, PlaylistRef, ProPresenterApi};
use crate::link::fake::FakeHost;
use crate::pro_file::Song;
use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

/// When ProPresenter gives a newly placed file its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loading {
    /// As soon as the library is read.
    Immediately,
    /// After the library listing was read this many times since the file was first seen.
    AfterReads(u32),
    /// Only once the library was selected on screen (`select_library`).
    AfterSelect,
}

type Corrupt = Arc<dyn Fn(&mut Vec<Value>) + Send + Sync>;

struct Playlist {
    uuid: String,
    name: String,
    folder: Option<String>,
    items: Vec<Value>,
}

struct State {
    playlists: Vec<Playlist>,
    /// Files that were there from the start (their ids are known at once).
    known: BTreeSet<(String, String)>,
    /// How many times the library listing was read since the file was first seen there.
    reads: BTreeMap<(String, String), u32>,
    loading: Loading,
    down: bool,
    fail_put: Option<ApiError>,
    corrupt: Option<Corrupt>,
    calls: Vec<String>,
    puts: Vec<Vec<Value>>,
    version: String,
    /// What `GET /v1/status/layers` says now (a camera input on, as on the real rig, nothing on screen).
    layers: Result<Layers, ApiError>,
    /// Answers that are given first, one per call, before `layers` is.
    layers_next: VecDeque<Result<Layers, ApiError>>,
}

pub struct FakeApi {
    host: Arc<FakeHost>,
    state: Mutex<State>,
}

/// A uuid-shaped id made from a library and file name.
pub fn entry_uuid(library: &str, file: &str) -> String {
    let digest = Sha256::digest(format!("{library}/{file}").as_bytes());
    let hex = hex::encode_upper(&digest[..16]);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

impl FakeApi {
    /// The files that are in the host now are loaded; later ones load as `loading` says.
    pub fn new(host: Arc<FakeHost>, loading: Loading) -> Self {
        let mut known = BTreeSet::new();
        for library in host.library_names() {
            for (file, _) in host.files_of(&library) {
                known.insert((library.clone(), file));
            }
        }
        Self {
            host,
            state: Mutex::new(State {
                playlists: Vec::new(),
                known,
                reads: BTreeMap::new(),
                loading,
                down: false,
                fail_put: None,
                corrupt: None,
                calls: Vec::new(),
                puts: Vec::new(),
                version: "ProPresenter 19.0.1 (API v1)".to_string(),
                layers: Ok(Layers {
                    video_input: true,
                    ..Layers::default()
                }),
                layers_next: VecDeque::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A playlist with these items (as the API would show them), optionally inside a folder.
    pub fn add_playlist(&self, name: &str, folder: Option<&str>, items: Vec<Value>) -> String {
        let mut s = self.lock();
        let uuid = format!("00000000-0000-4000-8000-{:012X}", s.playlists.len() + 1);
        s.playlists.push(Playlist {
            uuid: uuid.clone(),
            name: name.to_string(),
            folder: folder.map(str::to_string),
            items,
        });
        uuid
    }

    /// Nothing answers while it is down.
    pub fn set_down(&self, down: bool) {
        self.lock().down = down;
    }

    /// The next playlist writes fail with this error (and change nothing).
    pub fn fail_put(&self, error: Option<ApiError>) {
        self.lock().fail_put = error;
    }

    /// Every playlist read AFTER a playlist write passes its items through `f` first (what ProPresenter would show differently).
    pub fn corrupt_reads(&self, f: Option<Corrupt>) {
        self.lock().corrupt = f;
    }

    /// What `/v1/status/layers` says from now on.
    pub fn set_layers(&self, layers: Layers) {
        self.lock().layers = Ok(layers);
    }

    /// `/v1/status/layers` answers, from now on, with something that is not the expected shape.
    pub fn set_layers_unreadable(&self) {
        self.lock().layers = Err(ApiError::Unreadable);
    }

    /// The next calls to `/v1/status/layers` answer with these, one each, before the standing answer is used again.
    pub fn queue_layers(&self, answers: Vec<Result<Layers, ApiError>>) {
        self.lock().layers_next.extend(answers);
    }

    pub fn set_loading(&self, loading: Loading) {
        self.lock().loading = loading;
    }

    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    /// Every list of items written, in order.
    pub fn puts(&self) -> Vec<Vec<Value>> {
        self.lock().puts.clone()
    }

    /// The items of a playlist as stored (not as read).
    pub fn stored(&self, playlist_uuid: &str) -> Vec<Value> {
        self.lock()
            .playlists
            .iter()
            .find(|p| p.uuid == playlist_uuid)
            .map(|p| p.items.clone())
            .unwrap_or_default()
    }

    /// A presentation item as the API shows one.
    pub fn presentation_item(uuid: &str, name: &str, arrangement: &str, duration: f64) -> Value {
        json!({
            "id": {"uuid": uuid, "name": name, "index": 0},
            "type": "presentation", "is_hidden": false, "is_pco": false,
            "presentation_info": {"presentation_uuid": uuid, "arrangement_name": arrangement},
            "duration": duration
        })
    }

    pub fn header_item(name: &str, color: f64) -> Value {
        json!({
            "id": {"uuid": "", "name": name, "index": 0},
            "type": "header", "is_hidden": false, "is_pco": false,
            "header_color": {"red": color, "green": 0.5, "blue": 0.25, "alpha": 1.0}
        })
    }

    /// Every library entry now: (library, file, id when loaded).
    fn entries(
        &self,
        s: &State,
        library_filter: Option<&str>,
    ) -> Vec<(String, String, Option<String>)> {
        let mut out = Vec::new();
        for library in self.host.library_names() {
            if library_filter.is_some_and(|f| f != library) {
                continue;
            }
            let selected = self.host.selected().contains(&library);
            for (file, _) in self.host.files_of(&library) {
                let key = (library.clone(), file.clone());
                let loaded = s.known.contains(&key)
                    || match s.loading {
                        Loading::Immediately => true,
                        Loading::AfterReads(n) => s.reads.get(&key).copied().unwrap_or(0) > n,
                        Loading::AfterSelect => selected,
                    };
                let uuid = loaded.then(|| entry_uuid(&library, &file));
                out.push((library.clone(), file, uuid));
            }
        }
        out
    }

    fn arrangement_of(&self, library: &str, file: &str) -> String {
        self.host
            .files_of(library)
            .into_iter()
            .find(|(f, _)| f == file)
            .and_then(|(_, bytes)| Song::read(&bytes).ok())
            .and_then(|song| song.selected_arrangement().map(|a| a.name.clone()))
            .unwrap_or_default()
    }

    fn up(&self, call: String) -> Result<std::sync::MutexGuard<'_, State>, ApiError> {
        let mut s = self.lock();
        s.calls.push(call);
        if s.down {
            return Err(ApiError::NoAnswer("it is not there".into()));
        }
        Ok(s)
    }

    fn song_by_uuid(&self, s: &State, wanted: &str) -> Option<(String, String)> {
        self.entries(s, None)
            .into_iter()
            .find(|(_, _, uuid)| uuid.as_deref() == Some(wanted))
            .map(|(library, file, _)| (library, file))
    }
}

#[async_trait]
impl ProPresenterApi for FakeApi {
    async fn version(&self) -> Result<String, ApiError> {
        let s = self.up("version".into())?;
        Ok(s.version.clone())
    }

    async fn playlists(&self) -> Result<Vec<PlaylistRef>, ApiError> {
        let s = self.up("playlists".into())?;
        Ok(s.playlists
            .iter()
            .map(|p| PlaylistRef {
                uuid: p.uuid.clone(),
                name: p.name.clone(),
                folder: p.folder.clone(),
            })
            .collect())
    }

    async fn playlist(&self, id: &str) -> Result<Value, ApiError> {
        super::plain_id(id)?;
        let s = self.up(format!("playlist({id})"))?;
        let playlist = s
            .playlists
            .iter()
            .find(|p| p.uuid == id)
            .ok_or(ApiError::NotFound)?;
        let mut items = playlist.items.clone();
        for item in &mut items {
            if item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("presentation")
                != "presentation"
            {
                continue;
            }
            let uuid = item["id"]["uuid"].as_str().unwrap_or_default().to_string();
            if let Some((library, file)) = self.song_by_uuid(&s, &uuid) {
                // The arrangement is the one the file itself has selected, whatever was sent.
                item["presentation_info"] = json!({
                    "presentation_uuid": uuid,
                    "arrangement_name": self.arrangement_of(&library, &file)
                });
            }
        }
        // Only what ProPresenter shows after a write differs; what was there before is read as it was.
        if let (Some(corrupt), false) = (&s.corrupt, s.puts.is_empty()) {
            corrupt(&mut items);
        }
        Ok(
            json!({"id": {"uuid": playlist.uuid, "name": playlist.name, "index": 0}, "items": items}),
        )
    }

    async fn put_playlist(&self, id: &str, items: &[Value]) -> Result<(), ApiError> {
        super::plain_id(id)?;
        let mut s = self.up(format!("put_playlist({id},{})", items.len()))?;
        if let Some(error) = s.fail_put.clone() {
            return Err(error);
        }
        if !s.playlists.iter().any(|p| p.uuid == id) {
            return Err(ApiError::NotFound);
        }
        for item in items {
            let kind = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("presentation");
            if matches!(kind, "header" | "placeholder") || kind != "presentation" {
                continue;
            }
            let uuid = item["id"]["uuid"].as_str().unwrap_or_default();
            let Some((library, file)) = self.song_by_uuid(&s, uuid) else {
                return Err(ApiError::NotFound);
            };
            let shown = file.strip_suffix(".pro").unwrap_or(&file);
            if item["presentation_info"]["presentation_uuid"].as_str() != Some(uuid)
                || item["id"]["name"].as_str() != Some(shown)
            {
                return Err(ApiError::NotFound);
            }
            let _ = library;
        }
        s.puts.push(items.to_vec());
        let stored: Vec<Value> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut item = item.clone();
                item["id"]["index"] = json!(index);
                item
            })
            .collect();
        if let Some(playlist) = s.playlists.iter_mut().find(|p| p.uuid == id) {
            playlist.items = stored;
        }
        Ok(())
    }

    async fn libraries(&self) -> Result<Vec<LibraryRef>, ApiError> {
        let _s = self.up("libraries".into())?;
        Ok(self
            .host
            .library_names()
            .into_iter()
            .map(|name| LibraryRef {
                uuid: entry_uuid("library", &name),
                name,
            })
            .collect())
    }

    async fn library(&self, id: &str) -> Result<Vec<LibraryEntry>, ApiError> {
        super::plain_id(id)?;
        let mut s = self.up(format!("library({id})"))?;
        let name = self
            .host
            .library_names()
            .into_iter()
            .find(|name| entry_uuid("library", name) == id)
            .ok_or(ApiError::NotFound)?;
        for (file, _) in self.host.files_of(&name) {
            *s.reads.entry((name.clone(), file)).or_insert(0) += 1;
        }
        Ok(self
            .entries(&s, Some(&name))
            .into_iter()
            .map(|(library, file, uuid)| LibraryEntry {
                uuid,
                name: format!(
                    "{} - [ {} ]",
                    file.strip_suffix(".pro").unwrap_or(&file),
                    library
                ),
            })
            .collect())
    }

    async fn layers(&self) -> Result<Layers, ApiError> {
        let mut s = self.up("layers".into())?;
        match s.layers_next.pop_front() {
            Some(answer) => answer,
            None => s.layers.clone(),
        }
    }
}
