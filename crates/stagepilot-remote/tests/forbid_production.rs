//! The test-run guard: no test may reach a production service. `.cargo/config.toml` sets
//! `STAGEPILOT_FORBID_PRODUCTION_ORIGINS=1` for `cargo test` and `cargo nextest`; the real
//! transport then refuses production hosts before it opens a socket (these tests need no network).

use stagepilot_remote::bootstrap::DEFAULT_CONTROL_PLANE_ORIGIN;
use stagepilot_remote::http::{
    expecting_refusal, HttpClient, HttpRequest, HttpTransport, ReqwestTransport,
    FORBID_PRODUCTION_ENV,
};
use stagepilot_remote::ip_location::LOOKUP_URL;
use std::sync::Arc;

const PREFIX: &str = "refusing to contact production in a test: ";

/// Without `expecting_refusal` a refusal ends the process (that is the point of the guard).
fn get(url: &str) -> Result<stagepilot_remote::http::HttpResponse, String> {
    expecting_refusal(|| {
        ReqwestTransport::new()
            .send(&HttpRequest {
                method: "GET".into(),
                url: url.into(),
                timeout_secs: 2.0,
                ..Default::default()
            })
            .map_err(|e| e.0)
    })
}

#[test]
fn the_test_runner_turns_the_guard_on() {
    // Fails if `.cargo/config.toml` stops reaching `cargo test` or `cargo nextest`.
    assert_eq!(std::env::var(FORBID_PRODUCTION_ENV).as_deref(), Ok("1"));
}

#[test]
fn the_real_transport_refuses_the_control_plane_and_names_the_host() {
    let error = get(&format!("{DEFAULT_CONTROL_PLANE_ORIGIN}/v1/installations")).unwrap_err();
    assert!(error.starts_with(PREFIX), "{error}");
    assert!(
        error.contains("stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev"),
        "{error}"
    );
}

#[test]
fn the_real_transport_refuses_the_city_lookup_service() {
    let error = get(LOOKUP_URL).unwrap_err();
    assert!(error.starts_with(PREFIX), "{error}");
    assert!(error.contains("api.db-ip.com"), "{error}");
}

#[test]
fn an_http_client_on_the_real_transport_is_refused_too() {
    let client = HttpClient::new(
        DEFAULT_CONTROL_PLANE_ORIGIN,
        2.0,
        Arc::new(ReqwestTransport::new()),
    );
    let error = expecting_refusal(|| client.get("/v1/installations/x/status")).unwrap_err();
    assert!(error.0.starts_with(PREFIX), "{}", error.0);
}

#[test]
fn a_loopback_fake_is_not_refused() {
    // Port 9 (discard) is closed: the request fails to connect, but not because of the guard.
    let error = get("http://127.0.0.1:9/").unwrap_err();
    assert!(!error.starts_with(PREFIX), "{error}");
}
