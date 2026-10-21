//! Trusted remote ingress: the Rust counterpart of `api/remote_ingress.py::RemoteIngress`.
//!
//! Never select this boundary (or bypass it) using Host, Origin, IP or proxy headers. Only the
//! launcher wraps the router with [`RemoteIngress::wrap`] and exposes the result on the dedicated
//! remote listener. Origin is an additional browser CSRF check, never authentication. Wrapping
//! does not start a second runtime: the owning LAN app alone starts and stops the plugins.

use crate::remote_types::{RemoteAccess, RemoteContext, ACCESS_PATH, AUTH_PREFIX, COOKIE_NAME};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::json;
use stagepilot_core::auth::{csrf_token, RemoteAuthError, RemoteRole};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use url::Url;

const VIEWER_READ_PATHS: [&str; 5] = [
    "/api/v1/state",
    "/api/v1/health",
    "/api/v1/health/live",
    "/api/v1/health/ready",
    "/api/v1/remote-auth/session",
];
const OWN_MUTATIONS: [&str; 2] = [
    "/api/v1/remote-auth/logout",
    "/api/v1/remote-auth/revoke-sessions",
];
/// Bound auth payloads before JSON parsing / Argon2 allocation.
const MAX_AUTH_BODY: usize = 16_384;
const MAX_CONNECTIONS: usize = 32;

pub struct RemoteIngress {
    access: Arc<RemoteAccess>,
    public_origin: String,
    connections: Arc<Semaphore>,
}

impl RemoteIngress {
    /// `public_origin` must be an exact HTTPS origin without a path, query, fragment or
    /// credentials (the Python constructor raises `ValueError`).
    pub fn new(access: Arc<RemoteAccess>, public_origin: &str) -> Result<Arc<Self>, String> {
        let invalid = || "Remote public_origin must be an exact HTTPS origin without a path.";
        let parsed = Url::parse(public_origin).map_err(|_| invalid())?;
        let after_scheme = public_origin.strip_prefix("https://").unwrap_or("");
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || after_scheme.contains(['/', '?', '#', '@'])
        {
            return Err(invalid().into());
        }
        Ok(Arc::new(Self {
            access,
            public_origin: public_origin.to_string(),
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        }))
    }

    /// Put the ingress in front of an already built application router.
    pub fn wrap(self: &Arc<Self>, app: Router) -> Router {
        app.layer(middleware::from_fn_with_state(self.clone(), ingress))
    }
}

fn deny(status: u16, detail: &str, websocket: bool) -> Response {
    if websocket {
        // Starlette refuses a handshake that is closed before accept with a 403.
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        json!({ "detail": detail }).to_string(),
    )
        .into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if status == 429 {
        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("300"));
    }
    response
}

fn is_websocket(request: &Request) -> bool {
    request.method() == Method::GET
        && request
            .headers()
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

async fn ingress(State(ing): State<Arc<RemoteIngress>>, request: Request, next: Next) -> Response {
    let websocket = is_websocket(&request);
    let Ok(_permit) = ing.connections.clone().try_acquire_owned() else {
        return deny(503, "Remote access is busy.", websocket);
    };
    match authorise(&ing, request, websocket).await {
        Ok(request) => next.run(request).await,
        Err(error) => deny(error.status, &error.detail, websocket),
    }
}

fn denial(status: u16, detail: &str) -> RemoteAuthError {
    RemoteAuthError {
        status,
        detail: detail.to_string(),
    }
}

/// `RemoteIngress._request`: authorise, attach the [`RemoteContext`], or refuse.
async fn authorise(
    ing: &RemoteIngress,
    mut request: Request,
    websocket: bool,
) -> Result<Request, RemoteAuthError> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let mutation = !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS);
    // Reject duplicate Origin headers as well as absent/mismatched ones.
    let origins: Vec<&[u8]> = request
        .headers()
        .get_all(header::ORIGIN)
        .iter()
        .map(HeaderValue::as_bytes)
        .collect();
    if (mutation || websocket) && origins != [ing.public_origin.as_bytes()] {
        return Err(denial(403, "Remote same-origin request required."));
    }
    if websocket && path != "/ws" {
        return Err(denial(403, "Remote WebSocket path is not permitted."));
    }
    if path.starts_with("/api/v1/dashboard-auth") && path != "/api/v1/dashboard-auth/settings" {
        return Err(denial(
            403,
            "LAN PIN authentication is not available remotely.",
        ));
    }
    let read = matches!(method, Method::GET | Method::HEAD);
    let context = |token: Option<String>, session| RemoteContext {
        access: ing.access.clone(),
        token,
        session,
    };
    let public_asset = !websocket
        && read
        && (matches!(path.as_str(), "/" | "/index.html" | "/stagepilot-icon.png")
            || path.starts_with("/assets/"));
    if public_asset {
        // Only the compiled login shell and assets, never API state or configuration.
        request.extensions_mut().insert(context(None, None));
        return Ok(request);
    }
    let token = cookie(request.headers(), COOKIE_NAME);
    if !websocket && method == Method::GET && path == ACCESS_PATH {
        let session = lookup(&ing.access, token.clone()).await?;
        request.extensions_mut().insert(context(token, session));
        return Ok(request);
    }
    let login = !websocket && method == Method::POST && path == format!("{AUTH_PREFIX}/login");
    let session = if login {
        if request
            .headers()
            .get("x-stagepilot-remote")
            .is_none_or(|v| v.as_bytes() != b"1")
        {
            return Err(denial(403, "Remote login header required."));
        }
        None
    } else {
        let Some(session) = lookup(&ing.access, token.clone()).await? else {
            return Err(denial(401, "Remote email/password session required."));
        };
        if mutation {
            let sent = request
                .headers()
                .get("x-csrf-token")
                .map(HeaderValue::as_bytes)
                .unwrap_or_default();
            let valid = token
                .as_deref()
                .is_some_and(|t| !t.is_empty() && bool::from(sent.ct_eq(csrf_token(t).as_bytes())));
            if !valid {
                return Err(denial(403, "Remote CSRF token required."));
            }
        }
        let allowed = (websocket && path == "/ws")
            || (read && VIEWER_READ_PATHS.contains(&path.as_str()))
            || (method == Method::POST && OWN_MUTATIONS.contains(&path.as_str()));
        if session.role == RemoteRole::Viewer && !allowed {
            return Err(denial(403, "Viewer access is read-only."));
        }
        Some(session)
    };
    request.extensions_mut().insert(context(token, session));
    if path.starts_with(AUTH_PREFIX) && mutation {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, MAX_AUTH_BODY)
            .await
            .map_err(|_| denial(413, "Remote authentication payload is too large."))?;
        return Ok(Request::from_parts(parts, Body::from(bytes)));
    }
    Ok(request)
}

async fn lookup(
    access: &Arc<RemoteAccess>,
    token: Option<String>,
) -> Result<Option<stagepilot_core::auth::RemoteSession>, RemoteAuthError> {
    access
        .call(move |store| store.session(token.as_deref()))
        .await
}
