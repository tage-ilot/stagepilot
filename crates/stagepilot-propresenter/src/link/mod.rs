//! The ProPresenter Link: StagePilot's reach into the files and the screen of the ProPresenter computer.
//!
//! One trait, [`ProPresenterHost`], two ways to reach it:
//!
//! * [`local::LocalHost`] when StagePilot runs on the ProPresenter computer itself: the files are read and written
//!   directly and the screen scripts are started with `osascript`. No helper, no key.
//! * [`helper::HelperHost`] when ProPresenter is on another computer: the ProPresenter Link helper
//!   (`assets/propresenter-link/propresenter_link.py`, port 8790) answers signed requests that mirror the trait.
//!
//! Both follow the same rules, so callers cannot tell them apart:
//!
//! * Every path is relative to ProPresenter's data folder and is built from a library name and a file name, never
//!   from a path. `..`, separators, absolute paths and symlinks that lead out of the folder are refused.
//! * Reading is free; writing is not. The only things that can be written are a new `.pro` file in the library named
//!   [`PLAYBACK_LIBRARY`] (create-only: a name that is taken is never replaced) and a new library folder (never
//!   `Songs`). Nothing is ever edited, renamed or deleted.
//! * [`ProPresenterHost::search_import`] drives ProPresenter's own Search on screen (macOS Accessibility) and is
//!   the only way a file is added to the `Songs` library.
//!
//! [`fake::FakeHost`] is an in-memory host with the same rules, for tests.

pub mod add_songs;
pub mod api;
pub mod fake;
pub mod helper;
pub mod local;
pub mod paths;
pub mod protobuf;
pub mod screen;
pub mod status;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// The one library StagePilot may add files to.
pub const PLAYBACK_LIBRARY: &str = "Playback";
/// ProPresenter's own song library. Nothing here writes to it directly; only ProPresenter's import does.
pub const SONGS_LIBRARY: &str = "Songs";
/// The largest `.pro` file that is read or written.
pub const MAX_FILE_BYTES: usize = 40 * 1024 * 1024;
/// How long a song import may take from the first click to the file on disk.
pub const IMPORT_WAIT_SECONDS: u64 = 30;

/// What a host says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    /// The version of the helper (or, for the local host, of the copy bundled in this StagePilot).
    pub version: String,
    /// ProPresenter's own version, when it could be found.
    #[serde(default)]
    pub propresenter_version: Option<String>,
    /// `macOS`, `Linux`, `Windows`...
    pub os: String,
    pub data_folder: String,
    /// The `Libraries` folder is there.
    #[serde(default)]
    pub data_folder_exists: bool,
    /// The library folders under `Libraries`, sorted.
    #[serde(default)]
    pub libraries: Vec<String>,
    /// A song can be imported through ProPresenter's Search from here (macOS and the Accessibility permission).
    pub can_import: bool,
    /// Why not, in plain words.
    #[serde(default)]
    pub import_blocker: Option<String>,
}

/// One `.pro` file of a library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryFile {
    pub name: String,
    pub size: u64,
    /// Seconds since the epoch.
    pub mtime: i64,
}

/// One song file found under `Libraries/*`, with its MultiTracks number when the file holds one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub library: String,
    pub file: String,
    #[serde(default)]
    pub multitracks_id: Option<u64>,
}

/// What `select_library` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOutcome {
    pub tried: bool,
    #[serde(default)]
    pub detail: Option<String>,
}

/// Why a call to a host did not work. Every case has one plain sentence ([`LinkError::words`]) and a short code that the
/// helper uses on the wire ([`LinkError::code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// A name or path that is not allowed (a separator, `..`, an absolute path, a path that leaves the folder, not `.pro`).
    BadPath(String),
    NotFound(String),
    /// Create-only: the name is taken.
    AlreadyExists(String),
    TooLarge,
    /// The bytes did not match the checksum that came with them.
    HashMismatch,
    /// A write somewhere other than the library named `Playback`.
    NotAllowed(String),
    /// A song cannot be imported from here, and why.
    CannotImport(String),
    /// The on-screen steps stopped; which step and why.
    Screen {
        step: String,
        message: String,
    },
    /// ProPresenter's folder is not on that computer.
    FolderMissing(String),
    Busy(String),
    Timeout(String),
    /// Nothing answered.
    NoAnswer,
    /// The helper refused this StagePilot (HTTP 401, 403 or 429).
    Refused(u16),
    /// The answer was not in the expected shape, or too big.
    Unreadable,
    /// A file operation failed on this computer.
    Io(String),
}

impl LinkError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadPath(_) => "bad_path",
            Self::NotFound(_) => "not_found",
            Self::AlreadyExists(_) => "exists",
            Self::TooLarge => "too_large",
            Self::HashMismatch => "hash_mismatch",
            Self::NotAllowed(_) => "not_allowed",
            Self::CannotImport(_) => "cannot_import",
            Self::Screen { .. } => "screen",
            Self::FolderMissing(_) => "folder_missing",
            Self::Busy(_) => "busy",
            Self::Timeout(_) => "timeout",
            Self::NoAnswer => "no_answer",
            Self::Refused(_) => "refused",
            Self::Unreadable => "unreadable",
            Self::Io(_) => "io",
        }
    }

    /// One plain sentence. It never holds a key or a signature.
    pub fn words(&self) -> String {
        match self {
            Self::BadPath(m)
            | Self::NotFound(m)
            | Self::AlreadyExists(m)
            | Self::NotAllowed(m)
            | Self::CannotImport(m)
            | Self::FolderMissing(m)
            | Self::Busy(m)
            | Self::Timeout(m)
            | Self::Io(m) => m.clone(),
            Self::TooLarge => "That file is larger than 40 MB.".to_string(),
            Self::HashMismatch => {
                "The file arrived changed (its checksum does not match). Nothing was added."
                    .to_string()
            }
            Self::Screen { step, message } => {
                format!("The import stopped at the step \"{step}\": {message}")
            }
            Self::NoAnswer => "The ProPresenter computer did not answer.".to_string(),
            Self::Refused(_) => {
                "The ProPresenter Link refused this StagePilot. Run the setup command again."
                    .to_string()
            }
            Self::Unreadable => {
                "The ProPresenter Link answered, but not in a way StagePilot understands."
                    .to_string()
            }
        }
    }

    /// The error a helper's JSON answer `{error, code}` stands for.
    pub fn from_wire(status: u16, code: &str, message: &str) -> Self {
        let message = message.chars().take(300).collect::<String>();
        match (status, code) {
            (401 | 429, _) | (403, "") => Self::Refused(status),
            (_, "bad_path" | "bad_name" | "bad_id" | "bad_hash") => Self::BadPath(message),
            (_, "not_found") => Self::NotFound(message),
            (_, "exists") => Self::AlreadyExists(message),
            (_, "too_large") => Self::TooLarge,
            (_, "hash_mismatch") => Self::HashMismatch,
            (_, "not_allowed") => Self::NotAllowed(message),
            (_, "cannot_import") => Self::CannotImport(message),
            (_, "folder_missing") => Self::FolderMissing(message),
            (_, "busy") => Self::Busy(message),
            (_, "timeout") => Self::Timeout(message),
            (_, "screen") => Self::Screen {
                step: "on screen".to_string(),
                message,
            },
            _ => Self::Unreadable,
        }
    }
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words())
    }
}

impl std::error::Error for LinkError {}

/// The files and the screen of the ProPresenter computer. Object safe: StagePilot holds a `dyn ProPresenterHost`.
#[async_trait]
pub trait ProPresenterHost: Send + Sync {
    /// Version, system, folder, libraries and whether a song can be imported from here.
    async fn info(&self) -> Result<HostInfo, LinkError>;

    /// The `.pro` files of one library (nothing else), sorted by name.
    async fn list_library(&self, name: &str) -> Result<Vec<LibraryFile>, LinkError>;

    /// One file's bytes; at most [`MAX_FILE_BYTES`].
    async fn read_file(&self, library: &str, file: &str) -> Result<Vec<u8>, LinkError>;

    /// Makes the library folder when it is not there. `true` when it was made. Never `Songs`.
    async fn ensure_library(&self, name: &str) -> Result<bool, LinkError>;

    /// Adds a NEW file to the library named [`PLAYBACK_LIBRARY`]: create-only, `*.pro`, at most [`MAX_FILE_BYTES`], the
    /// bytes must match `sha256` (lower-case hex). An existing name is an error whatever it holds.
    async fn create_file(
        &self,
        library: &str,
        file: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<(), LinkError>;

    /// Every `.pro` file under `Libraries/*` with its MultiTracks number when it has one.
    async fn song_index(&self) -> Result<Vec<IndexEntry>, LinkError>;

    /// Imports one song through ProPresenter's Search (MultiTracks) on screen into the library `Songs`, under `name`
    /// (no dots), added to `playlist` when one is named. Returns the new file's name.
    async fn search_import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: Option<String>,
    ) -> Result<String, LinkError>;

    /// Best effort: select the library's row on screen so ProPresenter loads the files added there.
    async fn select_library(&self, name: &str) -> Result<SelectOutcome, LinkError>;
}
