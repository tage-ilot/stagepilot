//! m1: any HTTP 500 the router produces leaves a WARN with the method and path.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use stagepilot_server::security::log_server_errors;
mod support;
use tower::ServiceExt;
use tracing::Level;

#[tokio::test]
async fn a_500_is_logged_with_method_and_path_only() {
    let logs = support::Capture::start();
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
    let warnings: Vec<String> = logs.at(Level::WARN);
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
