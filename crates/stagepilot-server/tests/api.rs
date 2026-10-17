//! HTTP-level tests of the Rust server: the Rust counterparts of `test_dashboard_auth.py` and the
//! scoped parts of `test_api.py` / `test_settings_persistence.py`.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::settings::{
    MemoryCredentialStore, RuntimeSettings, SettingsFileStore, SettingsService,
};
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

async fn app(enforced: bool) -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral(RuntimeSettings::demo());
    let runtime = Runtime::new(Arc::new(service), enforced);
    runtime.start().await;
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    )
}

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
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
    let bytes = body.collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (parts.status, parts.headers, value)
}

fn cookie(headers: &axum::http::HeaderMap) -> String {
    headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn browser_api_requires_the_default_pin_and_uses_a_session_cookie() {
    let (router, _rt) = app(true).await;
    let (_, _, status) = call(&router, "GET", "/api/v1/dashboard-auth/status", None, &[]).await;
    assert_eq!(status, json!({"required": true, "authenticated": false}));
    assert_eq!(
        call(&router, "GET", "/api/v1/health", None, &[]).await.0,
        StatusCode::OK
    );
    let (code, _, body) = call(&router, "GET", "/api/v1/state", None, &[]).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({"detail": "Dashboard PIN required."}));
    let wrong = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "9999"})),
        &[],
    )
    .await;
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    let (code, headers, body) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "1234"})),
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["authenticated"], true);
    let set = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_lowercase();
    assert!(set.contains("httponly") && set.contains("samesite=strict"));
    let jar = cookie(&headers);
    let (code, _, _) = call(&router, "GET", "/api/v1/state", None, &[("cookie", &jar)]).await;
    assert_eq!(code, StatusCode::OK);
}

#[tokio::test]
async fn the_desktop_origin_bypasses_the_browser_pin_but_only_from_loopback() {
    let (router, _rt) = app(true).await;
    let (code, _, _) = call(
        &router,
        "GET",
        "/api/v1/state",
        None,
        &[("origin", "tauri://localhost")],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _, _) = call(
        &router,
        "GET",
        "/api/v1/state",
        None,
        &[("origin", "https://evil.example")],
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_pin_can_be_replaced_and_protection_disabled_and_the_hash_is_never_returned() {
    let (router, _rt) = app(true).await;
    let (_, headers, _) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "1234"})),
        &[],
    )
    .await;
    let jar = cookie(&headers);
    let (code, _, body) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/settings",
        Some(json!({"enabled": true, "pin": "8642"})),
        &[("cookie", &jar)],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(!body.to_string().contains("web_dashboard_pin_hash"));
    // Changing the PIN clears every session, including the one that changed it.
    assert_eq!(
        call(&router, "GET", "/api/v1/state", None, &[("cookie", &jar)])
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &router,
            "POST",
            "/api/v1/dashboard-auth/login",
            Some(json!({"pin": "1234"})),
            &[]
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, headers, _) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "8642"})),
        &[],
    )
    .await;
    let jar = cookie(&headers);
    call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/settings",
        Some(json!({"enabled": false})),
        &[("cookie", &jar)],
    )
    .await;
    assert_eq!(
        call(&router, "GET", "/api/v1/state", None, &[]).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn pin_validation_errors_match_fastapi() {
    let (router, _rt) = app(false).await;
    let (code, _, body) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "12"})),
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "string_too_short");
    assert_eq!(body["detail"][0]["loc"], json!(["body", "pin"]));
    let (_, _, body) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/settings",
        Some(json!({"pin": "8642"})),
        &[],
    )
    .await;
    assert_eq!(body["detail"][0]["type"], "missing");
    let (_, _, body) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/settings",
        Some(json!({"enabled": true, "pin": "12"})),
        &[],
    )
    .await;
    assert_eq!(body["detail"][0]["type"], "too_short");
}

#[tokio::test]
async fn settings_round_trip_survives_a_get_and_rejects_bad_bodies() {
    let (router, _rt) = app(false).await;
    let (code, _, body) = call(
        &router,
        "PUT",
        "/api/v1/settings",
        Some(json!({"timezone": "America/New_York"})),
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["settings"]["timezone"], "America/New_York");
    let (_, _, again) = call(&router, "GET", "/api/v1/settings", None, &[]).await;
    assert_eq!(again["settings"]["timezone"], "America/New_York");
    assert_eq!(
        call(
            &router,
            "PUT",
            "/api/v1/settings",
            Some(json!({"timezone": 5})),
            &[]
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &router,
            "PUT",
            "/api/v1/settings",
            Some(json!({"bogus": 1})),
            &[]
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn a_service_source_change_is_refused_while_the_timer_runs() {
    let (router, _rt) = app(false).await;
    call(&router, "POST", "/api/v1/actions/start_next", None, &[]).await;
    let (code, _, body) = call(
        &router,
        "PUT",
        "/api/v1/settings",
        Some(json!({"integration_modes": {"service_source": "planning_center", "midi_source": "simulated", "timer_output": "simulated"}})),
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(
        body["detail"],
        "Stop the timer before switching between Demo and Planning Center."
    );
}

#[tokio::test]
async fn actions_move_through_the_demo_plan_and_unknown_actions_are_422() {
    let (router, _rt) = app(false).await;
    let (_, _, started) = call(&router, "POST", "/api/v1/actions/start_next", None, &[]).await;
    assert_eq!(started["accepted"], true);
    assert_eq!(started["state"]["timer"]["status"], "running");
    assert_eq!(started["state"]["playing_song"]["title"], "Battle Belongs");
    let (_, _, stopped) = call(&router, "POST", "/api/v1/actions/stop_timer", None, &[]).await;
    assert_eq!(stopped["message"], "Timer stop requested.");
    let (code, _, bad) = call(&router, "POST", "/api/v1/actions/bogus", None, &[]).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad["detail"][0]["type"], "enum");
}

#[tokio::test]
async fn unknown_routes_and_wrong_methods_use_fastapi_bodies() {
    let (router, _rt) = app(false).await;
    let (code, _, body) = call(&router, "GET", "/api/v1/does-not-exist", None, &[]).await;
    assert_eq!(
        (code, body),
        (StatusCode::NOT_FOUND, json!({"detail": "Not Found"}))
    );
    let (code, _, body) = call(&router, "DELETE", "/api/v1/state", None, &[]).await;
    assert_eq!(
        (code, body),
        (
            StatusCode::METHOD_NOT_ALLOWED,
            json!({"detail": "Method Not Allowed"})
        )
    );
}

#[tokio::test]
async fn health_reports_healthy_in_demo_mode_with_the_three_plugins() {
    let (router, _rt) = app(false).await;
    let (_, _, body) = call(&router, "GET", "/api/v1/health", None, &[]).await;
    assert_eq!(body["status"], "healthy");
    let names: Vec<_> = body["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["lights", "demo", "playback_api"]);
}

#[tokio::test]
async fn access_reports_lan_or_desktop_mode() {
    let (router, _rt) = app(true).await;
    let (_, headers, lan) = call(&router, "GET", "/api/v1/access", None, &[]).await;
    assert_eq!(lan["mode"], "lan");
    assert_eq!(lan["authentication"], "pin");
    assert_eq!(lan["authenticated"], false);
    assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    let (_, _, desktop) = call(
        &router,
        "GET",
        "/api/v1/access",
        None,
        &[("origin", "tauri://localhost")],
    )
    .await;
    assert_eq!(desktop["mode"], "desktop");
    assert_eq!(desktop["authentication"], "none");
    assert_eq!(desktop["capabilities"]["canOperate"], true);
}

#[tokio::test]
async fn a_pin_hash_written_to_disk_is_the_python_format_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let build = || {
        let service = SettingsService::new(
            Box::new(SettingsFileStore::new(&path)),
            Box::new(MemoryCredentialStore::default()),
            Default::default(),
        );
        service.load();
        Runtime::new(Arc::new(service), true)
    };
    let first = build();
    let router = build_app(first.clone(), AppOptions { web_root: None });
    let (_, headers, _) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "1234"})),
        &[],
    )
    .await;
    let jar = cookie(&headers);
    let (code, _, _) = call(
        &router,
        "POST",
        "/api/v1/dashboard-auth/settings",
        Some(json!({"enabled": true, "pin": "8642"})),
        &[("cookie", &jar)],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(saved["web_dashboard_pin_hash"]
        .as_str()
        .unwrap()
        .starts_with("pbkdf2_sha256$210000$"));
    let second = build_app(build(), AppOptions { web_root: None });
    let ok = call(
        &second,
        "POST",
        "/api/v1/dashboard-auth/login",
        Some(json!({"pin": "8642"})),
        &[],
    )
    .await;
    assert_eq!(ok.0, StatusCode::OK);
}

#[tokio::test]
async fn the_websocket_streams_snapshots_with_increasing_revisions() {
    let (router, _rt) = app(false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/ws"))
        .await
        .unwrap();
    let next = |msg: Option<Result<tokio_tungstenite::tungstenite::Message, _>>| -> Value {
        match msg.unwrap().unwrap() {
            tokio_tungstenite::tungstenite::Message::Text(t) => {
                serde_json::from_str(t.as_str()).unwrap()
            }
            other => panic!("unexpected frame {other:?}"),
        }
    };
    let first = next(socket.next().await);
    assert_eq!(first["type"], "state.snapshot");
    let revision = first["data"]["revision"].as_u64().unwrap();
    // Trigger a change over HTTP on the same server.
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    use tokio::io::AsyncWriteExt;
    stream
        .write_all(b"POST /api/v1/actions/start_next HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let second = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await
        .unwrap();
    let second = next(second);
    assert!(second["data"]["revision"].as_u64().unwrap() > revision);
}

#[tokio::test]
async fn dashboard_pin_is_enforced_on_the_websocket() {
    let (router, _rt) = app(true).await;
    let (code, _, _) = call(&router, "GET", "/ws", None, &[]).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
}
