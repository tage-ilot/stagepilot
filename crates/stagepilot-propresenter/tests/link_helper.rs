//! `HelperHost` against the REAL helper program (`assets/propresenter-link/propresenter_link.py`) running on this computer on a
//! temporary ProPresenter folder: the signing, the 401 and 429 answers, and every call of the trait through the wire. Skipped when
//! there is no `python3` or on a computer that is not Unix. No ProPresenter, no screen (the helper says it cannot import).

#![cfg(unix)]

use sha2::{Digest, Sha256};
use stagepilot_core::settings::{CredentialStore, MemoryCredentialStore};
use stagepilot_propresenter::link::helper::HelperHost;
use stagepilot_propresenter::link::{LinkError, ProPresenterHost};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const KEY: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";

struct Helper {
    child: Child,
    port: u16,
    dir: tempfile::TempDir,
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Helper {
    fn lib(&self, name: &str) -> PathBuf {
        self.dir.path().join("ProPresenter/Libraries").join(name)
    }
    fn address(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

fn start(allow_loopback: bool) -> Option<Helper> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets/propresenter-link/propresenter_link.py");
    let dir = tempfile::tempdir().unwrap();
    for lib in ["Songs", "Playback"] {
        std::fs::create_dir_all(dir.path().join("ProPresenter/Libraries").join(lib)).unwrap();
    }
    let port = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let config = dir.path().join("config.json");
    std::fs::write(
        &config,
        serde_json::json!({
            "key": KEY, "bind": "127.0.0.1", "port": port, "allow_loopback": allow_loopback,
            "data_folder": dir.path().join("ProPresenter"), "log_file": dir.path().join("link.log"),
        })
        .to_string(),
    )
    .unwrap();
    let child = Command::new("python3")
        .arg(&script)
        .arg("run")
        .env("PROPRESENTER_LINK_CONFIG", &config)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let helper = Helper { child, port, dir };
    for _ in 0..100 {
        if std::net::TcpStream::connect_timeout(
            &([127, 0, 0, 1], port).into(),
            Duration::from_millis(100),
        )
        .is_ok()
        {
            return Some(helper);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn keys(key: &str) -> Arc<dyn CredentialStore> {
    let store = MemoryCredentialStore::default();
    store.set_secret(key).unwrap();
    Arc::new(store)
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn song(id: u64) -> Vec<u8> {
    // field 1 (some text), field 21 { field 1 = id }
    let mut out = vec![0x0a, 2, b'h', b'i', 0xaa, 0x01];
    let mut record = vec![0x08];
    let mut v = id;
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            record.push(byte);
            break;
        }
        record.push(byte | 0x80);
    }
    out.push(record.len() as u8);
    out.extend(record);
    out
}

macro_rules! helper_or_skip {
    ($allow:expr) => {
        match start($allow) {
            Some(h) => h,
            None => {
                eprintln!("skipped: no python3 on this computer");
                return;
            }
        }
    };
}

#[tokio::test]
async fn every_call_of_the_trait_works_through_the_real_helper() {
    let helper = helper_or_skip!(true);
    let host = HelperHost::new(&helper.address(), keys(KEY));
    let info = host.info().await.unwrap();
    assert_eq!(
        info.version,
        stagepilot_propresenter::helper_assets::BUNDLED_HELPER_VERSION
    );
    assert_eq!(info.libraries, ["Playback", "Songs"]);
    assert!(info.data_folder_exists);
    assert!(!info.can_import, "this is not a Mac");
    assert!(info.import_blocker.unwrap().contains("Mac"));

    std::fs::write(helper.lib("Songs").join("Original (1).pro"), song(4242)).unwrap();
    let listed = host.list_library("Songs").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Original (1).pro");
    assert_eq!(
        host.read_file("Songs", "Original (1).pro").await.unwrap(),
        song(4242)
    );

    let copy = song(4243);
    host.create_file("Playback", "Original (2).pro", &copy, &hash(&copy))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(helper.lib("Playback").join("Original (2).pro")).unwrap(),
        copy
    );
    assert!(matches!(
        host.create_file("Playback", "Original (2).pro", &copy, &hash(&copy))
            .await,
        Err(LinkError::AlreadyExists(_))
    ));
    assert_eq!(
        host.create_file("Playback", "Bad.pro", &copy, &hash(b"other"))
            .await,
        Err(LinkError::HashMismatch)
    );
    assert!(!helper.lib("Playback").join("Bad.pro").exists());
    assert!(matches!(
        host.create_file("Songs", "X.pro", &copy, &hash(&copy))
            .await,
        Err(LinkError::NotAllowed(_))
    ));

    let index = host.song_index().await.unwrap();
    let got: Vec<_> = index
        .iter()
        .map(|e| (e.library.as_str(), e.file.as_str(), e.multitracks_id))
        .collect();
    assert_eq!(
        got,
        [
            ("Playback", "Original (2).pro", Some(4243)),
            ("Songs", "Original (1).pro", Some(4242))
        ]
    );

    assert!(host.ensure_library("Sunday").await.unwrap());
    assert!(!host.ensure_library("Sunday").await.unwrap());
    assert!(host.ensure_library("Songs").await.is_err());
    assert!(matches!(
        host.list_library("Nope").await,
        Err(LinkError::NotFound(_))
    ));
    for bad in ["..", "a/b", "%2e%2e"] {
        assert!(host.list_library(bad).await.is_err(), "{bad}");
    }
    assert!(matches!(
        host.search_import("12345", "New Song", None).await,
        Err(LinkError::CannotImport(_))
    ));
    assert!(!host.select_library("Playback").await.unwrap().tried);
}

#[tokio::test]
async fn a_wrong_key_is_refused_and_ten_bad_requests_lock_the_address_for_a_minute() {
    let helper = helper_or_skip!(true);
    let wrong = HelperHost::new(&helper.address(), keys(&"Z".repeat(43)));
    for _ in 0..9 {
        assert_eq!(wrong.info().await, Err(LinkError::Refused(401)));
    }
    assert_eq!(wrong.info().await, Err(LinkError::Refused(401)));
    assert_eq!(
        wrong.info().await,
        Err(LinkError::Refused(429)),
        "the eleventh is 429"
    );
    // Even the right key waits out the block from the same address.
    let right = HelperHost::new(&helper.address(), keys(KEY));
    assert_eq!(right.info().await, Err(LinkError::Refused(429)));
}

#[tokio::test]
async fn an_old_or_future_time_stamp_is_refused() {
    let helper = helper_or_skip!(true);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let late =
        HelperHost::new(&helper.address(), keys(KEY)).with_clock(Box::new(move || now - 1000));
    assert_eq!(late.info().await, Err(LinkError::Refused(401)));
    let early =
        HelperHost::new(&helper.address(), keys(KEY)).with_clock(Box::new(move || now + 1000));
    assert_eq!(early.info().await, Err(LinkError::Refused(401)));
    let fine =
        HelperHost::new(&helper.address(), keys(KEY)).with_clock(Box::new(move || now + 100));
    assert!(fine.info().await.is_ok(), "inside the 120 second window");
}

#[tokio::test]
async fn loopback_is_refused_unless_the_installer_said_so() {
    let helper = helper_or_skip!(false);
    let host = HelperHost::new(&helper.address(), keys(KEY));
    assert_eq!(host.info().await, Err(LinkError::Refused(403)));
}

#[tokio::test]
async fn no_key_at_all_gets_the_bare_answer_and_nothing_else() {
    let helper = helper_or_skip!(true);
    let none = HelperHost::new(
        &helper.address(),
        Arc::new(MemoryCredentialStore::default()),
    );
    let health = none.health().await.unwrap();
    assert_eq!(health.auth.as_deref(), Some("required"));
    assert!(health.data_folder.is_none() && health.libraries.is_empty());
    assert_eq!(none.info().await, Err(LinkError::Refused(401)));
    assert_eq!(
        none.list_library("Songs").await,
        Err(LinkError::Refused(401))
    );
}

#[tokio::test]
async fn a_computer_that_is_not_on_this_network_is_never_asked() {
    let host = HelperHost::new("http://203.0.113.9:8790", keys(KEY));
    assert!(matches!(host.info().await, Err(LinkError::NotAllowed(_))));
    let nothing = HelperHost::new("", keys(KEY));
    assert!(matches!(nothing.info().await, Err(LinkError::BadPath(_))));
}
