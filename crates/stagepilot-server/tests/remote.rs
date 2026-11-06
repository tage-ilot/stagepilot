//! Rust counterparts of `test_diagnostics_api.py`, `test_remote_feature.py`, the ingress parts of
//! `test_remote_transport.py` / `test_remote_server.py`, and the static-file parts of
//! `test_api.py`. The contract (`scripts/contract`) covers the same routes against the Python
//! app; these run in-process with fakes for the Remote manager and a local mock control plane.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::auth::{csrf_token, RemoteRole, RemoteStore};
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_server::remote_fakes::{FakeBootstrap, FakeManager};
use stagepilot_server::remote_feature::{
    atomic_write, read_desired, write_status, DesiredRemote, RemoteFeature,
};
use stagepilot_server::{
    build_app, AppOptions, ManagerError, RemoteAccess, RemoteIngress, RemoteManager, Runtime,
};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

const PASSWORD: &str = "a-test-password-only";
const ORIGIN: &str = "https://remote.stagepilot.test";

struct Harness {
    local: axum::Router,
    runtime: Arc<Runtime>,
    access: Arc<RemoteAccess>,
}

fn harness(dir: &Path, enforced: bool, web_root: Option<&Path>) -> Harness {
    let service = SettingsService::ephemeral(RuntimeSettings::demo());
    let runtime = Runtime::new(Arc::new(service), enforced);
    let access = RemoteAccess::new(RemoteStore::new(dir.join("identity.db")));
    runtime.set_remote_access(Some(access.clone()));
    let local = build_app(
        runtime.clone(),
        AppOptions {
            web_root: web_root.map(Path::to_path_buf),
        },
    );
    Harness {
        local,
        runtime,
        access,
    }
}

type Reply = (StatusCode, HeaderMap, Vec<u8>);

async fn send(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let mut request = match body {
        Some(b) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        body.collect().await.unwrap().to_bytes().to_vec(),
    )
}

fn json_of(reply: &Reply) -> Value {
    serde_json::from_slice(&reply.2).unwrap_or(Value::Null)
}

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let reply = send(router, method, path, body, headers).await;
    let value = json_of(&reply);
    (reply.0, value)
}

const REMOTE: (&str, &str) = ("x-stagepilot-remote", "1");

// ---- test_remote_feature.py: product bootstrap and intent ----------------------------------------

#[tokio::test]
async fn enable_succeeds_with_zero_operators_on_fresh_install() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(dir.path(), true, None);
    let feature = Arc::new(RemoteFeature::new(dir.path().join("export/remote.json")));
    h.runtime.set_remote_feature(Some(feature));
    let pin = call(
        &h.local,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "1234"})),
        &[],
    )
    .await;
    assert_eq!(pin.0, StatusCode::OK);
    // The login set a session cookie; the guard needs it from here on.
    let cookie = {
        let reply = send(
            &h.local,
            "POST",
            "/api/v1/dashboard-auth/login",
            Some(json!({"pin": "1234"})),
            &[],
        )
        .await;
        let line = reply.1.get(header::SET_COOKIE).unwrap().to_str().unwrap();
        line.split(';').next().unwrap().to_string()
    };
    let auth = [("cookie", cookie.as_str()), REMOTE];
    let status = call(&h.local, "GET", "/api/v1/remote-access", None, &auth).await;
    assert_eq!(status.1["needs_operator"], json!(true));
    let enable = call(
        &h.local,
        "POST",
        "/api/v1/remote-access/enable",
        None,
        &auth,
    )
    .await;
    assert_eq!(enable.0, StatusCode::OK);
    assert_eq!(enable.1["needs_operator"], json!(true));
}

#[tokio::test]
async fn last_enabled_operator_removal_guard_is_unaffected() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteStore::new(dir.path().join("identity.db"));
    let user = store.bootstrap("operator@example.test", PASSWORD).unwrap();
    for result in [
        store.update_user(&user.id, None, None, None, true),
        store.update_user(&user.id, None, Some(false), None, false),
        store.update_user(&user.id, Some(RemoteRole::Viewer), None, None, false),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.status, 409);
        assert!(error.detail.contains("last enabled Operator"));
    }
}

#[tokio::test]
async fn product_bootstrap_and_intent() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(dir.path(), true, None);
    let feature = Arc::new(RemoteFeature::new(dir.path().join("export/remote.json")));
    h.runtime.set_remote_feature(Some(feature.clone()));
    let body = json!({"email": "operator@example.test", "password": PASSWORD});
    // Before the dashboard PIN login the guard refuses everything.
    let refused = call(
        &h.local,
        "POST",
        "/api/v1/remote-access/bootstrap",
        Some(body.clone()),
        &[],
    )
    .await;
    assert_eq!(refused.0, StatusCode::UNAUTHORIZED);
    let reply = send(
        &h.local,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "1234"})),
        &[],
    )
    .await;
    let cookie = reply
        .1
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let plain = [("cookie", cookie.as_str())];
    let auth = [("cookie", cookie.as_str()), REMOTE];
    let ra = "/api/v1/remote-access";
    assert_eq!(
        call(&h.local, "GET", ra, None, &plain).await.1["needs_operator"],
        json!(true)
    );
    // The remote header is required on every mutation.
    assert_eq!(
        call(&h.local, "POST", &format!("{ra}/enable"), None, &plain)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&h.local, "POST", &format!("{ra}/enable"), None, &auth)
            .await
            .0,
        StatusCode::OK
    );
    let boot = |b: Value| {
        call(
            &h.local,
            "POST",
            "/api/v1/remote-access/bootstrap",
            Some(b),
            &auth,
        )
    };
    assert_eq!(boot(body.clone()).await.0, StatusCode::CREATED);
    assert_eq!(boot(body.clone()).await.0, StatusCode::CONFLICT);
    let enable_path = format!("{ra}/enable");
    let enable = || call(&h.local, "POST", &enable_path, None, &auth);
    assert_eq!(enable().await.0, StatusCode::OK);
    let generation = feature.intent().generation;
    assert_eq!(enable().await.0, StatusCode::OK);
    assert_eq!(feature.intent().generation, generation);
    write_status(
        &feature.status_path,
        &json!({
            "available": true, "state": "connected", "url": ORIGIN,
            "generation": generation, "checked_at": now(),
        }),
    )
    .unwrap();
    let status = call(&h.local, "GET", ra, None, &plain).await.1;
    assert_eq!(status["state"], "connected");
    assert_eq!(status["url"], ORIGIN);
    assert!(!status.to_string().contains(dir.path().to_str().unwrap()));
    let off = call(&h.local, "POST", &format!("{ra}/disable"), None, &auth).await;
    assert_eq!(off.1["state"], "off");
    assert!(!read_desired(&feature.control).enabled);
    assert_eq!(
        call(&h.local, "GET", "/api/v1/health/live", None, &plain)
            .await
            .0,
        StatusCode::OK
    );
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[tokio::test]
async fn stale_status_and_foreign_route() {
    let dir = tempfile::tempdir().unwrap();
    let feature = RemoteFeature::new(dir.path().join("remote.json"));
    feature.set_enabled(true).unwrap();
    write_status(
        &feature.status_path,
        &json!({
            "available": true, "state": "connected", "url": ORIGIN,
            "generation": feature.intent().generation, "checked_at": now() - 100.0,
        }),
    )
    .unwrap();
    assert_eq!(feature.status()["url"], Value::Null);
    feature.set_enabled(false).unwrap();
    let foreign = DesiredRemote {
        enabled: true,
        generation: uuid::Uuid::new_v4().to_string(),
        public_origin: ORIGIN.into(),
        ..DesiredRemote::default()
    };
    atomic_write(&feature.control, &serde_json::to_string(&foreign).unwrap()).unwrap();
    assert!(feature.set_enabled(true).is_err());
}

#[tokio::test]
async fn named_marker_survives_ui_disable_and_reports_stable_url() {
    let dir = tempfile::tempdir().unwrap();
    let feature = RemoteFeature::new(dir.path().join("remote.json"));
    let marker = DesiredRemote {
        public_origin: ORIGIN.into(),
        port: 18766,
        ..DesiredRemote::default()
    };
    atomic_write(&feature.control, &serde_json::to_string(&marker).unwrap()).unwrap();
    feature.set_enabled(true).unwrap();
    write_status(
        &feature.status_path,
        &json!({
            "available": true, "state": "connected", "url": ORIGIN,
            "generation": feature.intent().generation, "temporary_url": false,
            "checked_at": now(),
        }),
    )
    .unwrap();
    assert_eq!(feature.status()["url"], ORIGIN);
    assert_eq!(feature.status()["temporary_url"], json!(false));
    feature.set_enabled(false).unwrap();
    let disabled = read_desired(&feature.control);
    assert!(!disabled.enabled);
    assert_eq!(disabled.public_origin, ORIGIN);
    assert_eq!(disabled.port, 18766);
}

#[test]
fn status_rejects_a_url_that_is_not_an_exact_origin() {
    let dir = tempfile::tempdir().unwrap();
    let feature = RemoteFeature::new(dir.path().join("remote.json"));
    feature.set_enabled(true).unwrap();
    for url in [
        "http://remote.test",
        "https://remote.test/path",
        "https://user@remote.test",
        "https://remote.test:8443",
        "https://remote.test?x=1",
        "https://remote.test/",
    ] {
        write_status(
            &feature.status_path,
            &json!({
                "available": true, "state": "connected", "url": url,
                "generation": feature.intent().generation, "checked_at": now(),
            }),
        )
        .unwrap();
        let status = feature.status();
        assert_eq!(status["state"], "error", "{url}");
        assert_eq!(status["url"], Value::Null, "{url}");
    }
}

#[test]
fn enable_error_messages_never_name_infrastructure() {
    use stagepilot_server::remote_routes::enable_error_message;
    let message = enable_error_message("Hostname ownership conflict; reset may be required");
    assert_eq!(
        message,
        "Your previous Remote setup is still registered. Try again in a minute; if it keeps \
         happening, reset Remote Access."
    );
    for word in ["tunnel", "cloudflare", "hostname", "dns"] {
        assert!(!message.to_lowercase().contains(word));
    }
    assert!(enable_error_message("other").contains("could not be enabled"));
    assert!(enable_error_message("enrollment limit reached").contains("enrollment limit"));
    assert!(enable_error_message("Could not reach x").contains("Internet"));
    assert!(enable_error_message("credential was revoked").contains("needs to be reset"));
    assert_eq!(
        enable_error_message("origin not trusted"),
        "Remote Access is unavailable on this installation."
    );
}

// ---- the Remote manager routes (fake) -------------------------------------------------------------

#[tokio::test]
async fn manager_routes_map_errors_to_fixed_texts() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(dir.path(), false, None);
    let manager = Arc::new(FakeManager::default());
    h.runtime.set_remote_manager(Some(manager.clone()));
    let ra = "/api/v1/remote-access";
    let post = |action: &str| {
        let path = format!("{ra}/{action}");
        let router = h.local.clone();
        async move { call(&router, "POST", &path, None, &[REMOTE]).await }
    };
    let status = call(&h.local, "GET", ra, None, &[]).await;
    assert_eq!(status.0, StatusCode::OK);
    assert_eq!(status.1["provisioned"], json!(true));
    assert_eq!(status.1["credential_available"], json!(true));
    assert_eq!(status.1["permanently_revoked"], json!(false));
    assert_eq!(post("enable").await.0, StatusCode::OK);

    *manager.enable_result.lock().unwrap() = Err(ManagerError::Value("x".into()));
    let conflict = post("enable").await;
    assert_eq!(conflict.0, StatusCode::CONFLICT);
    assert_eq!(
        conflict.1["detail"],
        "Remote Access is not provisioned or is already managed."
    );
    *manager.enable_result.lock().unwrap() =
        Err(ManagerError::Provider("Hostname ownership conflict".into()));
    let provider = post("enable").await;
    assert_eq!(provider.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(provider.1["detail"]
        .as_str()
        .unwrap()
        .starts_with("Your previous Remote setup is still registered"));

    *manager.disable_result.lock().unwrap() = Err(ManagerError::Provider("x".into()));
    let disable = post("disable").await;
    assert_eq!(disable.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        disable.1["detail"],
        "Remote public access is closed locally; provider revocation will retry on restart."
    );
    *manager.disable_result.lock().unwrap() = Ok(());
    assert_eq!(post("disable").await.0, StatusCode::OK);

    *manager.regenerate_result.lock().unwrap() = Err(ManagerError::Provider("x".into()));
    let regenerate = post("regenerate").await;
    assert_eq!(regenerate.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        regenerate.1["detail"],
        "Remote link regeneration is unavailable; local StagePilot is unaffected."
    );
    assert_eq!(post("reset").await.0, StatusCode::OK);
    assert!(manager.calls().contains(&"reset_identity".to_string()));
}

#[tokio::test]
async fn reset_and_regenerate_need_manager_support() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(dir.path(), false, None);
    let call_post = |action: &'static str| {
        let router = h.local.clone();
        async move {
            call(
                &router,
                "POST",
                &format!("/api/v1/remote-access/{action}"),
                None,
                &[REMOTE],
            )
            .await
        }
    };
    // No manager at all.
    let reset = call_post("reset").await;
    assert_eq!(reset.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        reset.1["detail"],
        "Resetting the installation identity is unavailable on this installation."
    );
    let regenerate = call_post("regenerate").await;
    assert_eq!(
        regenerate.1["detail"],
        "Regenerating the Remote link is unavailable on this installation."
    );
    // A manager without those capabilities answers the same.
    let manager = FakeManager {
        reset_supported: false,
        regenerate_supported: false,
        ..FakeManager::default()
    };
    h.runtime.set_remote_manager(Some(Arc::new(manager)));
    assert_eq!(call_post("reset").await.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        call_post("regenerate").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // Without a feature either, enable and disable are 503.
    assert_eq!(call_post("enable").await.0, StatusCode::OK);
}

#[tokio::test]
async fn not_configured_without_a_store() {
    let service = SettingsService::ephemeral(RuntimeSettings::demo());
    let runtime = Runtime::new(Arc::new(service), false);
    let router = build_app(runtime, AppOptions { web_root: None });
    for (method, path) in [
        ("GET", "/api/v1/remote-access"),
        ("GET", "/api/v1/remote-auth/users"),
    ] {
        let (status, body) = call(&router, method, path, None, &[]).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["detail"], "Remote access is not configured.");
    }
}

// ---- the remote ingress ---------------------------------------------------------------------------

fn ingress_headers(extra: &[(&'static str, String)]) -> Vec<(String, String)> {
    let mut headers = vec![
        ("origin".to_string(), ORIGIN.to_string()),
        ("x-stagepilot-remote".to_string(), "1".to_string()),
    ];
    headers.extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
    headers
}

async fn edge(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(String, String)],
) -> Reply {
    let borrowed: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    send(router, method, path, body, &borrowed).await
}

struct Edge {
    h: Harness,
    router: axum::Router,
    store_path: std::path::PathBuf,
}

fn edge_harness(dir: &Path) -> Edge {
    let h = harness(dir, false, None);
    let router = RemoteIngress::new(h.access.clone(), ORIGIN)
        .unwrap()
        .wrap(h.local.clone());
    Edge {
        h,
        router,
        store_path: dir.join("identity.db"),
    }
}

fn session_for(edge: &Edge, email: &str) -> (String, String) {
    let token = edge.h.access.store.login(email, PASSWORD, None).unwrap();
    let csrf = csrf_token(&token);
    (token, csrf)
}

#[tokio::test]
async fn ingress_requires_an_exact_https_origin() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(dir.path(), false, None);
    for bad in [
        "http://remote.test",
        "https://remote.test/path",
        "https://remote.test?q=1",
        "https://remote.test#frag",
        "https://user@remote.test",
        "remote.test",
        "",
    ] {
        assert!(RemoteIngress::new(h.access.clone(), bad).is_err(), "{bad}");
    }
    assert!(RemoteIngress::new(h.access.clone(), "https://remote.test").is_ok());
    assert!(RemoteIngress::new(h.access.clone(), "https://remote.test:8443").is_ok());
}

#[tokio::test]
async fn remote_retry_and_viewer_policy() {
    let dir = tempfile::tempdir().unwrap();
    let e = edge_harness(dir.path());
    let store = &e.h.access.store;
    store.bootstrap("operator@example.test", PASSWORD).unwrap();
    store
        .create_user("viewer@example.test", PASSWORD, RemoteRole::Viewer, false)
        .unwrap();
    let cookie_of = |token: &str| format!("__Host-stagepilot_remote={token}");

    // A viewer is read-only, and may not manage Remote.
    let (token, csrf) = session_for(&e, "viewer@example.test");
    let viewer = ingress_headers(&[("cookie", cookie_of(&token)), ("x-csrf-token", csrf)]);
    assert_eq!(
        edge(&e.router, "GET", "/api/v1/remote-access", None, &viewer)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let other = json!({"email": "other@example.test", "password": PASSWORD});
    assert_eq!(
        edge(
            &e.router,
            "POST",
            "/api/v1/remote-access/bootstrap",
            Some(other.clone()),
            &viewer
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // An operator cannot bootstrap from remote, and keyed actions replay.
    let (token, csrf) = session_for(&e, "operator@example.test");
    let operator = ingress_headers(&[
        ("cookie", cookie_of(&token)),
        ("x-csrf-token", csrf.clone()),
    ]);
    assert_eq!(
        edge(
            &e.router,
            "POST",
            "/api/v1/remote-access/bootstrap",
            Some(other),
            &operator
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let keyed = ingress_headers(&[
        ("cookie", cookie_of(&token)),
        ("x-csrf-token", csrf.clone()),
        ("idempotency-key", uuid::Uuid::new_v4().to_string()),
    ]);
    let first = edge(
        &e.router,
        "POST",
        "/api/v1/actions/start_next",
        None,
        &keyed,
    )
    .await;
    let second = edge(
        &e.router,
        "POST",
        "/api/v1/actions/start_next",
        None,
        &keyed,
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(second.0, StatusCode::OK);
    assert_eq!(first.2, second.2);
    assert_eq!(second.1.get(header::CACHE_CONTROL).unwrap(), "no-store");
    // The same key for another path is refused; a second request without a key just runs.
    let reused = edge(
        &e.router,
        "POST",
        "/api/v1/actions/stop_timer",
        None,
        &keyed,
    )
    .await;
    assert_eq!(reused.0, StatusCode::CONFLICT);
    let plain = edge(
        &e.router,
        "POST",
        "/api/v1/actions/stop_timer",
        None,
        &operator,
    )
    .await;
    assert_eq!(plain.0, StatusCode::OK);
    // A malformed key is a 422 and nothing runs.
    let bad = ingress_headers(&[
        ("cookie", cookie_of(&token)),
        ("x-csrf-token", csrf),
        ("idempotency-key", "short".to_string()),
    ]);
    assert_eq!(
        edge(&e.router, "POST", "/api/v1/actions/stop_timer", None, &bad)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // Revoking the session stops everything.
    e.h.access.store.revoke(&token, false).unwrap();
    assert_eq!(
        edge(
            &e.router,
            "POST",
            "/api/v1/actions/start_next",
            None,
            &keyed
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn journal_restart_uncertainty_and_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.db");
    let store = RemoteStore::new(&path);
    let later = now() + 60.0;
    assert!(store
        .claim_action("key", "fingerprint", later)
        .unwrap()
        .is_none());
    let restarted = RemoteStore::new(&path);
    let pending = restarted
        .claim_action("key", "fingerprint", later)
        .unwrap_err();
    assert!(pending.detail.contains("pending or uncertain"));
    let other = restarted.claim_action("key", "other", later).unwrap_err();
    assert!(other.detail.contains("another action"));
    restarted.finish_action("key", 200, b"{}").unwrap();
    assert_eq!(
        store.claim_action("key", "fingerprint", later).unwrap(),
        Some((200, b"{}".to_vec()))
    );
}

#[tokio::test]
async fn login_session_logout_and_cookie_flags() {
    let dir = tempfile::tempdir().unwrap();
    let e = edge_harness(dir.path());
    e.h.access
        .store
        .bootstrap("operator@example.test", PASSWORD)
        .unwrap();
    let body = json!({"email": "operator@example.test", "password": PASSWORD});
    // Same-origin and the remote header are both needed to log in.
    let no_origin = edge(
        &e.router,
        "POST",
        "/api/v1/remote-auth/login",
        Some(body.clone()),
        &[("x-stagepilot-remote".into(), "1".into())],
    )
    .await;
    assert_eq!(no_origin.0, StatusCode::FORBIDDEN);
    let no_header = edge(
        &e.router,
        "POST",
        "/api/v1/remote-auth/login",
        Some(body.clone()),
        &[("origin".into(), ORIGIN.into())],
    )
    .await;
    assert_eq!(
        json_of(&no_header)["detail"],
        "Remote login header required."
    );
    let login = edge(
        &e.router,
        "POST",
        "/api/v1/remote-auth/login",
        Some(body),
        &ingress_headers(&[]),
    )
    .await;
    assert_eq!(login.0, StatusCode::OK);
    assert_eq!(login.1.get(header::CACHE_CONTROL).unwrap(), "no-store");
    let cookie = login.1.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(cookie.starts_with("__Host-stagepilot_remote="));
    for flag in [
        "HttpOnly",
        "Secure",
        "SameSite=strict",
        "Path=/",
        "Max-Age=43200",
    ] {
        assert!(cookie.contains(flag), "{cookie}");
    }
    let token = cookie.split(';').next().unwrap().split_once('=').unwrap().1;
    let reply = json_of(&login);
    assert_eq!(reply["authenticated"], json!(true));
    assert_eq!(reply["csrf_token"], json!(csrf_token(token)));

    let with = ingress_headers(&[("cookie", format!("__Host-stagepilot_remote={token}"))]);
    let session = edge(&e.router, "GET", "/api/v1/remote-auth/session", None, &with).await;
    assert_eq!(session.0, StatusCode::OK);
    let session = json_of(&session);
    assert_eq!(session["user"]["email"], "operator@example.test");
    assert_eq!(session["user"]["role"], "Operator");
    // The remote-only routes refuse local callers.
    for (method, path) in [
        ("POST", "/api/v1/remote-auth/logout"),
        ("GET", "/api/v1/remote-auth/session"),
    ] {
        let local = call(&e.h.local, method, path, None, &[]).await;
        assert_eq!(local.0, StatusCode::FORBIDDEN, "{path}");
    }
    // Logout deletes the cookie and ends the session.
    let out = ingress_headers(&[
        ("cookie", format!("__Host-stagepilot_remote={token}")),
        ("x-csrf-token", csrf_token(token)),
    ]);
    let logout = edge(&e.router, "POST", "/api/v1/remote-auth/logout", None, &out).await;
    assert_eq!(logout.0, StatusCode::NO_CONTENT);
    let cleared = logout.1.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    assert_eq!(
        edge(&e.router, "GET", "/api/v1/remote-auth/session", None, &with)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert!(e.store_path.exists());
}

#[tokio::test]
async fn login_throttle_answers_429_with_retry_after() {
    let dir = tempfile::tempdir().unwrap();
    let e = edge_harness(dir.path());
    e.h.access
        .store
        .bootstrap("operator@example.test", PASSWORD)
        .unwrap();
    let wrong = json!({"email": "operator@example.test", "password": "wrong password!"});
    for _ in 0..5 {
        let miss = edge(
            &e.router,
            "POST",
            "/api/v1/remote-auth/login",
            Some(wrong.clone()),
            &ingress_headers(&[]),
        )
        .await;
        assert_eq!(miss.0, StatusCode::UNAUTHORIZED);
    }
    let blocked = edge(
        &e.router,
        "POST",
        "/api/v1/remote-auth/login",
        Some(wrong),
        &ingress_headers(&[]),
    )
    .await;
    assert_eq!(blocked.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(blocked.1.get(header::RETRY_AFTER).unwrap(), "300");
    assert_eq!(blocked.1.get(header::CACHE_CONTROL).unwrap(), "no-store");
}

#[tokio::test]
async fn ingress_denies_lan_pin_and_unauthenticated_state() {
    let dir = tempfile::tempdir().unwrap();
    let e = edge_harness(dir.path());
    let access = edge(&e.router, "GET", "/api/v1/access", None, &[]).await;
    let access = json_of(&access);
    assert_eq!(access["mode"], "remote");
    assert_eq!(access["authentication"], "password");
    assert_eq!(access["authenticated"], json!(false));
    assert_eq!(access["capabilities"]["canActivateServices"], json!(false));
    for (path, detail) in [
        ("/api/v1/state", "Remote email/password session required."),
        (
            "/api/v1/dashboard-auth/status",
            "LAN PIN authentication is not available remotely.",
        ),
    ] {
        let reply = edge(&e.router, "GET", path, None, &[]).await;
        assert_eq!(json_of(&reply)["detail"], detail, "{path}");
        assert_eq!(reply.1.get(header::CACHE_CONTROL).unwrap(), "no-store");
    }
    // A mutation from another origin, or with two origin headers, is refused.
    let wrong = edge(
        &e.router,
        "POST",
        "/api/v1/remote-auth/login",
        Some(json!({})),
        &[("origin".into(), "https://evil.example".into())],
    )
    .await;
    assert_eq!(
        json_of(&wrong)["detail"],
        "Remote same-origin request required."
    );
    let two = edge(
        &e.router,
        "POST",
        "/api/v1/remote-auth/login",
        Some(json!({})),
        &[
            ("origin".into(), ORIGIN.into()),
            ("origin".into(), ORIGIN.into()),
        ],
    )
    .await;
    assert_eq!(two.0, StatusCode::FORBIDDEN);
    // An oversize auth payload is a 413 before any parsing.
    let huge = json!({"email": "a".repeat(20_000), "password": "x"});
    let big = edge(
        &e.router,
        "POST",
        "/api/v1/remote-auth/login",
        Some(huge),
        &ingress_headers(&[]),
    )
    .await;
    assert_eq!(big.0, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn user_administration_over_the_local_surface() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(dir.path(), false, None);
    let users = "/api/v1/remote-auth/users";
    let created = call(
        &h.local,
        "POST",
        users,
        Some(json!({"email": "Op@Example.Test", "password": PASSWORD, "role": "Operator"})),
        &[],
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED);
    assert_eq!(created.1["email"], "op@example.test");
    let id = created.1["id"].as_str().unwrap().to_string();
    // The first user must be an Operator.
    let first = call(
        &h.local,
        "POST",
        users,
        Some(json!({"email": "v@example.test", "password": PASSWORD, "role": "Viewer"})),
        &[],
    )
    .await;
    assert_eq!(first.0, StatusCode::CREATED);
    // Validation errors never echo the input.
    let invalid = call(
        &h.local,
        "POST",
        users,
        Some(json!({"email": "x@example.test", "password": "short", "role": "Viewer"})),
        &[],
    )
    .await;
    assert_eq!(invalid.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        invalid.1["detail"],
        "Invalid remote authentication request."
    );
    assert!(!invalid.1.to_string().contains("short"));
    // The last enabled Operator is protected.
    let delete = call(&h.local, "DELETE", &format!("{users}/{id}"), None, &[]).await;
    assert_eq!(delete.0, StatusCode::CONFLICT);
    let unknown = call(&h.local, "DELETE", &format!("{users}/nobody"), None, &[]).await;
    assert_eq!(unknown.0, StatusCode::NOT_FOUND);
    let listed = call(&h.local, "GET", users, None, &[]).await.1;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(listed[0]["email"], "op@example.test");
}

// ---- diagnostics ----------------------------------------------------------------------------------

fn diagnostics_runtime(dir: &Path, manager: Option<Arc<FakeManager>>) -> axum::Router {
    let h = harness(dir, false, None);
    h.runtime
        .set_remote_manager(manager.map(|m| m as Arc<dyn RemoteManager>));
    h.local
}

const SEND: &str = "/api/v1/diagnostics/send";

#[tokio::test]
async fn bundle_too_large_returns_413() {
    let dir = tempfile::tempdir().unwrap();
    let manager = FakeManager {
        bootstrap: Some(Arc::new(FakeBootstrap::enrolled("https://control.example"))),
        ..FakeManager::default()
    };
    let router = diagnostics_runtime(dir.path(), Some(Arc::new(manager)));
    let oversized = "x".repeat(2 * 1024 * 1024 + 1);
    let (status, body) = call(
        &router,
        "POST",
        SEND,
        Some(json!({"bundle": oversized})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["detail"], "Diagnostic bundle is too large to send.");
}

fn manager_with(bootstrap: Option<FakeBootstrap>) -> Option<Arc<FakeManager>> {
    Some(Arc::new(FakeManager {
        bootstrap: bootstrap
            .map(|b| Arc::new(b) as Arc<dyn stagepilot_server::DiagnosticsBootstrap>),
        ..FakeManager::default()
    }))
}

#[tokio::test]
async fn unenrolled_and_broken_bootstraps_return_503() {
    let dir = tempfile::tempdir().unwrap();
    let not_set_up = "Sending logs to the developer requires Remote Access to be set up on this \
                      installation.";
    let unavailable = "Sending logs to the developer is unavailable right now.";
    let mut not_enrolled = FakeBootstrap::enrolled("https://control.example");
    not_enrolled.active = None;
    let mut raises = FakeBootstrap::enrolled("https://control.example");
    raises.state_fails = true;
    let mut untrusted = FakeBootstrap::enrolled("https://control.example");
    untrusted.trusted = vec!["https://other.example".into()];
    let cases = [
        // No remote manager at all, then a manager without a bootstrap store.
        (None, not_set_up),
        (manager_with(None), not_set_up),
        (manager_with(Some(not_enrolled)), not_set_up),
        (manager_with(Some(raises)), unavailable),
        (manager_with(Some(untrusted)), unavailable),
    ];
    for (manager, detail) in cases {
        let router = diagnostics_runtime(dir.path(), manager);
        let (status, body) = call(&router, "POST", SEND, Some(json!({"bundle": "log"})), &[]).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["detail"], detail);
    }
}

/// A one-request-at-a-time control plane that records what it was sent.
async fn mock_control(
    status: u16,
) -> (
    String,
    Arc<std::sync::Mutex<Vec<(String, HeaderMap, Vec<u8>)>>>,
) {
    use axum::extract::State;
    use axum::routing::post;
    type Seen = Arc<std::sync::Mutex<Vec<(String, HeaderMap, Vec<u8>)>>>;
    let seen: Seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = axum::Router::new()
        .route(
            "/v1/installations/{id}/diagnostics",
            post(
                move |State(seen): State<Seen>,
                      uri: axum::http::Uri,
                      headers: HeaderMap,
                      body: axum::body::Bytes| async move {
                    seen.lock()
                        .unwrap()
                        .push((uri.path().to_string(), headers, body.to_vec()));
                    (StatusCode::from_u16(status).unwrap(), "{\"ok\":true}")
                },
            ),
        )
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (origin, seen)
}

#[tokio::test]
async fn forwarding_path_statuses() {
    for (upstream, expected) in [(201u16, 200u16), (429, 429), (500, 502)] {
        let dir = tempfile::tempdir().unwrap();
        let (origin, seen) = mock_control(upstream).await;
        let router = diagnostics_runtime(
            dir.path(),
            manager_with(Some(FakeBootstrap::enrolled(&origin))),
        );
        let (status, body) = call(
            &router,
            "POST",
            SEND,
            Some(json!({"bundle": "log contents"})),
            &[],
        )
        .await;
        assert_eq!(status.as_u16(), expected, "upstream {upstream}");
        if expected == 200 {
            assert_eq!(
                body,
                json!({"ok": true, "message": "Logs sent to the developer."})
            );
        }
        let seen = seen.lock().unwrap();
        let (path, headers, content) = &seen[0];
        assert_eq!(path, "/v1/installations/abcd1234/diagnostics");
        assert_eq!(
            headers.get("authorization").unwrap(),
            &format!("Bearer spi_abcd1234.{}", "s".repeat(43))
        );
        assert_eq!(
            headers.get("content-type").unwrap(),
            "application/octet-stream"
        );
        assert!(content.starts_with(b"log contents\n--- playback ---\n"));
    }
}

#[tokio::test]
async fn json_bundles_gain_a_playback_section_and_a_failed_credential_is_a_502() {
    let dir = tempfile::tempdir().unwrap();
    let (origin, seen) = mock_control(201).await;
    let router = diagnostics_runtime(
        dir.path(),
        manager_with(Some(FakeBootstrap::enrolled(&origin))),
    );
    let bundle = json!({"app": "stagepilot"}).to_string();
    let (status, _) = call(&router, "POST", SEND, Some(json!({"bundle": bundle})), &[]).await;
    assert_eq!(status, StatusCode::OK);
    let sent: Value = serde_json::from_slice(&seen.lock().unwrap()[0].2).unwrap();
    assert_eq!(sent["app"], "stagepilot");
    assert!(sent["playback"].get("last_scan").is_some());
    assert!(sent["playback"]["recent_log"].is_array());

    let mut broken = FakeBootstrap::enrolled(&origin);
    broken.credential = Err("keychain".into());
    let router = diagnostics_runtime(dir.path(), manager_with(Some(broken)));
    let (status, body) = call(&router, "POST", SEND, Some(json!({"bundle": "x"})), &[]).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        body["detail"],
        "Logs could not be sent to the developer. Try again shortly."
    );
}

#[tokio::test]
async fn diagnostics_validation_matches_fastapi() {
    let dir = tempfile::tempdir().unwrap();
    let router = diagnostics_runtime(dir.path(), None);
    let missing = call(&router, "POST", SEND, Some(json!({})), &[]).await;
    assert_eq!(missing.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(missing.1["detail"][0]["type"], "missing");
    assert_eq!(missing.1["detail"][0]["loc"], json!(["body", "bundle"]));
    let wrong = call(&router, "POST", SEND, Some(json!({"bundle": 5})), &[]).await;
    assert_eq!(wrong.1["detail"][0]["type"], "string_type");
}

// ---- static files ---------------------------------------------------------------------------------

fn web_root(dir: &Path) -> std::path::PathBuf {
    let root = dir.join("web");
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(root.join("bare")).unwrap();
    std::fs::write(
        root.join("index.html"),
        "<html><script src=\"/assets/dashboard.js\"></script></html>",
    )
    .unwrap();
    std::fs::write(
        root.join("assets/dashboard.js"),
        "window.stagepilot = true;",
    )
    .unwrap();
    std::fs::write(root.join("sub/index.html"), "<p>sub</p>").unwrap();
    std::fs::write(root.join("bare/file.txt"), "text").unwrap();
    root
}

#[tokio::test]
async fn dashboard_and_assets_are_served_and_the_api_still_answers() {
    let dir = tempfile::tempdir().unwrap();
    let root = web_root(dir.path());
    let h = harness(dir.path(), false, Some(&root));
    let dashboard = send(&h.local, "GET", "/", None, &[]).await;
    assert_eq!(dashboard.0, StatusCode::OK);
    assert!(String::from_utf8_lossy(&dashboard.2).contains("src=\"/assets/dashboard.js\""));
    assert_eq!(
        dashboard.1.get(header::CONTENT_TYPE).unwrap(),
        "text/html; charset=utf-8"
    );
    assert_eq!(dashboard.1.get(header::ACCEPT_RANGES).unwrap(), "bytes");
    let asset = send(&h.local, "GET", "/assets/dashboard.js", None, &[]).await;
    assert_eq!(asset.2, b"window.stagepilot = true;");
    assert_eq!(
        asset.1.get(header::CONTENT_TYPE).unwrap(),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        send(&h.local, "GET", "/api/v1/health/live", None, &[])
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn static_404_405_redirects_and_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let root = web_root(dir.path());
    let h = harness(dir.path(), false, Some(&root));
    let missing = [
        "/nope",
        "/dashboard/settings",
        "/assets/",
        "/assets",
        "/bare/",
        "/%2e%2e/%2e%2e/etc/passwd",
        "/../../etc/passwd",
        "/index.html%00",
    ];
    for path in missing {
        let (status, body) = call(&h.local, "GET", path, None, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body, json!({"detail": "Not Found"}), "{path}");
    }
    for method in ["POST", "PUT", "DELETE"] {
        let (status, body) = call(&h.local, method, "/index.html", None, &[]).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(body, json!({"detail": "Method Not Allowed"}));
    }
    // A directory with an index redirects to the slash form.
    let redirect = send(&h.local, "GET", "/sub", None, &[("host", "dash.local")]).await;
    assert_eq!(redirect.0, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        redirect.1.get(header::LOCATION).unwrap(),
        "http://dash.local/sub/"
    );
    assert_eq!(
        send(&h.local, "GET", "/sub/", None, &[]).await.2,
        b"<p>sub</p>"
    );
    // Dot segments inside the root collapse.
    assert_eq!(
        send(&h.local, "GET", "/assets/../index.html", None, &[])
            .await
            .0,
        StatusCode::OK
    );
    // HEAD has the headers but no body.
    let head = send(&h.local, "HEAD", "/assets/dashboard.js", None, &[]).await;
    assert_eq!(head.0, StatusCode::OK);
    assert!(head.2.is_empty());
    assert_eq!(head.1.get(header::CONTENT_LENGTH).unwrap(), "25");
}

#[tokio::test]
async fn static_validators_and_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let root = web_root(dir.path());
    let h = harness(dir.path(), false, Some(&root));
    let first = send(&h.local, "GET", "/assets/dashboard.js", None, &[]).await;
    let etag = first
        .1
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let modified = first
        .1
        .get(header::LAST_MODIFIED)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let get = |headers: Vec<(&'static str, String)>| {
        let router = h.local.clone();
        async move {
            let borrowed: Vec<(&str, &str)> =
                headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            send(&router, "GET", "/assets/dashboard.js", None, &borrowed).await
        }
    };
    let by_etag = get(vec![("if-none-match", format!("W/{etag}"))]).await;
    assert_eq!(by_etag.0, StatusCode::NOT_MODIFIED);
    assert!(by_etag.2.is_empty());
    assert_eq!(
        get(vec![("if-none-match", "\"0\"".into())]).await.0,
        StatusCode::OK
    );
    assert_eq!(
        get(vec![("if-modified-since", modified)]).await.0,
        StatusCode::NOT_MODIFIED
    );
    let part = get(vec![("range", "bytes=0-5".into())]).await;
    assert_eq!(part.0, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.2, b"window");
    assert_eq!(part.1.get(header::CONTENT_RANGE).unwrap(), "bytes 0-5/25");
    let suffix = get(vec![("range", "bytes=-5".into())]).await;
    assert_eq!(suffix.2, b"true;");
    let open = get(vec![("range", "bytes=20-".into())]).await;
    assert_eq!(open.2, b"true;");
    let unsatisfiable = get(vec![("range", "bytes=99-".into())]).await;
    assert_eq!(unsatisfiable.0, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        unsatisfiable.1.get(header::CONTENT_RANGE).unwrap(),
        "bytes */25"
    );
    let units = get(vec![("range", "lines=1-2".into())]).await;
    assert_eq!(units.0, StatusCode::BAD_REQUEST);
    assert_eq!(units.2, b"Only support bytes range");
    let stale = get(vec![
        ("range", "bytes=0-3".into()),
        ("if-range", "\"0\"".into()),
    ])
    .await;
    assert_eq!(stale.0, StatusCode::OK);
    assert_eq!(stale.2.len(), 25);
    let matching = get(vec![("range", "bytes=0-3".into()), ("if-range", etag)]).await;
    assert_eq!(matching.0, StatusCode::PARTIAL_CONTENT);
}

#[tokio::test]
async fn a_404_html_page_is_served_with_status_404() {
    let dir = tempfile::tempdir().unwrap();
    let root = web_root(dir.path());
    std::fs::write(root.join("404.html"), "<h1>gone</h1>").unwrap();
    let h = harness(dir.path(), false, Some(&root));
    let reply = send(&h.local, "GET", "/nothing", None, &[]).await;
    assert_eq!(reply.0, StatusCode::NOT_FOUND);
    assert_eq!(reply.2, b"<h1>gone</h1>");
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_out_of_the_root_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let root = web_root(dir.path());
    std::fs::write(dir.path().join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link.txt")).unwrap();
    let h = harness(dir.path(), false, Some(&root));
    let (status, _) = call(&h.local, "GET", "/link.txt", None, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
