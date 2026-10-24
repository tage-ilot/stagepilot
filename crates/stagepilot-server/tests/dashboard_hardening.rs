//! m13/m15: dashboard PIN lockout, `pin_is_default`, the `Secure` cookie attribute, the Origin
//! check on mutating routes and the security headers.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tracing::Level;

const DEFAULT_PIN: &str = "1234";

mod support;

// ---- rig --------------------------------------------------------------------------------------

struct Rig {
    router: axum::Router,
    runtime: Arc<Runtime>,
    now: Arc<Mutex<f64>>,
}

fn web_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sp-web-{}", uuid_like()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><div id=root></div><script type=\"module\" src=\"/assets/app.js\"></script>",
    )
    .unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log('spa');").unwrap();
    dir
}

fn uuid_like() -> String {
    format!(
        "{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn rig() -> Rig {
    let runtime = Runtime::new(
        Arc::new(SettingsService::ephemeral(RuntimeSettings::demo())),
        false,
    );
    runtime.start().await;
    let now = Arc::new(Mutex::new(1000.0));
    let clock = now.clone();
    runtime
        .login_throttle
        .set_clock(Box::new(move || *clock.lock().unwrap()));
    Rig {
        router: build_app(
            runtime.clone(),
            AppOptions {
                web_root: Some(web_root()),
            },
        ),
        runtime,
        now,
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Rig {
    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        peer: &str,
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
        let peer: SocketAddr = format!("{peer}:50000").parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(peer));
        let response = self.router.clone().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        Reply {
            status: parts.status,
            headers: parts.headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn login(&self, pin: &str, peer: &str, headers: &[(&str, &str)]) -> Reply {
        self.send(
            "POST",
            "/api/v1/dashboard-auth/login",
            Some(json!({"pin": pin})),
            peer,
            headers,
        )
        .await
    }

    fn advance(&self, seconds: f64) {
        *self.now.lock().unwrap() += seconds;
    }

    async fn wrong_pins(&self, n: usize, peer: &str) {
        for _ in 0..n {
            assert_eq!(
                self.login("0000", peer, &[]).await.status,
                StatusCode::UNAUTHORIZED
            );
        }
    }
}

fn retry_after(reply: &Reply) -> u64 {
    reply.headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap()
}

// ---- m13: lockout -----------------------------------------------------------------------------

#[tokio::test]
async fn five_wrong_pins_lock_the_address_for_30_seconds_then_the_lock_escalates() {
    let r = rig().await;
    let lan = "192.168.1.50";
    for expected in [30u64, 60, 300, 900, 3600, 3600] {
        r.wrong_pins(5, lan).await;
        let locked = r.login(DEFAULT_PIN, lan, &[]).await;
        assert_eq!(
            locked.status,
            StatusCode::TOO_MANY_REQUESTS,
            "{}",
            locked.body
        );
        assert_eq!(retry_after(&locked), expected);
        assert!(locked.body["detail"]
            .as_str()
            .unwrap()
            .contains(&expected.to_string()));
        // Still locked a moment later; free after the wait. Even the right PIN waits.
        r.advance(expected as f64 - 1.0);
        assert_eq!(
            r.login(DEFAULT_PIN, lan, &[]).await.status,
            StatusCode::TOO_MANY_REQUESTS
        );
        r.advance(2.0);
    }
}

#[tokio::test]
async fn another_address_is_not_locked_by_the_first_and_a_correct_pin_resets_the_count() {
    let r = rig().await;
    r.wrong_pins(5, "192.168.1.50").await;
    assert_eq!(
        r.login("0000", "192.168.1.50", &[]).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        r.login(DEFAULT_PIN, "192.168.1.51", &[]).await.status,
        StatusCode::OK
    );

    // 4 wrong, a success, 4 wrong: never reaches 5 in a row, so never locks.
    let other = "192.168.1.60";
    r.wrong_pins(4, other).await;
    assert_eq!(
        r.login(DEFAULT_PIN, other, &[]).await.status,
        StatusCode::OK
    );
    r.wrong_pins(4, other).await;
    assert_eq!(
        r.login(DEFAULT_PIN, other, &[]).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn thirty_wrong_pins_in_ten_minutes_lock_everyone_for_five_minutes() {
    let r = rig().await;
    for host in 10..17 {
        r.wrong_pins(4, &format!("10.0.0.{host}")).await; // 28 failures, no address locked
    }
    r.wrong_pins(1, "10.0.0.20").await;
    r.wrong_pins(1, "10.0.0.21").await; // the 30th
    let locked = r.login(DEFAULT_PIN, "10.0.0.99", &[]).await;
    assert_eq!(
        locked.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        locked.body
    );
    assert_eq!(retry_after(&locked), 300);
    r.advance(301.0);
    assert_eq!(
        r.login(DEFAULT_PIN, "10.0.0.99", &[]).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_desktop_shell_is_never_locked_out() {
    let r = rig().await;
    let desktop = [("origin", "tauri://localhost")];
    for _ in 0..60 {
        assert_eq!(
            r.login("0000", "127.0.0.1", &desktop).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        r.login(DEFAULT_PIN, "127.0.0.1", &desktop).await.status,
        StatusCode::OK
    );
    // A loopback client without the desktop origin is an ordinary client.
    r.wrong_pins(5, "127.0.0.1").await;
    assert_eq!(
        r.login("0000", "127.0.0.1", &[]).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    // ... and the desktop origin from another machine is not exempt either.
    r.wrong_pins(5, "192.168.1.70").await;
    assert_eq!(
        r.login("0000", "192.168.1.70", &desktop).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn every_failed_login_is_logged_with_the_address_and_never_the_pin() {
    let logs = support::Capture::start();
    let r = rig().await;
    r.login("9876-secret", "192.168.1.80", &[]).await;
    let warnings: Vec<String> = logs
        .at(Level::WARN)
        .into_iter()
        .filter(|t| t.contains("dashboard_login_failed"))
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("192.168.1.80"));
    assert!(!warnings[0].contains("9876"));
}

// ---- pin_is_default, Secure cookie ------------------------------------------------------------

#[tokio::test]
async fn settings_say_when_the_pin_is_still_the_default() {
    let r = rig().await;
    let before = r
        .send("GET", "/api/v1/settings", None, "127.0.0.1", &[])
        .await;
    assert_eq!(before.body["pin_is_default"], true);
    let set = r
        .send(
            "POST",
            "/api/v1/dashboard-auth/settings",
            Some(json!({"enabled": true, "pin": "4821"})),
            "127.0.0.1",
            &[],
        )
        .await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.body);
    assert_eq!(set.body["pin_is_default"], false);
    let after = r
        .send("GET", "/api/v1/settings", None, "127.0.0.1", &[])
        .await;
    assert_eq!(after.body["pin_is_default"], false);
    let _ = &r.runtime;
}

#[tokio::test]
async fn the_cookie_is_secure_only_over_https() {
    let r = rig().await;
    let plain = r.login(DEFAULT_PIN, "192.168.1.90", &[]).await;
    let cookie = plain.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    assert!(!cookie.contains("Secure"), "{cookie}");
    let tunnel = r
        .login(DEFAULT_PIN, "127.0.0.1", &[("x-forwarded-proto", "https")])
        .await;
    let cookie = tunnel.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.ends_with("; Secure"), "{cookie}");
}

// ---- m15: Origin check ------------------------------------------------------------------------

#[tokio::test]
async fn a_foreign_origin_cannot_make_a_mutating_request() {
    let r = rig().await;
    let path = "/api/v1/playback-api/find/cancel";
    let host = "192.168.1.5:8765";
    for origin in [
        "https://evil.example",
        "http://192.168.1.5:9999",
        "null",
        "http://192.168.1.5",
    ] {
        let reply = r
            .send(
                "POST",
                path,
                None,
                "192.168.1.9",
                &[("host", host), ("origin", origin)],
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{origin}: {}",
            reply.body
        );
        assert_eq!(
            reply.body,
            json!({"detail": "Cross-origin request refused."})
        );
    }
    for method in ["PUT", "PATCH", "DELETE"] {
        let reply = r
            .send(
                method,
                "/api/v1/settings",
                Some(json!({})),
                "192.168.1.9",
                &[("host", host), ("origin", "https://evil.example")],
            )
            .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method}");
    }
    // Two Origin headers are never legitimate.
    let reply = r
        .send(
            "POST",
            path,
            None,
            "192.168.1.9",
            &[
                ("host", host),
                ("origin", "http://192.168.1.5:8765"),
                ("origin", "https://evil.example"),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn same_origin_desktop_dev_and_no_origin_requests_pass() {
    let r = rig().await;
    let path = "/api/v1/playback-api/find/cancel";
    let host = "192.168.1.5:8765";
    for headers in [
        vec![("host", host), ("origin", "http://192.168.1.5:8765")],
        vec![("host", host), ("origin", "HTTP://192.168.1.5:8765")],
        vec![("host", host), ("origin", "tauri://localhost")],
        vec![("host", host), ("origin", "http://tauri.localhost")],
        vec![("host", host), ("origin", "http://localhost:5173")],
        vec![("host", host)],
        vec![],
    ] {
        let reply = r.send("POST", path, None, "192.168.1.9", &headers).await;
        assert_ne!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{headers:?}: {}",
            reply.body
        );
    }
    // Reads are not mutating: a foreign Origin may read (CORS keeps the browser from seeing it).
    let reply = r
        .send(
            "GET",
            "/api/v1/health",
            None,
            "192.168.1.9",
            &[("host", host), ("origin", "https://evil.example")],
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
}

// ---- m15: headers, SPA ------------------------------------------------------------------------

fn assert_secure_headers(headers: &HeaderMap, what: &str) {
    assert_eq!(headers["x-content-type-options"], "nosniff", "{what}");
    assert_eq!(headers["x-frame-options"], "DENY", "{what}");
    assert_eq!(headers["referrer-policy"], "no-referrer", "{what}");
    let csp = headers["content-security-policy"].to_str().unwrap();
    for directive in [
        "default-src 'self'",
        "script-src 'self'",
        "connect-src 'self' ws: wss:",
        "object-src 'none'",
        "frame-ancestors 'none'",
    ] {
        assert!(csp.contains(directive), "{what}: {directive} in {csp}");
    }
}

#[tokio::test]
async fn security_headers_are_on_api_errors_and_the_spa() {
    let r = rig().await;
    for (path, expect) in [
        ("/api/v1/health", StatusCode::OK),
        ("/api/v1/nope", StatusCode::NOT_FOUND),
        ("/", StatusCode::OK),
        ("/assets/app.js", StatusCode::OK),
    ] {
        let reply = r.send("GET", path, None, "127.0.0.1", &[]).await;
        assert_eq!(reply.status, expect, "{path}");
        assert_secure_headers(&reply.headers, path);
    }
    let refused = r
        .send(
            "POST",
            "/api/v1/playback-api/find/cancel",
            None,
            "127.0.0.1",
            &[("host", "a:1"), ("origin", "https://evil.example")],
        )
        .await;
    assert_secure_headers(&refused.headers, "403");
}

#[tokio::test]
async fn the_compiled_spa_is_still_served() {
    let r = rig().await;
    let index = r.send("GET", "/", None, "192.168.1.9", &[]).await;
    assert_eq!(index.status, StatusCode::OK);
    let asset = r
        .send("GET", "/assets/app.js", None, "192.168.1.9", &[])
        .await;
    assert_eq!(asset.status, StatusCode::OK);
    assert!(asset.headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .contains("javascript"));
    // The page only loads same-origin module scripts, which `script-src 'self'` allows.
    assert!(!asset.headers.contains_key(header::SET_COOKIE));
}
