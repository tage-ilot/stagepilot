//! M4: a damaged or newer settings file is reported by `/api/v1/settings` and a newer file is
//! never overwritten by a save.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::settings::{MemoryCredentialStore, SettingsFileStore, SettingsService};
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

async fn app(file: &[u8]) -> (axum::Router, std::path::PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, file).unwrap();
    let service = SettingsService::new(
        Box::new(SettingsFileStore::new(&path)),
        Box::new(MemoryCredentialStore::default()),
        Default::default(),
    );
    service.load();
    let runtime = Runtime::new(Arc::new(service), false);
    runtime.start().await;
    (build_app(runtime, AppOptions { web_root: None }), path, dir)
}

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(path);
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
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_healthy_file_is_not_degraded() {
    let (router, _, _dir) = app(b"{\"schema_version\": 2}").await;
    let (_, body) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(body["settings_degraded"], false);
    assert_eq!(body["settings_degraded_reason"], Value::Null);
}

#[tokio::test]
async fn a_damaged_file_is_reported_and_kept() {
    let (router, path, _dir) = app(b"{ truncated").await;
    let (_, body) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(body["settings_degraded"], true);
    assert!(body["settings_degraded_reason"]
        .as_str()
        .unwrap()
        .contains("JSON"));
    assert!(body["warning"]
        .as_str()
        .unwrap()
        .contains("built-in defaults"));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"{ truncated",
        "reading settings never writes"
    );
}

#[tokio::test]
async fn a_newer_file_is_reported_and_a_save_cannot_overwrite_it() {
    let newer = br#"{"schema_version": 3, "from_the_future": true}"#;
    let (router, path, _dir) = app(newer).await;
    let (_, body) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(body["settings_degraded"], true);
    assert!(body["settings_degraded_reason"]
        .as_str()
        .unwrap()
        .contains("schema_version 3"));
    let (code, body) = call(
        &router,
        "PUT",
        "/api/v1/settings",
        Some(json!({"timezone": "America/Chicago"})),
    )
    .await;
    assert_ne!(code, StatusCode::OK, "{body}");
    assert_eq!(std::fs::read(&path).unwrap(), newer);
}
