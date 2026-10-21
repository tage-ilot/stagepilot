//! Auth compatibility with hashes produced by the Python app (`argon2-cffi`, PBKDF2 PIN hash),
//! sessions, lockout and the remote store.

use serde_json::Value;
use stagepilot_core::auth::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn hashes() -> Value {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/auth_hashes.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap().to_string()
}

#[test]
fn verifies_an_argon2id_hash_made_by_argon2_cffi() {
    let h = hashes();
    assert!(text(&h, "argon2_hash").starts_with("$argon2id$v=19$m=65536,t=3,p=1$"));
    assert!(verify_password(
        &text(&h, "argon2_password"),
        &text(&h, "argon2_hash")
    ));
    assert!(verify_password(
        &text(&h, "argon2_other_password"),
        &text(&h, "argon2_hash_other")
    ));
}

#[test]
fn rejects_a_wrong_password_and_a_mixed_up_hash() {
    let h = hashes();
    assert!(!verify_password("wrong", &text(&h, "argon2_hash")));
    assert!(!verify_password(
        &text(&h, "argon2_password"),
        &text(&h, "argon2_hash_other")
    ));
    assert!(!verify_password("x", "not a hash"));
    assert!(!verify_password("x", ""));
}

#[test]
fn rust_argon2_hashes_use_the_apps_parameters_and_need_no_rehash() {
    let encoded = hash_password("synthetic passphrase 1").unwrap();
    assert!(encoded.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"));
    assert!(verify_password("synthetic passphrase 1", &encoded));
    assert!(!needs_rehash(&encoded));
    assert!(needs_rehash("$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"));
}

#[test]
fn verifies_the_pbkdf2_pin_hash_the_python_app_wrote() {
    let h = hashes();
    assert!(verify_dashboard_pin(
        &text(&h, "pin"),
        Some(&text(&h, "pin_hash"))
    ));
    assert!(!verify_dashboard_pin("0000", Some(&text(&h, "pin_hash"))));
}

#[test]
fn pin_hash_round_trips_and_uses_a_fresh_salt() {
    let a = hash_dashboard_pin("8642");
    let b = hash_dashboard_pin("8642");
    assert_ne!(a, b);
    assert!(a.starts_with("pbkdf2_sha256$210000$"));
    assert!(verify_dashboard_pin("8642", Some(&a)));
    assert!(!verify_dashboard_pin("8643", Some(&a)));
}

#[test]
fn malformed_pin_hashes_never_verify() {
    for bad in [
        "",
        "pbkdf2_sha256",
        "md5$1$a$b",
        "pbkdf2_sha256$x$a$b",
        "pbkdf2_sha256$0$AAAA$BBBB",
    ] {
        assert!(!verify_dashboard_pin("1234", Some(bad)), "{bad}");
    }
}

#[test]
fn unset_hash_means_the_default_pin() {
    assert!(verify_dashboard_pin("1234", None));
    assert!(!verify_dashboard_pin("1235", None));
    assert!(!verify_dashboard_pin("", None));
}

#[test]
fn dashboard_sessions_expire_clear_and_reject_unknown_tokens() {
    let sessions = DashboardSessionStore::new();
    let token = sessions.create();
    assert!(token.len() >= 43);
    assert!(sessions.valid(Some(&token)));
    assert!(!sessions.valid(Some("nope")));
    assert!(!sessions.valid(None));
    assert!(!sessions.valid(Some("")));
    let short = sessions.create_with_ttl(Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(10));
    assert!(!sessions.valid(Some(&short)));
    sessions.clear();
    assert!(!sessions.valid(Some(&token)));
}

#[test]
fn session_tokens_are_unique() {
    let sessions = DashboardSessionStore::new();
    let a = sessions.create();
    let b = sessions.create();
    assert_ne!(a, b);
}

fn store(dir: &tempfile::TempDir) -> (RemoteStore, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(1_000_000));
    let clock = now.clone();
    let store = RemoteStore::with_clock(
        dir.path().join("identity.sqlite3"),
        Box::new(move || clock.load(Ordering::SeqCst) as f64),
    );
    (store, now)
}

#[test]
fn remote_login_creates_a_session_and_logout_revokes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    store
        .bootstrap("Operator@Example.test", "a synthetic passphrase")
        .unwrap();
    let token = store
        .login("operator@example.test", "a synthetic passphrase", None)
        .unwrap();
    let session = store.session(Some(&token)).unwrap().unwrap();
    assert_eq!(session.email, "operator@example.test");
    assert_eq!(session.role, RemoteRole::Operator);
    store.revoke(&token, false).unwrap();
    assert!(store.session(Some(&token)).unwrap().is_none());
}

#[test]
fn remote_sessions_expire_after_twelve_hours() {
    let dir = tempfile::tempdir().unwrap();
    let (store, now) = store(&dir);
    store
        .bootstrap("op@example.test", "a synthetic passphrase")
        .unwrap();
    let token = store
        .login("op@example.test", "a synthetic passphrase", None)
        .unwrap();
    now.fetch_add(12 * 3600 + 1, Ordering::SeqCst);
    assert!(store.session(Some(&token)).unwrap().is_none());
}

#[test]
fn wrong_password_and_unknown_user_give_the_same_401() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    store
        .bootstrap("op@example.test", "a synthetic passphrase")
        .unwrap();
    let wrong = store.login("op@example.test", "nope", None).unwrap_err();
    let unknown = store.login("who@example.test", "nope", None).unwrap_err();
    assert_eq!(
        (wrong.status, wrong.detail.clone()),
        (401, "Incorrect email or password.".into())
    );
    assert_eq!(wrong, unknown);
}

#[test]
fn the_sixth_attempt_for_one_email_is_locked_out_even_with_the_right_password() {
    let dir = tempfile::tempdir().unwrap();
    let (store, now) = store(&dir);
    store
        .bootstrap("op@example.test", "a synthetic passphrase")
        .unwrap();
    for _ in 0..5 {
        assert_eq!(
            store
                .login("op@example.test", "bad", None)
                .unwrap_err()
                .status,
            401
        );
    }
    let locked = store
        .login("op@example.test", "a synthetic passphrase", None)
        .unwrap_err();
    assert_eq!(locked.status, 429);
    assert_eq!(locked.detail, "Too many login attempts. Try again later.");
    // The window is 300 s; after it the right password works again.
    now.fetch_add(301, Ordering::SeqCst);
    assert!(store
        .login("op@example.test", "a synthetic passphrase", None)
        .is_ok());
}

#[test]
fn operator_reset_clears_the_throttle() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    store
        .bootstrap("op@example.test", "a synthetic passphrase")
        .unwrap();
    for _ in 0..5 {
        let _ = store.login("op@example.test", "bad", None);
    }
    store.reset_login_attempts().unwrap();
    store.reset_login_attempts().unwrap();
    assert!(store
        .login("op@example.test", "a synthetic passphrase", None)
        .is_ok());
}

#[test]
fn the_last_enabled_operator_cannot_be_removed_or_demoted() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    let op = store
        .bootstrap("op@example.test", "a synthetic passphrase")
        .unwrap();
    let delete = store.update_user(&op.id, None, None, None, true);
    assert!(delete.is_err());
    let demote = store.update_user(&op.id, Some(RemoteRole::Viewer), None, None, false);
    assert!(demote.is_err());
    assert_eq!(store.users().unwrap().len(), 1);
}

#[test]
fn csrf_tokens_are_stable_per_session_token_and_differ_between_tokens() {
    assert_eq!(csrf_token("abc"), csrf_token("abc"));
    assert_ne!(csrf_token("abc"), csrf_token("abd"));
}

#[test]
fn email_normalisation_rejects_garbage() {
    assert_eq!(normalize_email(" A@B.test ").unwrap(), "a@b.test");
    assert!(normalize_email("no-at-sign").is_err());
}
