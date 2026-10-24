//! B6: the Remote sign-in page was broken in rc.4. The real listener (`remote_listener`) served
//! the ingress without peer addresses, so `GET /api/v1/access` answered 500 "Missing request
//! extension: ConnectInfo<SocketAddr>". Other tests inject `ConnectInfo` by hand and could not
//! see it. These tests start the REAL listener on a loopback port and use real HTTP.

use serde_json::{json, Value};
use stagepilot_core::auth::{csrf_token, RemoteStore};
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_remote::server::{remote_listener, require_https, RemoteListener};
use stagepilot_server::{build_app, AppOptions, RemoteAccess, RemoteIngress, Runtime};
use std::sync::Arc;

const PASSWORD: &str = "a-test-password-only";
const ORIGIN: &str = "https://remote.stagepilot.test";
const COOKIE: &str = "__Host-stagepilot_remote";

struct Live {
    listener: RemoteListener,
    access: Arc<RemoteAccess>,
    client: reqwest::Client,
    _dir: tempfile::TempDir,
}

async fn live() -> Live {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(
        Arc::new(SettingsService::ephemeral(RuntimeSettings::demo())),
        false,
    );
    let access = RemoteAccess::new(RemoteStore::new(dir.path().join("identity.db")));
    runtime.set_remote_access(Some(access.clone()));
    let app = build_app(runtime, AppOptions { web_root: None });
    let router = require_https(
        RemoteIngress::new(access.clone(), ORIGIN)
            .unwrap()
            .wrap(app),
    );
    let listener = remote_listener(router, 0).await.expect("loopback port");
    Live {
        listener,
        access,
        client: reqwest::Client::builder().no_proxy().build().unwrap(),
        _dir: dir,
    }
}

struct Answer {
    status: u16,
    set_cookie: Option<String>,
    body: Value,
}

impl Live {
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, String)],
    ) -> Answer {
        let url = format!("http://{}{path}", self.listener.addr);
        let mut request = self
            .client
            .request(method, url)
            // The tunnel terminates TLS and tells the listener so.
            .header("x-forwarded-proto", "https");
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body.to_string());
        }
        let response = request.send().await.expect("the listener answers");
        let status = response.status().as_u16();
        let set_cookie = response
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let text = response.text().await.unwrap_or_default();
        Answer {
            status,
            set_cookie,
            body: serde_json::from_str(&text).unwrap_or(Value::Null),
        }
    }

    async fn get(&self, path: &str, headers: &[(&str, String)]) -> Answer {
        self.request(reqwest::Method::GET, path, None, headers)
            .await
    }
}

fn cookie_header(token: &str) -> (&'static str, String) {
    ("cookie", format!("{COOKIE}={token}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn access_answers_200_without_a_session_with_a_bad_cookie_and_with_a_valid_session() {
    let live = live().await;
    live.access
        .store
        .bootstrap("operator@example.test", PASSWORD)
        .unwrap();

    let anonymous = live.get("/api/v1/access", &[]).await;
    assert_eq!(anonymous.status, 200, "{:?}", anonymous.body);
    assert_eq!(anonymous.body["mode"], "remote");
    assert_eq!(anonymous.body["authenticated"], json!(false));

    let bad = live
        .get("/api/v1/access", &[cookie_header("not-a-real-session")])
        .await;
    assert_eq!(bad.status, 200, "{:?}", bad.body);
    assert_eq!(bad.body["authenticated"], json!(false));

    let token = live
        .access
        .store
        .login("operator@example.test", PASSWORD, None)
        .unwrap();
    let valid = live.get("/api/v1/access", &[cookie_header(&token)]).await;
    assert_eq!(valid.status, 200, "{:?}", valid.body);
    assert_eq!(valid.body["authenticated"], json!(true));
    assert_eq!(valid.body["csrf_token"], json!(csrf_token(&token)));
}

#[tokio::test(flavor = "multi_thread")]
async fn login_session_and_logout_round_trip_over_real_http() {
    let live = live().await;
    live.access
        .store
        .bootstrap("operator@example.test", PASSWORD)
        .unwrap();
    let origin = ("origin", ORIGIN.to_string());
    let remote = ("x-stagepilot-remote", "1".to_string());

    let login = live
        .request(
            reqwest::Method::POST,
            "/api/v1/remote-auth/login",
            Some(json!({"email": "operator@example.test", "password": PASSWORD})),
            &[origin.clone(), remote.clone()],
        )
        .await;
    assert_eq!(login.status, 200, "{:?}", login.body);
    let set_cookie = login.set_cookie.expect("a session cookie");
    let token = set_cookie
        .split(';')
        .next()
        .and_then(|pair| pair.split_once('='))
        .map(|(_, value)| value.to_string())
        .expect("cookie value");

    let access = live.get("/api/v1/access", &[cookie_header(&token)]).await;
    assert_eq!(access.status, 200, "{:?}", access.body);
    assert_eq!(access.body["authenticated"], json!(true));

    let session = live
        .get("/api/v1/remote-auth/session", &[cookie_header(&token)])
        .await;
    assert_eq!(session.status, 200, "{:?}", session.body);
    assert_eq!(session.body["user"]["email"], "operator@example.test");

    let logout = live
        .request(
            reqwest::Method::POST,
            "/api/v1/remote-auth/logout",
            None,
            &[
                origin,
                cookie_header(&token),
                ("x-csrf-token", csrf_token(&token)),
            ],
        )
        .await;
    assert_eq!(logout.status, 204);

    let after = live
        .get("/api/v1/remote-auth/session", &[cookie_header(&token)])
        .await;
    assert_eq!(after.status, 401);
    let access = live.get("/api/v1/access", &[cookie_header(&token)]).await;
    assert_eq!(access.body["authenticated"], json!(false));
}

/// Every GET route a remote viewer can reach must work through the real listener: none may fail
/// on a missing request extension.
#[tokio::test(flavor = "multi_thread")]
async fn no_remote_readable_route_fails_on_a_missing_extension() {
    let live = live().await;
    live.access
        .store
        .bootstrap("operator@example.test", PASSWORD)
        .unwrap();
    let token = live
        .access
        .store
        .login("operator@example.test", PASSWORD, None)
        .unwrap();
    for path in [
        "/api/v1/access",
        "/api/v1/state",
        "/api/v1/health",
        "/api/v1/health/live",
        "/api/v1/health/ready",
        "/api/v1/remote-auth/session",
        "/api/v1/dashboard-auth/status",
        "/api/v1/settings",
        "/api/v1/remote-access",
    ] {
        let answer = live.get(path, &[cookie_header(&token)]).await;
        assert_ne!(answer.status, 500, "{path}: {:?}", answer.body);
        assert!(
            !answer
                .body
                .to_string()
                .contains("Missing request extension"),
            "{path}: {:?}",
            answer.body
        );
    }
}

/// Defence in depth: even if a listener serves without peer addresses, the handlers must answer
/// (as a non-loopback peer) instead of failing with a 500.
#[tokio::test]
async fn handlers_tolerate_a_missing_peer_address() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let runtime = Runtime::new(
        Arc::new(SettingsService::ephemeral(RuntimeSettings::demo())),
        false,
    );
    let app = build_app(runtime, AppOptions { web_root: None });
    for path in ["/api/v1/access", "/api/v1/dashboard-auth/status"] {
        let request = Request::builder().uri(path).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status().as_u16(), 200, "{path}");
    }
}
