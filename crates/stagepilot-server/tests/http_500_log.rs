//! m1: any HTTP 500 the router produces leaves a WARN with the method and path.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use stagepilot_server::security::log_server_errors;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber};

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<(Level, String)>>>);
struct Text(String);
impl Visit for Text {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!("{}={:?} ", field.name(), value));
    }
}
impl Subscriber for Logs {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut text = Text(String::new());
        event.record(&mut text);
        self.0
            .lock()
            .unwrap()
            .push((*event.metadata().level(), text.0));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn a_500_is_logged_with_method_and_path_only() {
    let logs = Logs::default();
    let _guard = tracing::subscriber::set_default(logs.clone());
    let app = Router::new()
        .route(
            "/boom",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .route("/fine", get(|| async { "ok" }))
        .route(
            "/missing-handler-state",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        )
        .layer(middleware::from_fn(log_server_errors));
    let send = |method: &str, uri: &str| {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", "Bearer super-secret")
            .body(Body::from("pin=1234"))
            .unwrap();
        let app = app.clone();
        async move { app.oneshot(request).await.unwrap().status() }
    };
    assert_eq!(
        send("POST", "/boom?token=abc").await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(send("GET", "/fine").await, StatusCode::OK);
    assert_eq!(
        send("GET", "/missing-handler-state").await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let warnings: Vec<String> = logs
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(l, _)| *l == Level::WARN)
        .map(|(_, t)| t.clone())
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("http_500")
            && warnings[0].contains("POST")
            && warnings[0].contains("/boom")
    );
    for secret in ["super-secret", "token=abc", "1234"] {
        assert!(!warnings[0].contains(secret), "{warnings:?}");
    }
}
