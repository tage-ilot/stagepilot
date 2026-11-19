//! The ProPresenter network scan routes: start, state, cancel. The ProPresenter is a stand-in on a loopback port that records
//! every request it gets; no real ProPresenter, no screen, no rig.

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, Method, Request, StatusCode, Uri};
use axum::routing::any;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::ProPresenterSettings;
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

type Seen = Arc<Mutex<Vec<(String, String)>>>;

async fn stand_in() -> (u16, Seen) {
    let seen: Seen = Arc::default();
    async fn handler(
        State(seen): State<Seen>,
        method: Method,
        uri: Uri,
    ) -> axum::response::Response {
        seen.lock()
            .unwrap()
            .push((method.to_string(), uri.path().to_string()));
        axum::response::IntoResponse::into_response(axum::Json(json!({
            "name": "Stage MacBook", "host_description": "ProPresenter 19.0.1", "api_version": "v1"
        })))
    }
    let app = axum::Router::new()
        .route("/{*rest}", any(handler))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (port, seen)
}

async fn send(router: &axum::Router, method: &str, path: &str, body: &str) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
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

async fn app(host: &str, port: u16) -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        propresenter: ProPresenterSettings {
            host: host.into(),
            port,
            ..ProPresenterSettings::default()
        },
        ..PersistentSettings::default()
    });
    let runtime = Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi());
    runtime.start().await;
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scan_finds_the_saved_computer_asks_only_get_version_and_saves_nothing() {
    let (port, seen) = stand_in().await;
    let (router, rt) = app("127.0.0.1", port).await;
    let before = rt.settings_service.snapshot();

    let (status, idle) = send(&router, "GET", "/api/v1/propresenter/scan", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(idle["state"], "idle");
    assert_eq!(idle["candidates"], json!([]));

    let (status, started) = send(&router, "POST", "/api/v1/propresenter/scan", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(started["state"].as_str(), Some("scanning" | "found")),
        "{started}"
    );

    let mut done = Value::Null;
    for _ in 0..300 {
        let (_, now) = send(&router, "GET", "/api/v1/propresenter/scan", "").await;
        if now["state"] != "scanning" {
            done = now;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(done["state"], "found", "{done}");
    assert_eq!(done["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(done["candidates"][0]["host"], "127.0.0.1");
    assert_eq!(done["candidates"][0]["port"], port);
    assert_eq!(done["candidates"][0]["name"], "Stage MacBook");
    assert_eq!(done["candidates"][0]["version"], "ProPresenter 19.0.1");
    assert_eq!(done["error_class"], Value::Null);

    // The one request, and nothing saved: choosing a computer and Connect are the person's.
    assert_eq!(
        *seen.lock().unwrap(),
        vec![("GET".to_string(), "/version".to_string())]
    );
    assert_eq!(rt.settings_service.snapshot(), before);
    rt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_start_body_is_empty_and_cancel_is_harmless() {
    let (router, rt) = app("127.0.0.1", 1).await;
    let (status, body) = send(
        &router,
        "POST",
        "/api/v1/propresenter/scan",
        r#"{"host":"8.8.8.8"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = send(&router, "POST", "/api/v1/propresenter/scan", "[1]").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = send(&router, "POST", "/api/v1/propresenter/scan", "{}").await;
    assert_eq!(status, StatusCode::OK);

    // Cancel stops it; cancelling again, with nothing running, only says idle.
    let (status, cancelled) = send(&router, "POST", "/api/v1/propresenter/scan/cancel", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(
            cancelled["state"].as_str(),
            Some("idle" | "not_found" | "found")
        ),
        "{cancelled}"
    );
    let (status, again) = send(&router, "POST", "/api/v1/propresenter/scan/cancel", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(again["state"], "scanning");
    // Only these verbs: a PUT or a DELETE is not a scan.
    assert_eq!(
        send(&router, "DELETE", "/api/v1/propresenter/scan", "")
            .await
            .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        send(&router, "GET", "/api/v1/propresenter/scan/cancel", "")
            .await
            .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    rt.stop().await;
}
