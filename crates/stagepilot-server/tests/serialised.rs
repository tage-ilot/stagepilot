//! Port of `test_restartless_serialised.py`: concurrent connection changes end consistent (the
//! live plugins always equal the final saved settings), and a running timer still refuses a
//! presentation switch with 409 under concurrency. ProPresenter points at a closed loopback
//! port, so nothing outside this machine is contacted.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

const ROUNDS: usize = 15;

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

async fn app() -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral(RuntimeSettings::demo());
    let runtime = Runtime::new(Arc::new(service), false);
    runtime.start().await;
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    )
}

/// `_put`: read the saved settings, mutate them, write them back.
async fn put_presentation(router: &axum::Router, on: bool) -> StatusCode {
    let (_, current) = call(router, "GET", "/api/v1/settings", None).await;
    let mut settings = current["settings"].clone();
    settings["integration_modes"]["timer_output"] =
        json!(if on { "propresenter" } else { "simulated" });
    settings["propresenter"]["enabled"] = json!(on);
    settings["propresenter"]["host"] = json!("127.0.0.1");
    settings["propresenter"]["port"] = json!(1);
    call(router, "PUT", "/api/v1/settings", Some(settings))
        .await
        .0
}

async fn presentation_consistent(router: &axum::Router, runtime: &Runtime) {
    let (_, body) = call(router, "GET", "/api/v1/settings", None).await;
    let s = &body["settings"];
    let wanted = s["propresenter"]["enabled"] == json!(true)
        && s["integration_modes"]["timer_output"] == json!("propresenter");
    assert_eq!(runtime.plugins.has("propresenter"), wanted, "settings {s}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_presentation_saves_stay_consistent() {
    let (router, runtime) = app().await;
    for _ in 0..ROUNDS {
        let jobs: Vec<_> = (0..6)
            .map(|i| {
                let router = router.clone();
                tokio::spawn(async move { put_presentation(&router, i % 2 == 0).await })
            })
            .collect();
        for job in jobs {
            assert_eq!(job.await.unwrap(), StatusCode::OK);
        }
        presentation_consistent(&router, &runtime).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_timer_still_refuses_a_presentation_switch_under_concurrency() {
    let (router, runtime) = app().await;
    call(&router, "POST", "/api/v1/actions/start_next", None).await;
    let jobs: Vec<_> = (0..4)
        .map(|_| {
            let router = router.clone();
            tokio::spawn(async move { put_presentation(&router, true).await })
        })
        .collect();
    for job in jobs {
        assert_eq!(job.await.unwrap(), StatusCode::CONFLICT);
    }
    assert!(!runtime.plugins.has("propresenter"));
    presentation_consistent(&router, &runtime).await;
}
