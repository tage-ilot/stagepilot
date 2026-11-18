//! The ProPresenter Link on this computer (`LocalHost`): the rules about names and symlinks, create-only writes into the library
//! named Playback, the size limit, the song index against synthetic `.pro` bytes, and the import steps against a stand-in screen.
//! No real ProPresenter, no screen, no network.

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use stagepilot_propresenter::link::local::{resolve_data_folder, LocalHost};
use stagepilot_propresenter::link::screen::Screen;
use stagepilot_propresenter::link::{
    LinkError, ProPresenterHost, SelectOutcome, MAX_FILE_BYTES, PLAYBACK_LIBRARY,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn varint(mut value: u64) -> Vec<u8> {
    let mut out = vec![];
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// A synthetic `.pro`: some other fields, then the licence record (field 21 holding field 1 = the song number).
fn song(id: Option<u64>, filler: usize) -> Vec<u8> {
    let mut out = vec![0x0a, 4];
    out.extend(b"info");
    out.extend([0x12]);
    out.extend(varint(filler as u64));
    out.extend(vec![9u8; filler]);
    if let Some(id) = id {
        let mut record = vec![0x08];
        record.extend(varint(id));
        out.extend([0xaa, 0x01]);
        out.extend(varint(record.len() as u64));
        out.extend(record);
    }
    out
}

struct Rig {
    dir: tempfile::TempDir,
    host: LocalHost,
    screen: Arc<FakeScreen>,
}

impl Rig {
    fn root(&self) -> PathBuf {
        self.dir.path().join("ProPresenter")
    }
    fn lib(&self, name: &str) -> PathBuf {
        self.root().join("Libraries").join(name)
    }
}

#[derive(Default)]
struct FakeScreen {
    can: bool,
    /// What the import does: write this file into the Songs folder (None: nothing).
    writes: Mutex<Option<PathBuf>>,
    fails: Mutex<Option<LinkError>>,
    calls: Mutex<Vec<(String, String, String)>>,
    selected: Mutex<Vec<String>>,
}

#[async_trait]
impl Screen for FakeScreen {
    async fn access(&self) -> (bool, Option<String>) {
        if self.can {
            (true, None)
        } else {
            (false, Some("Allow StagePilot in Accessibility.".into()))
        }
    }
    async fn import(&self, id: &str, name: &str, playlist: &str) -> Result<(), LinkError> {
        self.calls
            .lock()
            .unwrap()
            .push((id.into(), name.into(), playlist.into()));
        if let Some(error) = self.fails.lock().unwrap().take() {
            return Err(error);
        }
        if let Some(path) = self.writes.lock().unwrap().clone() {
            std::fs::write(path, song(Some(id.parse().unwrap()), 10)).unwrap();
        }
        Ok(())
    }
    async fn select_library(&self, name: &str) -> SelectOutcome {
        self.selected.lock().unwrap().push(name.into());
        SelectOutcome {
            tried: true,
            detail: None,
        }
    }
}

fn rig(can_import: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ProPresenter");
    for lib in ["Songs", "Playback", "Other Library"] {
        std::fs::create_dir_all(root.join("Libraries").join(lib)).unwrap();
    }
    let screen = Arc::new(FakeScreen {
        can: can_import,
        ..FakeScreen::default()
    });
    let host =
        LocalHost::with_screen(root, screen.clone()).with_import_wait(Duration::from_millis(800));
    Rig { dir, host, screen }
}

#[tokio::test]
async fn info_says_where_the_folder_is_which_libraries_it_has_and_whether_a_song_can_be_imported() {
    let r = rig(true);
    let info = r.host.info().await.unwrap();
    assert_eq!(info.libraries, ["Other Library", "Playback", "Songs"]);
    assert!(info.data_folder_exists && info.can_import && info.import_blocker.is_none());
    assert_eq!(info.data_folder, r.root().display().to_string());
    let blocked = rig(false).host.info().await.unwrap();
    assert!(!blocked.can_import);
    assert_eq!(
        blocked.import_blocker.as_deref(),
        Some("Allow StagePilot in Accessibility.")
    );
    // A folder that is not there: still an answer, with nothing in it.
    let missing = LocalHost::with_screen(
        PathBuf::from("/nonexistent/pp"),
        Arc::new(FakeScreen::default()),
    );
    let info = missing.info().await.unwrap();
    assert!(!info.data_folder_exists && info.libraries.is_empty());
    assert!(matches!(
        missing.list_library("Songs").await,
        Err(LinkError::FolderMissing(_))
    ));
}

#[tokio::test]
async fn listing_shows_only_pro_files_with_size_and_time() {
    let r = rig(false);
    std::fs::write(r.lib("Songs").join("B.pro"), b"bb").unwrap();
    std::fs::write(r.lib("Songs").join("A.pro"), b"a").unwrap();
    std::fs::write(r.lib("Songs").join("notes.txt"), b"x").unwrap();
    std::fs::write(r.lib("Songs").join(".hidden.pro"), b"x").unwrap();
    std::fs::create_dir(r.lib("Songs").join("dir.pro")).unwrap();
    let files = r.host.list_library("Songs").await.unwrap();
    let names: Vec<_> = files.iter().map(|f| (f.name.as_str(), f.size)).collect();
    assert_eq!(names, [("A.pro", 1), ("B.pro", 2)]);
    assert!(files[0].mtime > 1_500_000_000);
    assert!(matches!(
        r.host.list_library("Nope").await,
        Err(LinkError::NotFound(_))
    ));
}

#[tokio::test]
async fn paths_that_escape_the_folder_are_refused_everywhere() {
    let r = rig(true);
    std::fs::write(r.root().join("secret.pro"), b"secret").unwrap();
    for bad in ["..", "../Libraries", "a/b", "/etc", "..\\x", ".hidden", ""] {
        assert!(
            matches!(r.host.list_library(bad).await, Err(LinkError::BadPath(_))),
            "{bad:?}"
        );
        assert!(
            matches!(
                r.host.read_file(bad, "A.pro").await,
                Err(LinkError::BadPath(_))
            ),
            "{bad:?}"
        );
        assert!(r.host.ensure_library(bad).await.is_err(), "{bad:?}");
    }
    for bad in [
        "../secret.pro",
        "/etc/passwd.pro",
        "a/../b.pro",
        "x.txt",
        "..",
        ".pro",
        ".a.pro",
    ] {
        assert!(
            matches!(
                r.host.read_file("Songs", bad).await,
                Err(LinkError::BadPath(_))
            ),
            "{bad:?}"
        );
        let bytes = b"x";
        assert!(
            r.host
                .create_file("Playback", bad, bytes, &hash(bytes))
                .await
                .is_err(),
            "{bad:?}"
        );
    }
    assert!(r.host.search_import("12", "../x", None).await.is_err());
    assert!(r.host.search_import("12", "a.b", None).await.is_err());
    assert!(r.host.select_library("../x").await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_that_leads_out_of_the_folder_is_never_followed() {
    let r = rig(true);
    let outside = r.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("Stolen.pro"), b"stolen").unwrap();
    // A library that is a link out, and a file that is a link out.
    std::os::unix::fs::symlink(&outside, r.lib("Linked")).unwrap();
    std::os::unix::fs::symlink(outside.join("Stolen.pro"), r.lib("Songs").join("Link.pro"))
        .unwrap();
    std::fs::write(r.lib("Songs").join("Real.pro"), song(Some(5), 4)).unwrap();
    assert!(matches!(
        r.host.list_library("Linked").await,
        Err(LinkError::BadPath(_))
    ));
    assert!(matches!(
        r.host.read_file("Linked", "Stolen.pro").await,
        Err(LinkError::BadPath(_))
    ));
    assert!(matches!(
        r.host.read_file("Songs", "Link.pro").await,
        Err(LinkError::BadPath(_))
    ));
    let listed: Vec<_> = r
        .host
        .list_library("Songs")
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(listed, ["Real.pro"], "the link is not listed");
    let index = r.host.song_index().await.unwrap();
    assert!(
        index
            .iter()
            .all(|e| e.library != "Linked" && e.file != "Link.pro"),
        "{index:?}"
    );
    assert!(r
        .host
        .info()
        .await
        .unwrap()
        .libraries
        .iter()
        .all(|l| l != "Linked"));
    // A link inside the folder is fine.
    std::os::unix::fs::symlink(
        r.lib("Songs").join("Real.pro"),
        r.lib("Other Library").join("Inside.pro"),
    )
    .unwrap();
    assert!(r
        .host
        .read_file("Other Library", "Inside.pro")
        .await
        .is_ok());
    // Creating over a link is refused as an existing name.
    std::os::unix::fs::symlink(
        outside.join("Stolen.pro"),
        r.lib("Playback").join("Trap.pro"),
    )
    .unwrap();
    assert!(matches!(
        r.host
            .create_file("Playback", "Trap.pro", b"x", &hash(b"x"))
            .await,
        Err(LinkError::AlreadyExists(_))
    ));
    assert_eq!(
        std::fs::read(outside.join("Stolen.pro")).unwrap(),
        b"stolen"
    );
}

#[tokio::test]
async fn a_new_file_is_created_once_and_never_replaced() {
    let r = rig(true);
    let bytes = song(Some(77), 100);
    r.host
        .create_file("Playback", "Song (1).pro", &bytes, &hash(&bytes))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(r.lib("Playback").join("Song (1).pro")).unwrap(),
        bytes
    );
    assert_eq!(
        r.host.read_file("Playback", "Song (1).pro").await.unwrap(),
        bytes
    );
    let other = b"different".to_vec();
    assert!(matches!(
        r.host
            .create_file("Playback", "Song (1).pro", &other, &hash(&other))
            .await,
        Err(LinkError::AlreadyExists(_))
    ));
    assert_eq!(
        std::fs::read(r.lib("Playback").join("Song (1).pro")).unwrap(),
        bytes,
        "untouched"
    );
    // A name that exists as an original song's name is also taken.
    std::fs::write(r.lib("Playback").join("Original.pro"), b"orig").unwrap();
    assert!(r
        .host
        .create_file("Playback", "Original.pro", &other, &hash(&other))
        .await
        .is_err());
    assert_eq!(
        std::fs::read(r.lib("Playback").join("Original.pro")).unwrap(),
        b"orig"
    );
}

#[tokio::test]
async fn a_hash_that_does_not_match_adds_nothing_and_leaves_no_temporary_file() {
    let r = rig(true);
    assert_eq!(
        r.host
            .create_file("Playback", "A.pro", b"abc", &hash(b"abd"))
            .await,
        Err(LinkError::HashMismatch)
    );
    for bad in ["", "abc", &"A".repeat(64), &"g".repeat(64)] {
        assert!(
            matches!(
                r.host.create_file("Playback", "A.pro", b"abc", bad).await,
                Err(LinkError::BadPath(_))
            ),
            "{bad:?}"
        );
    }
    let left: Vec<_> = std::fs::read_dir(r.lib("Playback"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(left.is_empty(), "nothing left behind: {left:?}");
}

#[tokio::test]
async fn files_are_only_added_to_the_playback_library() {
    let r = rig(true);
    for library in ["Songs", "Other Library", "playback", "Playback ", "Nope"] {
        let result = r
            .host
            .create_file(library, "A.pro", b"x", &hash(b"x"))
            .await;
        assert!(
            matches!(result, Err(LinkError::NotAllowed(_))),
            "{library:?}: {result:?}"
        );
    }
    assert!(!r.lib("Songs").join("A.pro").exists());
    assert_eq!(PLAYBACK_LIBRARY, "Playback");
    // A Playback library that is not there is not made by a file write.
    std::fs::remove_dir_all(r.lib("Playback")).unwrap();
    assert!(matches!(
        r.host
            .create_file("Playback", "A.pro", b"x", &hash(b"x"))
            .await,
        Err(LinkError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_library_is_made_only_with_a_plain_name_and_never_songs() {
    let r = rig(true);
    assert!(r.host.ensure_library("Sunday Set-2").await.unwrap());
    assert!(r.lib("Sunday Set-2").is_dir());
    assert!(!r.host.ensure_library("Sunday Set-2").await.unwrap());
    for bad in ["Songs", "songs", "a_b", "..", "a/b", " x", "é"] {
        assert!(r.host.ensure_library(bad).await.is_err(), "{bad:?}");
    }
    assert_eq!(
        std::fs::read_dir(r.root().join("Libraries"))
            .unwrap()
            .count(),
        4
    );
}

#[tokio::test]
async fn files_over_forty_megabytes_are_neither_read_nor_written() {
    let r = rig(true);
    let big = vec![0u8; MAX_FILE_BYTES + 1];
    assert_eq!(
        r.host
            .create_file("Playback", "Big.pro", &big, &hash(&big))
            .await,
        Err(LinkError::TooLarge)
    );
    assert!(!r.lib("Playback").join("Big.pro").exists());
    let file = std::fs::File::create(r.lib("Songs").join("Huge.pro")).unwrap();
    file.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
    assert_eq!(
        r.host.read_file("Songs", "Huge.pro").await,
        Err(LinkError::TooLarge)
    );
    let exactly = vec![1u8; MAX_FILE_BYTES];
    r.host
        .create_file("Playback", "Exact.pro", &exactly, &hash(&exactly))
        .await
        .unwrap();
    assert_eq!(
        r.host
            .read_file("Playback", "Exact.pro")
            .await
            .unwrap()
            .len(),
        MAX_FILE_BYTES
    );
}

#[tokio::test]
async fn the_song_index_finds_the_number_in_a_valid_record_and_nothing_in_the_rest() {
    let r = rig(true);
    std::fs::write(
        r.lib("Songs").join("Valid.pro"),
        song(Some(4_000_001), 50_000),
    )
    .unwrap();
    std::fs::write(r.lib("Songs").join("NoRecord.pro"), song(None, 10)).unwrap();
    std::fs::write(
        r.lib("Songs").join("Garbage.pro"),
        b"not a protobuf at all \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff",
    )
    .unwrap();
    let whole = song(Some(9), 30);
    std::fs::write(
        r.lib("Other Library").join("Truncated.pro"),
        &whole[..whole.len() - 2],
    )
    .unwrap();
    std::fs::write(r.lib("Other Library").join("Empty.pro"), b"").unwrap();
    std::fs::write(r.lib("Other Library").join("readme.txt"), song(Some(1), 1)).unwrap();
    let index = r.host.song_index().await.unwrap();
    let got: Vec<_> = index
        .iter()
        .map(|e| (e.library.as_str(), e.file.as_str(), e.multitracks_id))
        .collect();
    assert_eq!(
        got,
        [
            ("Other Library", "Empty.pro", None),
            ("Other Library", "Truncated.pro", None),
            ("Songs", "Garbage.pro", None),
            ("Songs", "NoRecord.pro", None),
            ("Songs", "Valid.pro", Some(4_000_001)),
        ]
    );
    // Cached by (path, size, modified time): a changed file is read again.
    std::fs::write(r.lib("Songs").join("Valid.pro"), song(Some(4_000_002), 10)).unwrap();
    let again = r.host.song_index().await.unwrap();
    assert_eq!(
        again
            .iter()
            .find(|e| e.file == "Valid.pro")
            .unwrap()
            .multitracks_id,
        Some(4_000_002)
    );
}

#[tokio::test]
async fn an_import_needs_the_screen_checks_the_number_and_waits_for_the_file() {
    let r = rig(true);
    *r.screen.writes.lock().unwrap() = Some(r.lib("Songs").join("New Song.pro"));
    let file = r
        .host
        .search_import("12345", "New Song", Some("Sunday".into()))
        .await
        .unwrap();
    assert_eq!(file, "New Song.pro");
    assert_eq!(
        r.screen.calls.lock().unwrap().clone(),
        [(
            "12345".to_string(),
            "New Song".to_string(),
            "Sunday".to_string()
        )]
    );
    assert!(r.lib("Songs").join("New Song.pro").is_file());
    // The song is in the index with its number now.
    let index = r.host.song_index().await.unwrap();
    assert_eq!(
        index
            .iter()
            .find(|e| e.file == "New Song.pro")
            .unwrap()
            .multitracks_id,
        Some(12345)
    );
    // A name that is taken, a number that is not digits, a name with a dot: refused before the screen is touched.
    let before = r.screen.calls.lock().unwrap().len();
    assert!(matches!(
        r.host.search_import("12345", "New Song", None).await,
        Err(LinkError::AlreadyExists(_))
    ));
    for (id, name) in [
        ("12a", "X"),
        ("", "X"),
        ("1 2", "X"),
        ("-5", "X"),
        ("12", "A.B"),
        ("12", ""),
    ] {
        assert!(
            matches!(
                r.host.search_import(id, name, None).await,
                Err(LinkError::BadPath(_))
            ),
            "{id:?} {name:?}"
        );
    }
    assert_eq!(r.screen.calls.lock().unwrap().len(), before);
}

#[tokio::test]
async fn an_import_is_refused_without_the_permission_and_reports_the_step_that_stopped() {
    let blocked = rig(false);
    assert_eq!(
        blocked.host.search_import("12", "X", None).await,
        Err(LinkError::CannotImport(
            "Allow StagePilot in Accessibility.".into()
        ))
    );
    assert!(
        blocked.screen.calls.lock().unwrap().is_empty(),
        "the screen was not touched"
    );
    let r = rig(true);
    *r.screen.fails.lock().unwrap() = Some(LinkError::Screen {
        step: "read the search result".into(),
        message: "The search found 2 songs for this number, not exactly one.".into(),
    });
    let error = r.host.search_import("12", "X", None).await.unwrap_err();
    assert!(
        error.words().contains("read the search result")
            && error.words().contains("not exactly one")
    );
    assert!(!r.lib("Songs").join("X.pro").exists());
    // The screen said done but no file came: a plain timeout, not a success.
    let timeout = r.host.search_import("12", "Y", None).await.unwrap_err();
    assert!(matches!(timeout, LinkError::Timeout(_)), "{timeout:?}");
}

#[tokio::test]
async fn selecting_a_library_is_a_best_effort_try() {
    let r = rig(true);
    assert_eq!(
        r.host.select_library("Playback").await.unwrap(),
        SelectOutcome {
            tried: true,
            detail: None
        }
    );
    assert_eq!(r.screen.selected.lock().unwrap().clone(), ["Playback"]);
}

#[test]
fn the_data_folder_is_the_default_or_the_saved_one() {
    assert!(resolve_data_folder(None).ends_with("Documents/ProPresenter"));
    assert!(resolve_data_folder(Some("  ")).ends_with("Documents/ProPresenter"));
    assert_eq!(
        resolve_data_folder(Some("/Volumes/x/ProPresenter")),
        Path::new("/Volumes/x/ProPresenter")
    );
    assert!(resolve_data_folder(Some("~/PP")).ends_with("PP"));
    assert!(!resolve_data_folder(Some("~/PP")).starts_with("~"));
}
