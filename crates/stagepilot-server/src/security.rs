//! Hardening for the local dashboard: a login throttle (m13), an Origin check on mutating routes
//! and the response security headers (m15).

use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

// ---- login throttle ---------------------------------------------------------------------------

/// Lockout after each further 5 wrong PINs from one address: 30 s, 60 s, 5 min, 15 min, 1 h.
const LOCKOUTS: [f64; 5] = [30.0, 60.0, 300.0, 900.0, 3600.0];
const FAILURES_PER_LOCKOUT: u32 = 5;
/// 30 wrong PINs from anywhere within 10 minutes lock every address out for 5 minutes.
const GLOBAL_FAILURES: usize = 30;
const GLOBAL_WINDOW: f64 = 600.0;
const GLOBAL_LOCKOUT: f64 = 300.0;
/// Addresses remembered at most (expired, unlocked ones are dropped first).
const MAX_TRACKED: usize = 4096;

#[derive(Default)]
struct Entry {
    failures: u32,
    locked_until: f64,
}

#[derive(Default)]
struct State {
    by_address: HashMap<String, Entry>,
    recent: Vec<f64>,
    global_locked_until: f64,
}

pub type Clock = Box<dyn Fn() -> f64 + Send + Sync>;

/// Per-address and global throttle for dashboard PIN logins. The clock counts seconds and is
/// replaceable so tests can move time without sleeping.
pub struct LoginThrottle {
    state: Mutex<State>,
    clock: Mutex<Clock>,
}

impl Default for LoginThrottle {
    fn default() -> Self {
        let start = Instant::now();
        Self::with_clock(Box::new(move || start.elapsed().as_secs_f64()))
    }
}

impl LoginThrottle {
    pub fn with_clock(clock: Clock) -> Self {
        Self {
            state: Mutex::new(State::default()),
            clock: Mutex::new(clock),
        }
    }

    /// Replace the clock (tests).
    pub fn set_clock(&self, clock: Clock) {
        *self.clock.lock().unwrap_or_else(|p| p.into_inner()) = clock;
    }

    fn now(&self) -> f64 {
        (self.clock.lock().unwrap_or_else(|p| p.into_inner()))()
    }

    /// `Err(seconds to wait)` while this address (or everyone) is locked out.
    pub fn check(&self, address: &str) -> Result<(), u64> {
        let now = self.now();
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let own = state
            .by_address
            .get(address)
            .map_or(0.0, |e| e.locked_until);
        let until = own.max(state.global_locked_until);
        if until > now {
            Err((until - now).ceil() as u64)
        } else {
            Ok(())
        }
    }

    /// Count a wrong PIN.
    pub fn failure(&self, address: &str) {
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.by_address.len() >= MAX_TRACKED && !state.by_address.contains_key(address) {
            state
                .by_address
                .retain(|_, e| e.locked_until > now || e.failures % FAILURES_PER_LOCKOUT != 0);
            if state.by_address.len() >= MAX_TRACKED {
                state.by_address.clear();
            }
        }
        let entry = state.by_address.entry(address.to_string()).or_default();
        entry.failures += 1;
        if entry.failures % FAILURES_PER_LOCKOUT == 0 {
            let step = (entry.failures / FAILURES_PER_LOCKOUT - 1) as usize;
            entry.locked_until = now + LOCKOUTS[step.min(LOCKOUTS.len() - 1)];
        }
        state.recent.retain(|t| now - t < GLOBAL_WINDOW);
        state.recent.push(now);
        if state.recent.len() >= GLOBAL_FAILURES {
            state.global_locked_until = now + GLOBAL_LOCKOUT;
            state.recent.clear();
        }
    }

    /// A correct PIN clears the counters.
    pub fn success(&self, address: &str) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.by_address.remove(address);
        state.recent.clear();
        state.global_locked_until = 0.0;
    }
}

// ---- Origin check -----------------------------------------------------------------------------

/// Origins that may call the API besides the server's own address: the desktop shell and the
/// Vite dev server.
const TRUSTED_ORIGINS: [&str; 5] = [
    "http://127.0.0.1:5173",
    "http://localhost:5173",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
];

fn mutating(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// An `Origin` is acceptable when it names this server (same authority as `Host`) or is trusted.
pub fn origin_allowed(origins: &[&str], host: Option<&str>) -> bool {
    let [origin] = origins else {
        return false; // several Origin headers are never legitimate
    };
    if TRUSTED_ORIGINS.contains(origin) {
        return true;
    }
    let (Some(host), Some((scheme, authority))) = (host, origin.split_once("://")) else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && authority.eq_ignore_ascii_case(host)
}

/// Refuse a browser-initiated mutating request from another origin (CSRF). A request without
/// `Origin` (curl, the desktop shell's backend calls, scripts) is not a browser cross-site call.
/// Remote Access requests were checked by the remote ingress and are passed through.
pub async fn origin_guard(request: Request, next: Next) -> Response {
    let remote = request
        .extensions()
        .get::<crate::remote_types::RemoteContext>()
        .is_some();
    if !remote && mutating(request.method()) && request.headers().contains_key(header::ORIGIN) {
        let origins: Vec<&str> = request
            .headers()
            .get_all(header::ORIGIN)
            .iter()
            .map(|v| v.to_str().unwrap_or("\u{0}invalid"))
            .collect();
        let host = request
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok());
        if !origin_allowed(&origins, host) {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({"detail": "Cross-origin request refused."})),
            )
                .into_response();
        }
    }
    next.run(request).await
}

// ---- response headers -------------------------------------------------------------------------

/// Same-origin scripts only (the compiled SPA), same-origin fetch and WebSocket, inline styles
/// (React `style=` attributes), no plugins, no framing, no form posts elsewhere.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; \
connect-src 'self' ws: wss:; object-src 'none'; base-uri 'self'; form-action 'self'; \
frame-ancestors 'none'";

pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    set_security_headers(response.headers_mut());
    response
}

fn set_security_headers(headers: &mut HeaderMap) {
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("content-security-policy", CONTENT_SECURITY_POLICY),
    ] {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
}

/// True when the request reached us over HTTPS through a trusted tunnel.
pub fn forwarded_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("https"))
}

// ---- 500 log ----------------------------------------------------------------------------------

/// Log a WARN with the method and path of every request the router answered with HTTP 500 (m1):
/// a 500 used to leave no line at all. No headers, query string or body are logged.
pub async fn log_server_errors(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    if response.status() == StatusCode::INTERNAL_SERVER_ERROR {
        tracing::warn!(%method, %path, "http_500");
    }
    response
}
