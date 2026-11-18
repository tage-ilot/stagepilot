//! [`LocalHost`]: the ProPresenter Link when StagePilot runs on the ProPresenter computer itself. The files are read and written
//! directly and the screen is driven with `osascript` ([`super::screen`]). On a computer that is not a Mac the file operations
//! work and a song cannot be imported.
//!
//! [`Folder`] is the synchronous part (the rules about names, symlinks, create-only writes and the song index). The
//! ProPresenter Link helper (`propresenter_link.py`) implements the same rules in Python, so both behave alike.

use super::paths::{
    ensure_name, import_name, library_name, playlist_name, sha256_hex, song_file_name, song_number,
};
use super::protobuf::multitracks_id_of_file;
use super::screen::{MacScreen, Screen, NOT_A_MAC};
use super::{
    HostInfo, IndexEntry, LibraryFile, LinkError, ProPresenterHost, SelectOutcome,
    IMPORT_WAIT_SECONDS, MAX_FILE_BYTES, PLAYBACK_LIBRARY, SONGS_LIBRARY,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// `~/Documents/ProPresenter` unless the settings name another folder.
pub fn default_data_folder() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join("Documents").join("ProPresenter")
}

/// The saved folder (a leading `~/` is the home folder) or the default.
pub fn resolve_data_folder(setting: Option<&str>) -> PathBuf {
    match setting.map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) => match path.strip_prefix("~/") {
            Some(rest) => {
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map_or_else(|| PathBuf::from("."), PathBuf::from);
                home.join(rest)
            }
            None => PathBuf::from(path),
        },
        None => default_data_folder(),
    }
}

fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    }
}

fn io(error: &std::io::Error) -> LinkError {
    LinkError::Io(match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            "StagePilot is not allowed to use ProPresenter's folder. Allow it in System Settings > Privacy & Security > Files and Folders."
                .to_string()
        }
        _ => "ProPresenter's folder could not be read or written on this computer.".to_string(),
    })
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(hex::encode(hasher.finalize()));
        }
        hasher.update(&buffer[..n]);
    }
}

fn seconds(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

type CacheKey = (PathBuf, u64, u128);

/// ProPresenter's data folder and everything that can be done to it.
pub struct Folder {
    root: PathBuf,
    cache: Mutex<HashMap<CacheKey, Option<u64>>>,
}

impl Folder {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn real_root(&self) -> Result<PathBuf, LinkError> {
        self.root.canonicalize().map_err(|_| self.missing())
    }

    fn missing(&self) -> LinkError {
        LinkError::FolderMissing(format!(
            "ProPresenter's folder was not found on this computer ({}). Open ProPresenter once.",
            self.root.display()
        ))
    }

    fn libraries_dir(&self) -> Result<PathBuf, LinkError> {
        let path = self.root.join("Libraries");
        if !path.is_dir() {
            return Err(self.missing());
        }
        self.inside(&path)?;
        Ok(path)
    }

    /// The real path of an existing path, which must lie inside the folder (symlinks resolved).
    fn inside(&self, path: &Path) -> Result<PathBuf, LinkError> {
        let root = self.real_root()?;
        let real = path
            .canonicalize()
            .map_err(|_| LinkError::NotFound("That is not in ProPresenter's folder.".into()))?;
        if real.starts_with(&root) {
            Ok(real)
        } else {
            Err(LinkError::BadPath(
                "That path leads outside ProPresenter's folder.".into(),
            ))
        }
    }

    fn library_path(&self, name: &str) -> Result<PathBuf, LinkError> {
        let path = self.libraries_dir()?.join(library_name(name)?);
        if path.exists() {
            self.inside(&path)?;
        }
        Ok(path)
    }

    pub fn library_names(&self) -> Result<Vec<String>, LinkError> {
        let base = self.libraries_dir()?;
        let mut names = vec![];
        for entry in std::fs::read_dir(&base).map_err(|e| io(&e))?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if !name.starts_with('.') && path.is_dir() && self.inside(&path).is_ok() {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn list_library(&self, name: &str) -> Result<Vec<LibraryFile>, LinkError> {
        let path = self.library_path(name)?;
        if !path.is_dir() {
            return Err(LinkError::NotFound(
                "That library is not in ProPresenter's folder.".into(),
            ));
        }
        let mut files = vec![];
        for entry in std::fs::read_dir(&path).map_err(|e| io(&e))?.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !file_name.ends_with(".pro") || file_name.starts_with('.') {
                continue;
            }
            let file_path = entry.path();
            if self.inside(&file_path).is_err() {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&file_path) else {
                continue;
            };
            if meta.is_file() {
                files.push(LibraryFile {
                    name: file_name,
                    size: meta.len(),
                    mtime: meta.modified().map_or(0, seconds),
                });
            }
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }

    pub fn read_file(&self, library: &str, file: &str) -> Result<Vec<u8>, LinkError> {
        let path = self.library_path(library)?.join(song_file_name(file)?);
        let real = self.inside(&path)?;
        let meta = std::fs::metadata(&real).map_err(|e| io(&e))?;
        if !meta.is_file() {
            return Err(LinkError::NotFound(
                "That file is not in the library.".into(),
            ));
        }
        if meta.len() > MAX_FILE_BYTES as u64 {
            return Err(LinkError::TooLarge);
        }
        std::fs::read(&real).map_err(|e| io(&e))
    }

    pub fn ensure_library(&self, name: &str) -> Result<bool, LinkError> {
        let name = ensure_name(name)?;
        let path = self.libraries_dir()?.join(name);
        if path.is_dir() {
            return Ok(false);
        }
        if path.exists() {
            return Err(LinkError::BadPath(
                "A file with that name is in the way.".into(),
            ));
        }
        std::fs::create_dir(&path).map_err(|e| io(&e))?;
        Ok(true)
    }

    /// Create-only: the bytes go to a temporary file in the same folder, are written out to disk, their checksum is checked, and the
    /// file is put in place with a hard link (which fails when the name is taken, so nothing is ever replaced).
    pub fn create_file(
        &self,
        library: &str,
        file: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<(), LinkError> {
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
        let folder = self.library_path(library)?;
        if !folder.is_dir() {
            return Err(LinkError::NotFound(format!(
                "The {PLAYBACK_LIBRARY} library does not exist yet."
            )));
        }
        let final_path = folder.join(file);
        let taken = || {
            LinkError::AlreadyExists(
                "A file with that name is already in the library. Files are never replaced.".into(),
            )
        };
        if final_path.symlink_metadata().is_ok() {
            return Err(taken());
        }
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let temp = folder.join(format!(
            ".{file}.{}-{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let outcome = (|| {
            let mut handle = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| io(&e))?;
            handle.write_all(bytes).map_err(|e| io(&e))?;
            handle.flush().map_err(|e| io(&e))?;
            handle.sync_all().map_err(|e| io(&e))?;
            drop(handle);
            if file_sha256(&temp).map_err(|e| io(&e))? != sha256 {
                return Err(LinkError::HashMismatch);
            }
            match std::fs::hard_link(&temp, &final_path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(taken()),
                Err(e) => Err(io(&e)),
            }
        })();
        let _ = std::fs::remove_file(&temp);
        outcome?;
        if file_sha256(&final_path).map_err(|e| io(&e))? != sha256 {
            return Err(LinkError::HashMismatch);
        }
        Ok(())
    }

    pub fn song_index(&self) -> Result<Vec<IndexEntry>, LinkError> {
        let mut out = vec![];
        for library in self.library_names()? {
            let base = self.libraries_dir()?.join(&library);
            let mut names: Vec<String> = std::fs::read_dir(&base)
                .map_err(|e| io(&e))?
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(".pro") && !n.starts_with('.'))
                .collect();
            names.sort();
            for file in names {
                let path = base.join(&file);
                if self.inside(&path).is_err() {
                    continue;
                }
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                let key = (path.clone(), meta.len(), mtime);
                let known = self
                    .cache
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(&key)
                    .copied();
                let id = match known {
                    Some(id) => id,
                    None => {
                        let id = multitracks_id_of_file(&path);
                        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
                        if cache.len() > 20_000 {
                            cache.clear();
                        }
                        cache.insert(key, id);
                        id
                    }
                };
                out.push(IndexEntry {
                    library: library.clone(),
                    file,
                    multitracks_id: id,
                });
            }
        }
        Ok(out)
    }
}

/// StagePilot on the ProPresenter computer.
pub struct LocalHost {
    folder: Arc<Folder>,
    screen: Arc<dyn Screen>,
    one_import: tokio::sync::Mutex<()>,
    import_wait: Duration,
}

impl LocalHost {
    /// The default folder or the saved one, and this computer's own screen.
    pub fn new(data_folder: Option<&str>) -> Self {
        Self::with_screen(
            resolve_data_folder(data_folder),
            Arc::new(MacScreen::new(MacScreen::default_dir())),
        )
    }

    pub fn with_screen(root: PathBuf, screen: Arc<dyn Screen>) -> Self {
        Self {
            folder: Arc::new(Folder::new(root)),
            screen,
            one_import: tokio::sync::Mutex::new(()),
            import_wait: Duration::from_secs(IMPORT_WAIT_SECONDS),
        }
    }

    /// How long to wait for the file after the import was started (tests shorten it).
    #[must_use]
    pub fn with_import_wait(mut self, wait: Duration) -> Self {
        self.import_wait = wait;
        self
    }

    pub fn folder(&self) -> &Folder {
        &self.folder
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Folder) -> Result<T, LinkError> + Send + 'static,
    ) -> Result<T, LinkError> {
        let folder = self.folder.clone();
        tokio::task::spawn_blocking(move || work(&folder))
            .await
            .map_err(|_| LinkError::Io("The folder could not be read.".into()))?
    }
}

#[async_trait]
impl ProPresenterHost for LocalHost {
    async fn info(&self) -> Result<HostInfo, LinkError> {
        let (can_import, import_blocker) = self.screen.access().await;
        let (exists, libraries) = self
            .blocking(|f| {
                Ok(match f.library_names() {
                    Ok(names) => (true, names),
                    Err(_) => (false, vec![]),
                })
            })
            .await?;
        Ok(HostInfo {
            version: crate::helper_assets::BUNDLED_HELPER_VERSION.to_string(),
            propresenter_version: None,
            os: os_name().to_string(),
            data_folder: self.folder.root().display().to_string(),
            data_folder_exists: exists,
            libraries,
            can_import,
            import_blocker: import_blocker.or_else(|| (!can_import).then(|| NOT_A_MAC.to_string())),
        })
    }

    async fn list_library(&self, name: &str) -> Result<Vec<LibraryFile>, LinkError> {
        let name = name.to_string();
        self.blocking(move |f| f.list_library(&name)).await
    }

    async fn read_file(&self, library: &str, file: &str) -> Result<Vec<u8>, LinkError> {
        let (library, file) = (library.to_string(), file.to_string());
        self.blocking(move |f| f.read_file(&library, &file)).await
    }

    async fn ensure_library(&self, name: &str) -> Result<bool, LinkError> {
        let name = name.to_string();
        self.blocking(move |f| f.ensure_library(&name)).await
    }

    async fn create_file(
        &self,
        library: &str,
        file: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<(), LinkError> {
        let (library, file, bytes, sha256) = (
            library.to_string(),
            file.to_string(),
            bytes.to_vec(),
            sha256.to_string(),
        );
        self.blocking(move |f| f.create_file(&library, &file, &bytes, &sha256))
            .await
    }

    async fn song_index(&self) -> Result<Vec<IndexEntry>, LinkError> {
        self.blocking(Folder::song_index).await
    }

    async fn search_import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: Option<String>,
    ) -> Result<String, LinkError> {
        let id = song_number(multitracks_id)?.to_string();
        let name = import_name(name)?;
        let playlist = playlist.unwrap_or_default();
        playlist_name(&playlist)?;
        let (can_import, blocker) = self.screen.access().await;
        if !can_import {
            return Err(LinkError::CannotImport(
                blocker.unwrap_or_else(|| NOT_A_MAC.to_string()),
            ));
        }
        let file = format!("{name}.pro");
        let target = self.folder.library_path(SONGS_LIBRARY)?.join(&file);
        if target.symlink_metadata().is_ok() {
            return Err(LinkError::AlreadyExists(
                "A song with that name is already in the Songs library.".into(),
            ));
        }
        if !target.parent().is_some_and(Path::is_dir) {
            return Err(LinkError::NotFound(
                "ProPresenter's Songs library was not found.".into(),
            ));
        }
        let Ok(_one_at_a_time) = self.one_import.try_lock() else {
            return Err(LinkError::Busy(
                "A song is being imported right now.".into(),
            ));
        };
        self.screen.import(&id, &name, &playlist).await?;
        let deadline = Instant::now() + self.import_wait;
        loop {
            if std::fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() > 0) {
                tokio::time::sleep(Duration::from_millis(500).min(self.import_wait)).await;
                return Ok(file);
            }
            if Instant::now() >= deadline {
                return Err(LinkError::Timeout(format!(
                    "ProPresenter did not write the song within {} seconds.",
                    self.import_wait.as_secs().max(1)
                )));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn select_library(&self, name: &str) -> Result<SelectOutcome, LinkError> {
        library_name(name)?;
        Ok(self.screen.select_library(name).await)
    }
}
