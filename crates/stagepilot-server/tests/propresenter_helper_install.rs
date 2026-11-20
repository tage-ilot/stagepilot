//! "Set up the ProPresenter computer": the HTTP surface (session routes, token-guarded files, the key that
//! rides inside the served script and nowhere else), the remote-tunnel exclusion, the session that ends
//! when the helper answers, and the generated wrapper run by a real `sh` and `curl` against a real
//! listener with a stand-in installer.

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::auth::{csrf_token, RemoteStore};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_propresenter::helper_assets as bundled;
use stagepilot_server::helper_install::MAX_REQUESTS;
use stagepilot_server::propresenter_helper_install::{
    NOT_NEEDED, NO_ADDRESS, NO_PROPRESENTER_COMPUTER,
};
// Only the tests that run the generated script with a real `sh` and `curl` (Unix) use these.
use stagepilot_propresenter::link::helper::sign;
#[cfg(unix)]
use stagepilot_server::propresenter_helper_install::{
    served_routes, Assets, ProPresenterHelperInstall,
};
#[cfg(unix)]
use stagepilot_server::propresenter_link::ProPresenterLink;
use stagepilot_server::remote_types::RemoteContext;
use stagepilot_server::{
    build_app, AppOptions, OutputBackends, RemoteAccess, RemoteIngress, Runtime,
};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const LAN: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 20);
const ORIGIN: &str = "https://remote.stagepilot.test";
const PASSWORD: &str = "a-test-password-only";
const KEY: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

async fn send(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    peer: &str,
) -> Reply {
    let builder = Request::builder().method(method).uri(path);
    let mut request = match body {
        Some(b) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let peer: SocketAddr = format!("{peer}:50000").parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body: body.collect().await.unwrap().to_bytes().to_vec(),
    }
}

fn runtime() -> Arc<Runtime> {
    let service = SettingsService::ephemeral_persistent(PersistentSettings::default());
    Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi())
}

/// The ProPresenter output is chosen and this is its host.
fn save_pp_host(rt: &Runtime, host: &str) {
    let mut next = rt.settings_service.snapshot();
    next.integration_modes.timer_output = stagepilot_core::config::TimerOutput::Propresenter;
    next.propresenter.host = host.to_string();
    rt.settings_service.save(next).unwrap();
}

/// An app whose session is already open at `LAN:8000`, a key made, and the token.
fn app_with_session() -> (axum::Router, Arc<Runtime>, String) {
    let rt = runtime();
    rt.propresenter_link.set_key_for_test(KEY);
    let info = rt
        .propresenter_helper_install
        .begin(LAN, 8000, None)
        .unwrap();
    (
        build_app(rt.clone(), AppOptions { web_root: None }),
        rt,
        info.token,
    )
}

#[tokio::test]
async fn an_unknown_token_is_404_with_an_empty_body() {
    let (app, _rt, _token) = app_with_session();
    for path in [
        "/propresenter-helper/0123456789abcdef0123456789abcdef/install.sh",
        "/propresenter-helper/0123456789abcdef0123456789abcdef/files/install.sh",
        "/propresenter-helper/x/files/SHA256SUMS",
    ] {
        let reply = send(&app, "GET", path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert!(reply.body.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn an_expired_or_ended_session_is_404() {
    let (app, rt, token) = app_with_session();
    let path = format!("/propresenter-helper/{token}/install.sh");
    assert_eq!(
        send(&app, "GET", &path, None, "192.168.1.50").await.status,
        StatusCode::OK
    );
    rt.propresenter_helper_install
        .set_clock(Box::new(|| u64::MAX / 2));
    let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert!(reply.body.is_empty());

    let (app, rt, token) = app_with_session();
    assert!(rt.propresenter_helper_install.end());
    let path = format!("/propresenter-helper/{token}/files/install.sh");
    assert_eq!(
        send(&app, "GET", &path, None, "192.168.1.50").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_public_source_address_is_403_even_with_the_right_token() {
    let (app, _rt, token) = app_with_session();
    for peer in ["8.8.8.8", "203.0.113.9"] {
        for path in [
            format!("/propresenter-helper/{token}/install.sh"),
            format!("/propresenter-helper/{token}/files/install.sh"),
        ] {
            let reply = send(&app, "GET", &path, None, peer).await;
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{peer} {path}");
            assert!(reply.body.is_empty());
            assert!(!reply.text().contains(KEY));
        }
    }
    for peer in [
        "192.168.1.50",
        "10.1.2.3",
        "172.16.4.4",
        "169.254.7.7",
        "127.0.0.1",
    ] {
        let path = format!("/propresenter-helper/{token}/files/propresenter_link.py");
        assert_eq!(
            send(&app, "GET", &path, None, peer).await.status,
            StatusCode::OK,
            "{peer}"
        );
    }
}

#[tokio::test]
async fn the_31st_request_for_a_token_is_404() {
    let (app, _rt, token) = app_with_session();
    let path = format!("/propresenter-helper/{token}/files/SHA256SUMS");
    for n in 1..=MAX_REQUESTS {
        let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::OK, "request {n}");
    }
    let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert!(reply.body.is_empty());
}

#[tokio::test]
async fn exactly_the_three_bundled_files_are_served_with_their_content_types() {
    let (app, _rt, token) = app_with_session();
    let expected = [
        ("install.sh", "text/x-shellscript; charset=utf-8"),
        ("propresenter_link.py", "text/x-python; charset=utf-8"),
        ("SHA256SUMS", "text/plain; charset=utf-8"),
    ];
    for (name, content_type) in expected {
        let reply = send(
            &app,
            "GET",
            &format!("/propresenter-helper/{token}/files/{name}"),
            None,
            "192.168.1.50",
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{name}");
        assert_eq!(reply.headers[header::CONTENT_TYPE], content_type, "{name}");
        assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(reply.body, bundled::file(name).unwrap(), "{name}");
        assert!(
            !reply.text().contains(KEY),
            "{name}: a file never holds the key"
        );
    }
    for name in [
        "uninstall.sh",
        "config.json",
        "..%2Finstall.sh",
        "install.sh%00",
        "",
        "INSTALL.SH",
        "propresenter_link.pyc",
    ] {
        let reply = send(
            &app,
            "GET",
            &format!("/propresenter-helper/{token}/files/{name}"),
            None,
            "192.168.1.50",
        )
        .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{name:?}");
        assert!(reply.body.is_empty(), "{name:?}");
    }
}

#[tokio::test]
async fn there_is_no_directory_listing_and_no_other_method() {
    let (app, _rt, token) = app_with_session();
    for path in [
        "/propresenter-helper".to_string(),
        "/propresenter-helper/".to_string(),
        format!("/propresenter-helper/{token}"),
        format!("/propresenter-helper/{token}/"),
        format!("/propresenter-helper/{token}/files"),
        format!("/propresenter-helper/{token}/files/"),
        format!("/propresenter-helper/{token}/files/../../etc/passwd"),
    ] {
        let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert!(reply.body.is_empty(), "{path}");
    }
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        let path = format!("/propresenter-helper/{token}/install.sh");
        let reply = send(&app, method, &path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{method}");
        assert!(reply.body.is_empty(), "{method}");
    }
}

#[tokio::test]
async fn the_served_wrapper_names_the_session_url_the_checksums_and_carries_the_key() {
    let (app, rt, token) = app_with_session();
    let reply = send(
        &app,
        "GET",
        &format!("/propresenter-helper/{token}/install.sh"),
        None,
        "192.168.1.50",
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "text/x-shellscript; charset=utf-8"
    );
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
    let script = reply.text();
    assert!(script.contains(&format!(
        "BASE='http://192.168.1.20:8000/propresenter-helper/{token}'"
    )));
    for name in bundled::FILE_NAMES {
        let want = hex::encode(Sha256::digest(bundled::file(name).unwrap()));
        assert!(
            script.contains(&format!("{name}) want='{want}' ;;")),
            "{name}"
        );
    }
    assert!(script.contains("--key-file"));
    assert!(
        script.contains(KEY),
        "the key rides inside the served script"
    );
    // The stored key became unusable while the session was open: there is nothing to install any more.
    rt.propresenter_link.set_key_for_test("short");
    let again = send(
        &app,
        "GET",
        &format!("/propresenter-helper/{token}/install.sh"),
        None,
        "192.168.1.50",
    )
    .await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
    assert!(again.body.is_empty());
}

// ---- the session routes ---------------------------------------------------------------------------

#[tokio::test]
async fn a_bad_address_is_refused_and_a_missing_propresenter_computer_gets_its_own_sentence() {
    let rt = runtime();
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let bad = send(
        &app,
        "POST",
        "/api/v1/propresenter-link/helper-install",
        Some(json!({"address": "8.8.8.8"})),
        "127.0.0.1",
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    // No ProPresenter computer is saved: nothing to find an address toward.
    let none = send(
        &app,
        "POST",
        "/api/v1/propresenter-link/helper-install",
        None,
        "127.0.0.1",
    )
    .await;
    assert_eq!(none.status, StatusCode::CONFLICT);
    assert_eq!(none.json()["detail"], NO_PROPRESENTER_COMPUTER);
    assert_eq!(none.json()["code"], "no_propresenter_computer");
    // A name that does not resolve has no address to find: that is the one plain sentence.
    save_pp_host(&rt, "propresenter-mac.invalid");
    let no_lan = send(
        &app,
        "POST",
        "/api/v1/propresenter-link/helper-install",
        None,
        "127.0.0.1",
    )
    .await;
    assert_eq!(no_lan.status, StatusCode::CONFLICT);
    assert_eq!(no_lan.json()["detail"], NO_ADDRESS);
    assert_eq!(no_lan.json()["code"], "no_lan_address");
    assert!(
        !rt.propresenter_link.is_paired(),
        "a refused session makes no key"
    );
    // The saved ProPresenter computer is this computer (loopback, or one of its own addresses): there is nothing to set up, no
    // session is opened and no key is made.
    let mut this_computer = vec!["127.0.0.1".to_string(), "localhost".to_string()];
    this_computer.extend(own_private_address().map(|ip| ip.to_string()));
    for host in this_computer {
        save_pp_host(&rt, &host);
        let same = send(
            &app,
            "POST",
            "/api/v1/propresenter-link/helper-install",
            None,
            "127.0.0.1",
        )
        .await;
        assert_eq!(same.status, StatusCode::CONFLICT, "{host}: {}", same.text());
        assert_eq!(same.json()["code"], "not_needed", "{host}");
        assert_eq!(same.json()["detail"], NOT_NEEDED, "{host}");
        assert!(!rt.propresenter_link.is_paired(), "{host}");
        assert!(!rt.propresenter_helper_install.is_active(), "{host}");
    }
}

/// This computer's own private address, as the system would pick it toward the internet.
fn own_private_address() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if ip.is_private() => Some(ip),
        _ => None,
    }
}

#[tokio::test]
async fn post_opens_a_session_makes_the_key_once_extends_it_and_delete_closes_it() {
    let Some(ip) = own_private_address() else {
        eprintln!("skipped: this computer has no private LAN address");
        return;
    };
    let rt = runtime();
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    save_pp_host(&rt, "10.30.0.141");
    assert!(!rt.propresenter_link.is_paired());
    let body = Some(json!({"address": ip.to_string()}));
    let first = send(
        &app,
        "POST",
        "/api/v1/propresenter-link/helper-install",
        body.clone(),
        "127.0.0.1",
    )
    .await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.json());
    assert!(
        rt.propresenter_link.is_paired(),
        "the first session makes the key"
    );
    let first = first.json();
    let port = first["port"].as_u64().unwrap() as u16;
    assert_ne!(
        port, rt.settings.bind_port,
        "the extra listener has its own port"
    );
    assert_eq!(first["lan_ip"], ip.to_string());
    assert_eq!(first["bundled_version"], bundled::BUNDLED_HELPER_VERSION);
    let url = first["url"].as_str().unwrap().to_string();
    assert!(url.starts_with(&format!("http://{ip}:{port}/propresenter-helper/")));
    assert!(url.ends_with("/install.sh"));
    assert_eq!(first["command"], format!("curl -fsS {url} | sh"));
    assert!(first["expires_at"].as_str().unwrap().ends_with('Z'));
    // Nothing in the reply but the one-time address: no key, in any field.
    assert_eq!(
        first
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "bundled_version",
            "command",
            "expires_at",
            "lan_ip",
            "port",
            "url"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    );

    // The extra listener is on that one address, serves the wrapper (with the key), and nothing else.
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let got = client.get(&url).send().await.unwrap();
    assert_eq!(got.status(), 200);
    let script = got.text().await.unwrap();
    assert!(script.starts_with("#!/bin/sh"));
    // The script holds some key (the one just made), 43 characters, and the command did not.
    let made = script
        .lines()
        .find_map(|l| {
            l.strip_prefix("  printf '%s\\n' '")
                .and_then(|r| r.strip_suffix("' > \"$TMP/key.txt\""))
        })
        .expect("the key line")
        .to_string();
    assert_eq!(made.len(), 43);
    assert!(!first["command"].as_str().unwrap().contains(&made));
    assert!(!first.to_string().contains(&made));
    let api = client
        .get(format!("http://{ip}:{port}/api/v1/state"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        api.status(),
        404,
        "the dashboard API is not on this listener"
    );
    assert!(
        std::net::TcpStream::connect_timeout(
            &SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port),
            std::time::Duration::from_millis(500)
        )
        .is_err(),
        "it is bound to the LAN address, not 0.0.0.0"
    );

    // A second POST extends: same token, same port, same key.
    let second = send(
        &app,
        "POST",
        "/api/v1/propresenter-link/helper-install",
        body,
        "127.0.0.1",
    )
    .await
    .json();
    assert_eq!(second["url"], first["url"]);
    let again = client.get(&url).send().await.unwrap().text().await.unwrap();
    assert!(again.contains(&made));

    let ended = send(
        &app,
        "DELETE",
        "/api/v1/propresenter-link/helper-install",
        None,
        "127.0.0.1",
    )
    .await;
    assert_eq!(ended.json()["ended"], json!(true));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        client.get(&url).send().await.is_err(),
        "the listener closed with the session"
    );
    // The key outlives the session: it is made once.
    assert!(rt.propresenter_link.is_paired());
    let third = send(
        &app,
        "POST",
        "/api/v1/propresenter-link/helper-install",
        Some(json!({"address": ip.to_string()})),
        "127.0.0.1",
    )
    .await
    .json();
    let script = client
        .get(third["url"].as_str().unwrap())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(script.contains(&made), "a new session keeps the same key");
    let _ = send(
        &app,
        "DELETE",
        "/api/v1/propresenter-link/helper-install",
        None,
        "127.0.0.1",
    )
    .await;
}

/// A stand-in for helper 0.3.0 that wants `KEY`: it answers a signed `/health` and nothing else.
async fn keyed_helper() -> String {
    async fn answer(State(seen): State<Arc<Mutex<u32>>>, uri: Uri, headers: HeaderMap) -> Response {
        *seen.lock().unwrap() += 1;
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let target = uri
            .path_and_query()
            .map_or(String::new(), |t| t.as_str().to_string());
        let valid = match (text("x-ppl-time"), text("x-ppl-sig")) {
            (Some(time), Some(sig)) => time
                .parse::<u64>()
                .is_ok_and(|t| t.abs_diff(now) <= 120 && sign(KEY, "GET", &target, t, None) == sig),
            _ => false,
        };
        if valid && uri.path() == "/health" {
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                json!({"ok": true, "version": bundled::BUNDLED_HELPER_VERSION, "auth": "key", "os": "macOS", "data_folder": "/Users/x/Documents/ProPresenter", "data_folder_exists": true, "libraries": ["Playback", "Songs"], "can_import": true, "updater": 1}).to_string(),
            )
                .into_response();
        }
        (StatusCode::UNAUTHORIZED, "{\"error\": \"key required\"}").into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new()
        .fallback(answer)
        .with_state(Arc::new(Mutex::new(0u32)));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://127.0.0.1:{port}")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_session_ends_by_itself_when_the_helper_answers_the_signed_request() {
    let Some(ip) = own_private_address() else {
        eprintln!("skipped: this computer has no private LAN address");
        return;
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let helper = keyed_helper().await;
    let rt = runtime();
    // The key exists already (an earlier session); the helper has not been set up yet, so it refuses.
    rt.propresenter_link
        .set_key_for_test("ANOTHER-key-ANOTHER-key-ANOTHER-key-0123");
    save_pp_host(&rt, "127.0.0.1");
    // The stand-in helper is on this computer at its own port: asked as another computer.
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| false));
    rt.propresenter_link
        .set_helper_port(helper.rsplit(':').next().unwrap().parse().unwrap());
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let reply = send(
        &app,
        "POST",
        "/api/v1/propresenter-link/helper-install",
        Some(json!({"address": ip.to_string()})),
        "127.0.0.1",
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
    tokio::time::sleep(std::time::Duration::from_millis(3800)).await;
    assert!(
        rt.propresenter_helper_install.is_active(),
        "a helper that refuses the key does not end the session"
    );
    // The command is run on the ProPresenter computer: the helper now has the key StagePilot holds.
    rt.propresenter_link.set_key_for_test(KEY);
    // (A helper that refused is not asked again for ten seconds; the test does not wait them out.)
    rt.propresenter_link
        .advance_clock(std::time::Duration::from_secs(11));
    let mut ended = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if !rt.propresenter_helper_install.is_active() {
            ended = true;
            break;
        }
    }
    assert!(
        ended,
        "the session ended when the helper answered with a valid signature"
    );
}

// ---- never through the remote tunnel -------------------------------------------------------------------

#[tokio::test]
async fn the_remote_ingress_never_passes_the_installer_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime();
    let access = RemoteAccess::new(RemoteStore::new(dir.path().join("identity.db")));
    rt.set_remote_access(Some(access.clone()));
    rt.propresenter_link.set_key_for_test(KEY);
    let info = rt
        .propresenter_helper_install
        .begin(LAN, 8000, None)
        .unwrap();
    let local = build_app(rt.clone(), AppOptions { web_root: None });
    let edge = RemoteIngress::new(access.clone(), ORIGIN)
        .unwrap()
        .wrap(local);
    access
        .store
        .bootstrap("operator@example.test", PASSWORD)
        .unwrap();
    let session = access
        .store
        .login("operator@example.test", PASSWORD, None)
        .unwrap();
    let cookie = format!("__Host-stagepilot_remote={session}");
    let csrf = csrf_token(&session);
    let paths = [
        format!("/propresenter-helper/{}/install.sh", info.token),
        format!("/propresenter-helper/{}/files/install.sh", info.token),
        "/propresenter-helper/".to_string(),
        "/propresenter-helper".to_string(),
    ];
    for path in paths {
        for (name, signed_in) in [("signed out", false), ("an admin session", true)] {
            for method in ["GET", "POST"] {
                let mut builder = Request::builder()
                    .method(method)
                    .uri(&path)
                    .header("origin", ORIGIN)
                    .header("x-stagepilot-remote", "1");
                if signed_in {
                    builder = builder
                        .header("cookie", &cookie)
                        .header("x-csrf-token", &csrf);
                }
                let mut request = builder.body(Body::empty()).unwrap();
                let peer: SocketAddr = "192.168.1.50:50000".parse().unwrap();
                request.extensions_mut().insert(ConnectInfo(peer));
                let response = edge.clone().oneshot(request).await.unwrap();
                assert_eq!(
                    response.status(),
                    StatusCode::NOT_FOUND,
                    "{method} {path} ({name})"
                );
                let body = response.into_body().collect().await.unwrap().to_bytes();
                let text = String::from_utf8_lossy(&body);
                assert!(
                    !text.contains("#!/bin/sh") && !text.contains(KEY),
                    "{path} ({name}) leaked"
                );
            }
        }
    }
}

#[tokio::test]
async fn the_handlers_refuse_a_request_that_carries_a_remote_context() {
    // Second line of defence: even if a listener passed the prefix, the handlers answer 404 to anything
    // the remote ingress has touched.
    let dir = tempfile::tempdir().unwrap();
    let (app, _rt, token) = app_with_session();
    let access = RemoteAccess::new(RemoteStore::new(dir.path().join("identity.db")));
    for path in [
        format!("/propresenter-helper/{token}/install.sh"),
        format!("/propresenter-helper/{token}/files/install.sh"),
    ] {
        let mut request = Request::builder().uri(&path).body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo("192.168.1.50:5".parse::<SocketAddr>().unwrap()));
        request.extensions_mut().insert(RemoteContext {
            access: access.clone(),
            token: None,
            session: None,
        });
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

// ---- the wrapper, run for real ------------------------------------------------------------------------

#[cfg(unix)]
mod wrapper {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    /// The bundled helper script with a stand-in installer that records how it was called and what the key
    /// file looked like, then deletes the key file as the real installer does.
    fn stub_assets() -> Assets {
        let installer = "#!/bin/sh\n\
            printf '%s\\n' \"$@\" > \"$HOME/installer-args\"\n\
            f=\"$2\"\n\
            ls -ln \"$f\" | cut -c1-10 > \"$HOME/key-mode\"\n\
            ls -ldn \"$(dirname \"$f\")\" | cut -c1-10 > \"$HOME/dir-mode\"\n\
            head -1 \"$f\" > \"$HOME/key-content\"\n\
            pwd > \"$HOME/installer-dir\"\n\
            ls > \"$HOME/installer-files\"\n\
            rm -f \"$f\"\n\
            echo 'stub installer ran'\n";
        let mut files: Vec<(String, Vec<u8>)> = vec![(
            "propresenter_link.py".into(),
            bundled::file("propresenter_link.py").unwrap().to_vec(),
        )];
        files.push(("install.sh".into(), installer.as_bytes().to_vec()));
        let mut sums = String::new();
        for (name, bytes) in &files {
            sums.push_str(&format!("{}  {name}\n", hex::encode(Sha256::digest(bytes))));
        }
        files.push(("SHA256SUMS".into(), sums.into_bytes()));
        Assets::from_files(files, "9.9.9")
    }

    /// A real listener on loopback serving a session; returns `(base_url, service, library)`.
    async fn serve(
        assets: Assets,
    ) -> (
        String,
        Arc<ProPresenterHelperInstall>,
        Arc<ProPresenterLink>,
    ) {
        // The tests fetch the wrapper with `reqwest`, which has no TLS provider of its own here.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let library = Arc::new(ProPresenterLink::new(
            stagepilot_server::propresenter_link::memory_settings(),
        ));
        library.set_key_for_test(KEY);
        let svc = Arc::new(ProPresenterHelperInstall::with_assets(
            library.clone(),
            assets,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = served_routes::<()>(svc.clone());
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let info = svc.begin(Ipv4Addr::LOCALHOST, port, None).unwrap();
        (info.base_url(), svc, library)
    }

    struct Run {
        ok: bool,
        output: String,
        home: tempfile::TempDir,
    }

    impl Run {
        fn file(&self, name: &str) -> Option<String> {
            std::fs::read_to_string(self.home.path().join(name)).ok()
        }
    }

    /// Fetch the wrapper with `curl` (as the pasted command does) and pipe it to `sh`, in a temp HOME and TMPDIR.
    async fn paste(url: String) -> Run {
        tokio::task::spawn_blocking(move || {
            let home = tempfile::tempdir().unwrap();
            std::fs::create_dir(home.path().join("tmp")).unwrap();
            let command = format!("curl -fsS '{url}' | sh");
            let output = Command::new("sh")
                .arg("-c")
                .arg(command)
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap())
                .env("HOME", home.path())
                .env("TMPDIR", home.path().join("tmp"))
                .output()
                .unwrap();
            Run {
                ok: output.status.success(),
                output: format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
                home,
            }
        })
        .await
        .unwrap()
    }

    fn nothing_left_in(dir: &Path) -> bool {
        std::fs::read_dir(dir).unwrap().next().is_none()
    }

    fn curl_and_sh_exist() -> bool {
        ["curl", "sh"]
            .iter()
            .all(|tool| Command::new(tool).arg("--version").output().is_ok())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_wrapper_fetches_verifies_and_runs_the_installer_with_a_private_key_file() {
        if !curl_and_sh_exist() {
            eprintln!("skipped: no curl");
            return;
        }
        let (base, svc, _library) = serve(stub_assets()).await;
        let run = paste(format!("{base}/install.sh")).await;
        assert!(run.ok, "{}", run.output);
        assert!(
            run.output.contains("Done. Go back to StagePilot."),
            "{}",
            run.output
        );
        assert!(run.output.contains("stub installer ran"), "{}", run.output);
        // The installer was given a key FILE: the key itself is on no command line and in no output.
        let args = run.file("installer-args").unwrap();
        let mut lines = args.lines();
        assert_eq!(lines.next(), Some("--key-file"));
        let path = lines.next().unwrap();
        assert!(path.ends_with("/key.txt"), "{args}");
        // And only the StagePilot computer's address, as the one computer that is answered.
        assert_eq!(lines.next(), Some("--allow-for"));
        assert_eq!(lines.next(), Some("127.0.0.1"));
        assert!(lines.next().is_none(), "{args}");
        assert!(!args.contains(KEY));
        assert!(!run.output.contains(KEY), "the key never prints");
        // That file held the key, only its owner could read it, in a folder only its owner could enter.
        assert_eq!(run.file("key-content").unwrap().trim_end(), KEY);
        assert_eq!(run.file("key-mode").unwrap().trim_end(), "-rw-------");
        assert_eq!(run.file("dir-mode").unwrap().trim_end(), "drwx------");
        let mut files: Vec<String> = run
            .file("installer-files")
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                "SHA256SUMS",
                "install.sh",
                "key.txt",
                "propresenter_link.py"
            ]
        );
        // It cleaned up after itself: no folder, no key file.
        assert!(nothing_left_in(&run.home.path().join("tmp")));
        assert!(svc.is_active());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_tampered_file_aborts_before_the_key_is_written_or_the_installer_runs() {
        if !curl_and_sh_exist() {
            eprintln!("skipped: no curl");
            return;
        }
        // Server A: the honest files. Server B: the same, but one file altered. The wrapper (with A's
        // checksums) is told to download from B.
        let (honest_base, _a, _) = serve(stub_assets()).await;
        for tampered in ["propresenter_link.py", "install.sh"] {
            let mut assets = stub_assets();
            let entry = assets
                .files_mut()
                .iter_mut()
                .find(|(n, _)| n == tampered)
                .unwrap();
            entry.1.extend_from_slice(b"\necho pwned\n");
            let (evil_base, _b, _) = serve(assets).await;
            let script = reqwest::get(format!("{honest_base}/install.sh"))
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            let script = script.replace(&honest_base, &evil_base);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("wrapper.sh");
            std::fs::write(&path, script).unwrap();
            let path_text = path.to_string_lossy().to_string();
            let run = tokio::task::spawn_blocking(move || {
                let home = tempfile::tempdir().unwrap();
                std::fs::create_dir(home.path().join("tmp")).unwrap();
                let output = Command::new("sh")
                    .arg(&path_text)
                    .env_clear()
                    .env("PATH", std::env::var("PATH").unwrap())
                    .env("HOME", home.path())
                    .env("TMPDIR", home.path().join("tmp"))
                    .output()
                    .unwrap();
                Run {
                    ok: output.status.success(),
                    output: format!(
                        "{}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ),
                    home,
                }
            })
            .await
            .unwrap();
            assert!(!run.ok, "{tampered}: {}", run.output);
            assert!(
                run.output
                    .contains(&format!("{tampered} does not match its checksum")),
                "{tampered}: {}",
                run.output
            );
            assert!(
                run.file("installer-args").is_none(),
                "the installer must not run"
            );
            assert!(
                nothing_left_in(&run.home.path().join("tmp")),
                "the temp folder is removed"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_sums_file_that_disagrees_with_a_file_aborts_too() {
        if !curl_and_sh_exist() {
            eprintln!("skipped: no curl");
            return;
        }
        // The files and their checksums in the script agree with each other, but SHA256SUMS (itself
        // checked) lists another hash for install.sh: nothing is installed.
        let mut assets = stub_assets();
        let entry = assets
            .files_mut()
            .iter_mut()
            .find(|(n, _)| n == "SHA256SUMS")
            .unwrap();
        entry.1 = format!(
            "{}  install.sh\n{}  propresenter_link.py\n",
            "0".repeat(64),
            "1".repeat(64)
        )
        .into_bytes();
        let (base, _svc, _) = serve(assets).await;
        let run = paste(format!("{base}/install.sh")).await;
        assert!(!run.ok, "{}", run.output);
        assert!(
            run.output
                .contains("is not listed with the same checksum in SHA256SUMS"),
            "{}",
            run.output
        );
        assert!(run.file("installer-args").is_none());
        assert!(nothing_left_in(&run.home.path().join("tmp")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_ended_session_exits_non_zero_with_a_reason_and_leaves_nothing() {
        if !curl_and_sh_exist() {
            eprintln!("skipped: no curl");
            return;
        }
        let (base, svc, _) = serve(stub_assets()).await;
        // The session ends after the wrapper was fetched: the file downloads fail.
        let script = reqwest::get(format!("{base}/install.sh"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        svc.end();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.sh");
        std::fs::write(&path, script).unwrap();
        let path_text = path.to_string_lossy().to_string();
        let output = tokio::task::spawn_blocking(move || {
            Command::new("sh").arg(path_text).output().unwrap()
        })
        .await
        .unwrap();
        assert!(!output.status.success());
        let text = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            text.contains("StagePilot setup failed: could not download"),
            "{text}"
        );
        assert!(!text.contains(KEY));
    }
}
