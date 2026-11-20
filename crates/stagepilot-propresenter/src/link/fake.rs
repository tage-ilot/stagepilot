//! [`FakeHost`]: a ProPresenter computer in memory, for tests. It follows the same rules as the real hosts (names, the Playback-only
//! create-only writes, the size limit, `Songs` that only an import adds to), keeps a log of what was asked and can be told to fail
//! the next call or to refuse imports. Nothing here touches a disk or the network.

use super::paths::{
    ensure_name, import_name, library_name, playlist_name, sha256_hex, song_file_name, song_number,
};
use super::protobuf::multitracks_id;
use super::{
    HostInfo, IndexEntry, LibraryFile, LinkError, ProPresenterHost, SelectOutcome, MAX_FILE_BYTES,
    PLAYBACK_LIBRARY, SONGS_LIBRARY,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Mutex;

#[derive(Default)]
struct State {
    libraries: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    calls: Vec<String>,
    fail_next: Option<LinkError>,
    can_import: bool,
    blocker: Option<String>,
    /// What an import writes: the bytes of the new file (default: a few bytes).
    import_bytes: Vec<u8>,
    imports: Vec<(String, String, Option<String>)>,
    selected: Vec<String>,
    select_tries: bool,
}

/// A ProPresenter computer in memory.
pub struct FakeHost {
    state: Mutex<State>,
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHost {
    /// A folder with the library `Songs` and nothing else; imports are possible.
    pub fn new() -> Self {
        let mut state = State {
            can_import: true,
            import_bytes: b"imported".to_vec(),
            select_tries: true,
            ..State::default()
        };
        state
            .libraries
            .insert(SONGS_LIBRARY.to_string(), BTreeMap::new());
        Self {
            state: Mutex::new(state),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Adds a library (a folder), as ProPresenter itself would have made it.
    #[must_use]
    pub fn with_library(self, name: &str) -> Self {
        self.lock().libraries.entry(name.to_string()).or_default();
        self
    }

    /// Adds a file to a library, as ProPresenter itself would have (no rule applies).
    #[must_use]
    pub fn with_file(self, library: &str, file: &str, bytes: &[u8]) -> Self {
        self.lock()
            .libraries
            .entry(library.to_string())
            .or_default()
            .insert(file.to_string(), bytes.to_vec());
        self
    }

    /// Whether a song can be imported from here, and why not.
    pub fn set_import(&self, can: bool, blocker: Option<&str>) {
        let mut s = self.lock();
        s.can_import = can;
        s.blocker = blocker.map(str::to_string);
    }

    /// What the next imports write into `Songs`.
    pub fn set_import_bytes(&self, bytes: &[u8]) {
        self.lock().import_bytes = bytes.to_vec();
    }

    /// The next call (whatever it is) fails with this error.
    pub fn fail_next(&self, error: LinkError) {
        self.lock().fail_next = Some(error);
    }

    /// Whether `select_library` reports that it tried.
    pub fn set_select_tries(&self, tries: bool) {
        self.lock().select_tries = tries;
    }

    /// Every call made, as `name(args)`.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    /// The imports asked for: (song number, name, playlist).
    pub fn imports(&self) -> Vec<(String, String, Option<String>)> {
        self.lock().imports.clone()
    }

    /// The libraries `select_library` was asked for.
    pub fn selected(&self) -> Vec<String> {
        self.lock().selected.clone()
    }

    /// A file's bytes, for a test to look at.
    pub fn file(&self, library: &str, file: &str) -> Option<Vec<u8>> {
        self.lock().libraries.get(library)?.get(file).cloned()
    }

    fn begin(&self, call: String) -> Result<std::sync::MutexGuard<'_, State>, LinkError> {
        let mut s = self.lock();
        s.calls.push(call);
        match s.fail_next.take() {
            Some(error) => Err(error),
            None => Ok(s),
        }
    }
}

#[async_trait]
impl ProPresenterHost for FakeHost {
    async fn info(&self) -> Result<HostInfo, LinkError> {
        let s = self.begin("info()".into())?;
        Ok(HostInfo {
            version: crate::helper_assets::BUNDLED_HELPER_VERSION.to_string(),
            propresenter_version: Some("19.0.1".to_string()),
            os: "macOS".to_string(),
            data_folder: "/fake/ProPresenter".to_string(),
            data_folder_exists: true,
            libraries: s.libraries.keys().cloned().collect(),
            can_import: s.can_import,
            import_blocker: if s.can_import {
                None
            } else {
                s.blocker.clone()
            },
        })
    }

    async fn list_library(&self, name: &str) -> Result<Vec<LibraryFile>, LinkError> {
        let s = self.begin(format!("list_library({name})"))?;
        let library = s.libraries.get(library_name(name)?).ok_or_else(|| {
            LinkError::NotFound("That library is not in ProPresenter's folder.".into())
        })?;
        Ok(library
            .iter()
            .filter(|(file, _)| file.ends_with(".pro") && !file.starts_with('.'))
            .map(|(file, bytes)| LibraryFile {
                name: file.clone(),
                size: bytes.len() as u64,
                mtime: 1_700_000_000,
            })
            .collect())
    }

    async fn read_file(&self, library: &str, file: &str) -> Result<Vec<u8>, LinkError> {
        let s = self.begin(format!("read_file({library},{file})"))?;
        let (library, file) = (library_name(library)?, song_file_name(file)?);
        let bytes = s
            .libraries
            .get(library)
            .and_then(|l| l.get(file))
            .ok_or_else(|| LinkError::NotFound("That file is not in the library.".into()))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(LinkError::TooLarge);
        }
        Ok(bytes.clone())
    }

    async fn ensure_library(&self, name: &str) -> Result<bool, LinkError> {
        let mut s = self.begin(format!("ensure_library({name})"))?;
        let name = ensure_name(name)?;
        if s.libraries.contains_key(name) {
            return Ok(false);
        }
        s.libraries.insert(name.to_string(), BTreeMap::new());
        Ok(true)
    }

    async fn create_file(
        &self,
        library: &str,
        file: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<(), LinkError> {
        let mut s = self.begin(format!("create_file({library},{file})"))?;
        if library != PLAYBACK_LIBRARY {
            return Err(LinkError::NotAllowed(format!(
                "Files can only be added to the library named {PLAYBACK_LIBRARY}."
            )));
        }
        let file = song_file_name(file)?;
        let sha256 = sha256_hex(sha256)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(LinkError::TooLarge);
        }
        let folder = s.libraries.get_mut(PLAYBACK_LIBRARY).ok_or_else(|| {
            LinkError::NotFound(format!(
                "The {PLAYBACK_LIBRARY} library does not exist yet."
            ))
        })?;
        if folder.contains_key(file) {
            return Err(LinkError::AlreadyExists(
                "A file with that name is already in the library. Files are never replaced.".into(),
            ));
        }
        if hex::encode(Sha256::digest(bytes)) != sha256 {
            return Err(LinkError::HashMismatch);
        }
        folder.insert(file.to_string(), bytes.to_vec());
        Ok(())
    }

    async fn song_index(&self) -> Result<Vec<IndexEntry>, LinkError> {
        let s = self.begin("song_index()".into())?;
        Ok(s.libraries
            .iter()
            .flat_map(|(library, files)| {
                files
                    .iter()
                    .filter(|(file, _)| file.ends_with(".pro"))
                    .map(move |(file, bytes)| IndexEntry {
                        library: library.clone(),
                        file: file.clone(),
                        multitracks_id: multitracks_id(Cursor::new(bytes), bytes.len() as u64),
                    })
            })
            .collect())
    }

    async fn search_import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: Option<String>,
    ) -> Result<String, LinkError> {
        let mut s = self.begin(format!("search_import({multitracks_id},{name})"))?;
        let id = song_number(multitracks_id)?.to_string();
        let name = import_name(name)?;
        if let Some(playlist) = &playlist {
            playlist_name(playlist)?;
        }
        if !s.can_import {
            return Err(LinkError::CannotImport(s.blocker.clone().unwrap_or_else(
                || "A song cannot be imported from this computer.".into(),
            )));
        }
        let file = format!("{name}.pro");
        let bytes = s.import_bytes.clone();
        let songs = s.libraries.entry(SONGS_LIBRARY.to_string()).or_default();
        if songs.contains_key(&file) {
            return Err(LinkError::AlreadyExists(
                "A song with that name is already in the Songs library.".into(),
            ));
        }
        songs.insert(file.clone(), bytes);
        s.imports.push((id, name, playlist));
        Ok(file)
    }

    async fn select_library(&self, name: &str) -> Result<SelectOutcome, LinkError> {
        let mut s = self.begin(format!("select_library({name})"))?;
        library_name(name)?;
        s.selected.push(name.to_string());
        Ok(SelectOutcome {
            tried: s.select_tries,
            detail: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    #[tokio::test]
    async fn the_fake_keeps_the_rules_of_the_real_hosts() {
        let host = FakeHost::new();
        assert!(host.ensure_library("Playback").await.unwrap());
        assert!(!host.ensure_library("Playback").await.unwrap());
        assert!(host.ensure_library("Songs").await.is_err());
        host.create_file("Playback", "A (1).pro", b"abc", &hash(b"abc"))
            .await
            .unwrap();
        assert!(matches!(
            host.create_file("Playback", "A (1).pro", b"abc", &hash(b"abc"))
                .await,
            Err(LinkError::AlreadyExists(_))
        ));
        assert_eq!(
            host.create_file("Playback", "B.pro", b"abc", &hash(b"abd"))
                .await,
            Err(LinkError::HashMismatch)
        );
        assert!(matches!(
            host.create_file("Songs", "B.pro", b"abc", &hash(b"abc"))
                .await,
            Err(LinkError::NotAllowed(_))
        ));
        assert_eq!(
            host.read_file("Playback", "A (1).pro").await.unwrap(),
            b"abc"
        );
        let listed = host.list_library("Playback").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(host.song_index().await.unwrap()[0].multitracks_id, None);
    }

    #[tokio::test]
    async fn an_import_adds_to_songs_only_and_can_be_refused() {
        let host = FakeHost::new();
        assert_eq!(
            host.search_import("123", "New Song", Some("Sunday".into()))
                .await
                .unwrap(),
            "New Song.pro"
        );
        assert_eq!(
            host.imports(),
            [("123".into(), "New Song".into(), Some("Sunday".into()))]
        );
        assert!(matches!(
            host.search_import("123", "New Song", None).await,
            Err(LinkError::AlreadyExists(_))
        ));
        host.set_import(false, Some("Allow it."));
        assert_eq!(
            host.search_import("124", "Other", None).await,
            Err(LinkError::CannotImport("Allow it.".into()))
        );
        host.fail_next(LinkError::NoAnswer);
        assert_eq!(host.info().await, Err(LinkError::NoAnswer));
        assert!(host.info().await.is_ok(), "only the next call fails");
        assert!(host.calls().len() >= 5);
    }
}
