//! VerseVision inside StagePilot (`versevision_addon`): the page and its API at `/versevision/`, behind the PIN and the Origin check,
//! its files under the settings folder, ProPresenter's address from the Presentation connection, the installation credential that
//! never leaves memory, and nothing for a remote session.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::auth::{csrf_token, RemoteStore};
use stagepilot_core::settings::{MemoryCredentialStore, SettingsFileStore, SettingsService};
use stagepilot_server::remote_fakes::{FakeBootstrap, FakeManager};
use stagepilot_server::versevision_addon::Installation;
use stagepilot_server::{build_app, AppOptions, RemoteAccess, RemoteIngress, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;
use versevision_bible::InstallationAuth;

mod support;

const HOST: &str = "192.168.1.20:8765";

struct Rig {
    router: axum::Router,
    runtime: Arc<Runtime>,
    dir: tempfile::TempDir,
}

/// A StagePilot with file-backed settings in a temporary folder and the dashboard PIN enforced (the default PIN, 1234).
async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let service = SettingsService::new(
        Box::new(SettingsFileStore::new(dir.path().join("settings.json"))),
        Box::new(MemoryCredentialStore::default()),
        Default::default(),
    );
    service.load();
    let runtime = Runtime::new(Arc::new(service), true);
    runtime.start().await;
    let router = build_app(runtime.clone(), AppOptions { web_root: None });
    Rig {
        router,
        runtime,
        dir,
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }
}

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
    let peer: SocketAddr = "192.168.1.50:50000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status,
        headers: parts.headers,
        bytes: body.collect().await.unwrap().to_bytes().to_vec(),
    }
}

/// Sign in with the default PIN; returns the session cookie.
async fn sign_in(router: &axum::Router) -> String {
    let reply = send(
        router,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "1234"})),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

fn present(rig: &Rig, host: &str, port: u16) {
    let service = &rig.runtime.settings_service;
    let mut settings = service.snapshot();
    settings.propresenter.enabled = true;
    settings.propresenter.host = host.to_string();
    settings.propresenter.port = port;
    service.save(settings).unwrap();
    let live = service.effective_snapshot().propresenter;
    assert_eq!((live.host.as_str(), live.port), (host, port));
}

// ---- the page and its API are behind the PIN -----------------------------------------------------

#[tokio::test]
async fn the_page_and_its_api_answer_with_the_pin_and_are_refused_without_it() {
    let r = rig().await;
    let page = send(
        &r.router,
        "GET",
        "/versevision/",
        None,
        &[("accept", "text/html"), ("host", HOST)],
    )
    .await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert!(page.text().contains("Enter the StagePilot PIN first"));
    for path in [
        "/versevision/api/status",
        "/versevision/api/config",
        "/versevision/api/bible/keys",
        "/versevision/sw.js",
        "/versevision",
    ] {
        let refused = send(&r.router, "GET", path, None, &[("host", HOST)]).await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(
            refused.json(),
            json!({"detail": "Dashboard PIN required."}),
            "{path}"
        );
    }
    let refused = send(
        &r.router,
        "POST",
        "/versevision/api/undo",
        Some(json!({})),
        &[("host", HOST)],
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    // Nothing was created for a refused request.
    assert!(!r.dir.path().join("versevision").exists());

    let cookie = sign_in(&r.router).await;
    let with = [
        ("cookie", cookie.as_str()),
        ("host", HOST),
        ("accept", "text/html"),
    ];
    let page = send(&r.router, "GET", "/versevision/", None, &with).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.text().contains("VerseVision"));
    // The page keeps its own policy (it has inline script and style); StagePilot's other headers still apply.
    let policy = page.headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    assert!(
        policy.contains("default-src 'self'")
            && policy.contains("script-src 'self' 'unsafe-inline'"),
        "{policy}"
    );
    assert_eq!(page.headers["x-frame-options"], "DENY");
    assert_eq!(page.headers["x-content-type-options"], "nosniff");
    // The address people type without the slash answers too.
    assert_eq!(
        send(&r.router, "GET", "/versevision", None, &with)
            .await
            .status,
        StatusCode::OK
    );

    let status = send(&r.router, "GET", "/versevision/api/status", None, &with).await;
    assert_eq!(status.status, StatusCode::OK);
    assert_eq!(
        status.json()["version"],
        json!(stagepilot_server::APP_VERSION)
    );
    assert_eq!(
        send(&r.router, "GET", "/versevision/api/config", None, &with)
            .await
            .status,
        StatusCode::OK
    );
    // StagePilot's own dashboard is unchanged: its CSP is still the strict one.
    let state = send(&r.router, "GET", "/api/v1/state", None, &with).await;
    assert!(state.headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("script-src 'self';"));
}

#[tokio::test]
async fn its_files_live_under_the_settings_folder_and_the_keys_are_owner_only() {
    let r = rig().await;
    let cookie = sign_in(&r.router).await;
    let with = [("cookie", cookie.as_str()), ("host", HOST)];
    assert_eq!(
        send(&r.router, "GET", "/versevision/api/status", None, &with)
            .await
            .status,
        StatusCode::OK
    );
    let root = r.dir.path().join("versevision");
    for folder in ["config", "data", "logs", "keys"] {
        assert!(root.join(folder).is_dir(), "{folder}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&root.join("keys")), 0o700);
        // A key saved on the Config page lands in keys/keys.env with owner-only permissions.
        let saved = send(
            &r.router,
            "POST",
            "/versevision/api/bible/keys",
            Some(json!({"id": "esv", "key": "not-a-real-key-0123456789"})),
            &[
                ("cookie", cookie.as_str()),
                ("host", HOST),
                ("origin", "http://192.168.1.20:8765"),
            ],
        )
        .await;
        assert_eq!(saved.status, StatusCode::OK, "{}", saved.text());
        assert_eq!(mode(&root.join("keys").join("keys.env")), 0o600);
        assert!(!saved.text().contains("not-a-real-key"));
    }
}

#[tokio::test]
async fn a_post_from_another_origin_is_refused() {
    let r = rig().await;
    let cookie = sign_in(&r.router).await;
    for origin in ["https://evil.example", "http://192.168.1.20:9999", "null"] {
        let refused = send(
            &r.router,
            "POST",
            "/versevision/api/bible/source",
            Some(json!({"source": "own_keys"})),
            &[
                ("cookie", cookie.as_str()),
                ("host", HOST),
                ("origin", origin),
            ],
        )
        .await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "{origin}");
    }
    assert!(!r
        .dir
        .path()
        .join("versevision")
        .join("config")
        .join("config.toml")
        .exists());
    // The page's own origin is let through to VerseVision, which answers.
    let allowed = send(
        &r.router,
        "POST",
        "/versevision/api/bible/source",
        Some(json!({"source": "own_keys"})),
        &[
            ("cookie", cookie.as_str()),
            ("host", HOST),
            ("origin", "http://192.168.1.20:8765"),
        ],
    )
    .await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.text());
}

// ---- remote sessions get nothing -----------------------------------------------------------------

#[tokio::test]
async fn a_remote_session_gets_not_found_even_when_signed_in() {
    const ORIGIN: &str = "https://remote.stagepilot.test";
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(
        Arc::new(SettingsService::ephemeral(
            stagepilot_core::settings::RuntimeSettings::demo(),
        )),
        false,
    );
    let access = RemoteAccess::new(RemoteStore::new(dir.path().join("identity.db")));
    runtime.set_remote_access(Some(access.clone()));
    let router = RemoteIngress::new(access.clone(), ORIGIN)
        .unwrap()
        .wrap(build_app(runtime.clone(), AppOptions { web_root: None }));
    access
        .store
        .bootstrap("operator@example.test", "a-test-password-only")
        .unwrap();
    let token = access
        .store
        .login("operator@example.test", "a-test-password-only", None)
        .unwrap();
    let cookie = format!("__Host-stagepilot_remote={token}");
    let csrf = csrf_token(&token);
    for (method, path) in [
        ("GET", "/versevision/"),
        ("GET", "/versevision/api/status"),
        ("POST", "/versevision/api/undo"),
        ("GET", "/versevision"),
    ] {
        let reply = send(
            &router,
            method,
            path,
            (method == "POST").then(|| json!({})),
            &[
                ("cookie", cookie.as_str()),
                ("origin", ORIGIN),
                ("x-stagepilot-remote", "1"),
                ("x-csrf-token", csrf.as_str()),
            ],
        )
        .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    assert!(!dir.path().join("versevision").exists());
}

// ---- the installation credential -----------------------------------------------------------------

#[tokio::test]
async fn the_credential_is_used_by_the_bible_service_check_and_never_shown_or_logged() {
    let logs = support::Capture::start();
    let r = rig().await;
    let bootstrap = FakeBootstrap::enrolled("https://control.stagepilot.test");
    let credential = bootstrap.credential.clone().unwrap();
    let manager = FakeManager {
        bootstrap: Some(Arc::new(bootstrap)),
        ..FakeManager::default()
    };
    r.runtime.set_remote_manager(Some(Arc::new(manager)));
    let cookie = sign_in(&r.router).await;
    let with = [
        ("cookie", cookie.as_str()),
        ("host", HOST),
        ("accept", "text/html"),
    ];

    let keys = send(&r.router, "GET", "/versevision/api/bible/keys", None, &with).await;
    assert_eq!(keys.status, StatusCode::OK);
    assert_eq!(
        keys.json()["service"]["available"],
        json!(true),
        "{}",
        keys.text()
    );

    let mut everything = keys.text();
    for path in [
        "/versevision/",
        "/versevision/api/status",
        "/versevision/api/config",
        "/versevision/api/check",
        "/versevision/api/history",
        "/versevision/api/playlists",
    ] {
        let reply = send(&r.router, "GET", path, None, &with).await;
        everything.push_str(&reply.text());
        everything.push_str(&format!("{:?}", reply.headers));
    }
    assert!(
        !everything.contains(&credential),
        "the credential reached a response"
    );
    assert!(
        !everything.contains("spi_"),
        "something looking like a credential reached a response"
    );
    for line in [
        tracing::Level::TRACE,
        tracing::Level::DEBUG,
        tracing::Level::INFO,
        tracing::Level::WARN,
        tracing::Level::ERROR,
    ]
    .into_iter()
    .flat_map(|level| logs.at(level))
    {
        assert!(
            !line.contains(&credential),
            "the credential reached a log line: {line}"
        );
    }
    // And nothing under the data folder holds it.
    let mut stack = vec![r.dir.path().to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                assert!(
                    !String::from_utf8_lossy(&bytes).contains(&credential),
                    "{}",
                    path.display()
                );
            }
        }
    }
}

#[tokio::test]
async fn without_an_enrolment_there_is_no_installation_and_the_service_is_not_offered() {
    let r = rig().await;
    let none = Installation::new(r.runtime.clone());
    assert!(
        none.installation_id().is_none() && none.credential().is_none() && none.origin().is_empty()
    );
    let cookie = sign_in(&r.router).await;
    let keys = send(
        &r.router,
        "GET",
        "/versevision/api/bible/keys",
        None,
        &[("cookie", cookie.as_str()), ("host", HOST)],
    )
    .await;
    assert_eq!(keys.json()["service"]["available"], json!(false));

    // Enrolled with an origin StagePilot does not trust: still nothing.
    let mut untrusted = FakeBootstrap::enrolled("https://elsewhere.example");
    untrusted.trusted = vec!["https://control.stagepilot.test".into()];
    r.runtime.set_remote_manager(Some(Arc::new(FakeManager {
        bootstrap: Some(Arc::new(untrusted)),
        ..FakeManager::default()
    })));
    assert!(none.installation_id().is_none() && none.credential().is_none());

    // Enrolled and trusted, and the store has no credential to give: the id may be known, the credential is not.
    let mut broken = FakeBootstrap::enrolled("https://control.stagepilot.test");
    broken.credential = Err("keychain waiting".into());
    r.runtime.set_remote_manager(Some(Arc::new(FakeManager {
        bootstrap: Some(Arc::new(broken)),
        ..FakeManager::default()
    })));
    assert_eq!(none.installation_id().as_deref(), Some("abcd1234"));
    assert!(none.credential().is_none());
}

// ---- ProPresenter's address is the Presentation connection ---------------------------------------

/// A made-up ProPresenter: answers every request and remembers the paths it was asked for.
async fn fake_propresenter() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0_u8; 8192];
                let n = socket.read(&mut buffer).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buffer[..n]).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let (kind, body): (&str, &[u8]) = if path.contains("thumbnail") {
                    ("image/jpeg", b"\xff\xd8\xffthumb")
                } else {
                    ("application/json", b"[]")
                };
                log.lock().unwrap().push(path);
                let _ = socket
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes(),
                    )
                    .await;
                let _ = socket.write_all(body).await;
            });
        }
    });
    (port, seen)
}

#[tokio::test]
async fn propresenters_address_follows_the_presentation_connection_and_is_not_copied() {
    let r = rig().await;
    let cookie = sign_in(&r.router).await;
    let with = [("cookie", cookie.as_str()), ("host", HOST)];
    let (first, first_seen) = fake_propresenter().await;
    let (second, second_seen) = fake_propresenter().await;

    present(&r, "127.0.0.1", first);
    send(&r.router, "GET", "/versevision/api/playlists", None, &with).await;
    let picture = send(
        &r.router,
        "GET",
        "/versevision/api/themethumb?theme=Verse%20Look&slide=2",
        None,
        &with,
    )
    .await;
    assert_eq!(picture.status, StatusCode::OK);
    assert_eq!(picture.bytes, b"\xff\xd8\xffthumb");
    assert_eq!(picture.headers[header::CONTENT_TYPE], "image/jpeg");
    assert!(
        first_seen
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.starts_with("/v1/")),
        "no work reached the first ProPresenter"
    );
    assert!(
        first_seen
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/v1/theme/Verse%20Look/slides/1/thumbnail?quality=200"),
        "{:?}",
        first_seen.lock().unwrap()
    );
    assert!(second_seen.lock().unwrap().is_empty());

    // The person changes the connection on the Presentation page: the next request goes to the new address.
    let before = first_seen.lock().unwrap().len();
    present(&r, "127.0.0.1", second);
    send(&r.router, "GET", "/versevision/api/playlists", None, &with).await;
    send(
        &r.router,
        "GET",
        "/versevision/api/themethumb?theme=Verse%20Look",
        None,
        &with,
    )
    .await;
    assert_eq!(
        first_seen.lock().unwrap().len(),
        before,
        "the old address was used again"
    );
    assert!(second_seen
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.starts_with("/v1/")));
    assert!(second_seen
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.contains("thumbnail")));

    // A theme picture ProPresenter cannot give is a plain 404, as on the page's own route.
    present(&r, "127.0.0.1", 9);
    let none = send(
        &r.router,
        "GET",
        "/versevision/api/themethumb?theme=x",
        None,
        &with,
    )
    .await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);

    // Saving something on VerseVision's own Config page does not write the connection into its config.toml.
    let saved = send(
        &r.router,
        "POST",
        "/versevision/api/bible/source",
        Some(json!({"source": "auto"})),
        &[
            ("cookie", cookie.as_str()),
            ("host", HOST),
            ("origin", "http://192.168.1.20:8765"),
        ],
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK);
    let file = r
        .dir
        .path()
        .join("versevision")
        .join("config")
        .join("config.toml");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        !text.contains(&second.to_string()) && !text.contains(&first.to_string()),
        "{text}"
    );
}
