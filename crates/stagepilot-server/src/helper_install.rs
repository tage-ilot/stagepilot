//! "Set up the Lighting computer": StagePilot hands the Lightkey helper (`lightkey-api`) to the
//! Lighting Mac itself, so installing it needs no internet, no GitHub and no SSH.
//!
//! * `POST /api/v1/lights/lightkey/helper-install` opens (or extends) a **setup session**: a random
//!   128-bit token held in memory only, valid for 30 minutes, ended by `DELETE` of the same path or
//!   by the helper connecting. The reply carries the one command to paste into Terminal.
//! * `GET /lightkey-helper/<token>/install.sh` is a generated POSIX `sh` wrapper;
//!   `GET /lightkey-helper/<token>/files/<name>` serves exactly the five vendored helper files.
//!   Unknown or expired token: `404`, empty body. More than [`MAX_REQUESTS`] requests: `404`.
//!   A source address that is not private (RFC 1918, link-local, loopback): `403`. The token is
//!   compared in constant time and is never logged.
//! * These routes are never reachable through the remote tunnel: the remote ingress refuses the
//!   prefix, and the handlers refuse any request that carries a `RemoteContext`.
//! * When the main listener is on `0.0.0.0` (LAN access on) the routes are served there; otherwise
//!   a short-lived extra listener is bound to the one LAN address that routes to the Lighting host
//!   (never `0.0.0.0`) on an ephemeral port, and closed when the session ends.

use crate::remote_types::RemoteContext;
use crate::runtime::Runtime;
use crate::validation::{http_error, ok, Json};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, post};
use axum::Router;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stagepilot_lights::lightkey_helper_assets as embedded;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use subtle::ConstantTimeEq;

/// How long a session lasts, and how far "extend" pushes it.
pub const SESSION_MS: u64 = 30 * 60 * 1000;
/// Requests one token may make. The wrapper makes six; the rest is room for a retry.
pub const MAX_REQUESTS: u32 = 30;
/// Shown when no LAN address that routes to the Lighting host can be found.
pub const NO_ADDRESS: &str = "Could not find this computer's address on the Lighting network";
/// The URL prefix. Keep it off the remote allow-list (`remote_ingress`).
pub const PREFIX: &str = "/lightkey-helper";

type Clock = Box<dyn Fn() -> u64 + Send + Sync>;

// ---- the files and the wrapper -------------------------------------------------------------------

/// The five served files, the version they are, and what the wrapper must know about them.
#[derive(Clone)]
pub struct Assets {
    files: Vec<(String, Vec<u8>)>,
    version: String,
}

impl Assets {
    /// The helper files compiled into this binary.
    pub fn embedded() -> Self {
        let files = embedded::FILE_NAMES
            .iter()
            .filter_map(|name| Some((name.to_string(), embedded::file(name)?.to_vec())))
            .collect();
        Self {
            files,
            version: embedded::bundled_version().to_string(),
        }
    }

    /// Files given by the caller (tests); the names must be the five served names.
    pub fn from_files(files: Vec<(String, Vec<u8>)>, version: &str) -> Self {
        Self {
            files,
            version: version.to_string(),
        }
    }

    /// The files, mutable (tests alter one to prove the wrapper notices).
    #[doc(hidden)]
    pub fn files_mut(&mut self) -> &mut Vec<(String, Vec<u8>)> {
        &mut self.files
    }

    fn get(&self, name: &str) -> Option<&[u8]> {
        // Only the five names are ever served, whatever `files` holds.
        embedded::FILE_NAMES
            .contains(&name)
            .then(|| self.files.iter().find(|(n, _)| n == name))
            .flatten()
            .map(|(_, bytes)| bytes.as_slice())
    }

    fn sha256(&self, name: &str) -> String {
        hex::encode(Sha256::digest(self.get(name).unwrap_or_default()))
    }

    /// The bundled installer understands `--allow-for <ip>` (helper 0.5.0 and later).
    fn supports_allow_for(&self) -> bool {
        self.get("install.sh")
            .is_some_and(|b| String::from_utf8_lossy(b).contains("--allow-for"))
    }

    /// The generated `sh` wrapper for one session.
    pub fn wrapper(&self, base_url: &str, lan_ip: Ipv4Addr) -> String {
        let mut wants = String::new();
        for name in embedded::FILE_NAMES {
            wants.push_str(&format!("    {name}) want='{}' ;;\n", self.sha256(name)));
        }
        let (flag, flag_note) = if self.supports_allow_for() {
            (format!(" --allow-for {lan_ip}"), "")
        } else {
            (
                String::new(),
                "# This helper's installer has no --allow-for; the owner vendors 0.5.0 before shipping.\n",
            )
        };
        let names = embedded::FILE_NAMES.join(" ");
        format!(
            r#"#!/bin/sh
# StagePilot: set up the Lighting computer (Lightkey helper {version}).
# Downloads the helper from StagePilot at the address below (this local network only), checks
# every file against the checksums written in this script, then runs the helper's own installer.
{flag_note}set -eu

BASE='{base_url}'

fail() {{
  printf 'StagePilot setup failed: %s\n' "$*" >&2
  exit 1
}}

sha256_of() {{
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{{print $1}}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{{print $1}}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | awk '{{print $NF}}'
  else
    fail "this computer has no shasum, sha256sum or openssl to check the download."
  fi
}}

main() {{
  command -v curl >/dev/null 2>&1 || fail "curl is not available on this computer."
  TMP=$(mktemp -d "${{TMPDIR:-/tmp}}/stagepilot-helper.XXXXXX") || fail "could not make a temporary folder."
  trap 'cd /; rm -rf "$TMP"' EXIT
  trap 'exit 1' HUP INT TERM
  printf 'Downloading the Lightkey helper {version} from StagePilot ...\n'
  for name in {names}; do
    case "$name" in
{wants}    esac
    curl -fsS "$BASE/files/$name" -o "$TMP/$name" ||
      fail "could not download $name from StagePilot. Is StagePilot still waiting? The command works for 30 minutes."
    have=$(sha256_of "$TMP/$name")
    [ "$have" = "$want" ] || fail "$name does not match its checksum; nothing was installed."
  done
  for name in install.sh lightkey_api.py lightkey_cues.py uninstall.sh; do
    listed=$(awk -v n="$name" '$2 == n {{print $1}}' "$TMP/SHA256SUMS")
    have=$(sha256_of "$TMP/$name")
    [ -n "$listed" ] && [ "$listed" = "$have" ] || fail "$name is not listed with the same checksum in SHA256SUMS; nothing was installed."
  done
  cd "$TMP"
  printf 'Installing ...\n'
  sh ./install.sh{flag} </dev/null || fail "the helper installer reported an error (see above)."
  printf 'Done. Go back to StagePilot.\n'
}}

main
"#,
            version = self.version,
        )
    }
}

// ---- the session -------------------------------------------------------------------------------

/// Closing the extra listener when the session ends.
pub struct ListenerGuard(tokio::task::JoinHandle<()>);

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Session {
    id: u64,
    token: String,
    expires_ms: u64,
    requests: u32,
    lan_ip: Ipv4Addr,
    port: u16,
    _listener: Option<ListenerGuard>,
    started_connected: bool,
    initial_version: Option<String>,
}

#[derive(Default)]
struct State_ {
    session: Option<Session>,
    next_id: u64,
}

/// What the caller may tell the user about a session. The token is in `url`/`command` only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: u64,
    pub token: String,
    pub expires_ms: u64,
    pub lan_ip: Ipv4Addr,
    pub port: u16,
}

impl SessionInfo {
    pub fn url(&self) -> String {
        format!("{}/install.sh", self.base_url())
    }

    pub fn base_url(&self) -> String {
        format!(
            "http://{}:{}{PREFIX}/{}",
            self.lan_ip, self.port, self.token
        )
    }

    pub fn command(&self) -> String {
        format!("curl -fsS {} | sh", self.url())
    }
}

/// Why a served request was refused. All three answer with an empty body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// The source address is not on a private network.
    Forbidden,
    /// No such token, an expired one, or one that used up its requests.
    NotFound,
}

pub struct HelperInstall {
    state: Mutex<State_>,
    clock: Mutex<Clock>,
    assets: Assets,
    /// Serialises "look for a session, maybe open a listener, start one" across requests.
    pub(crate) create_lock: tokio::sync::Mutex<()>,
}

impl Default for HelperInstall {
    fn default() -> Self {
        Self::new()
    }
}

impl HelperInstall {
    pub fn new() -> Self {
        Self::with_assets(Assets::embedded())
    }

    pub fn with_assets(assets: Assets) -> Self {
        Self {
            state: Mutex::new(State_::default()),
            clock: Mutex::new(Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64)
            })),
            assets,
            create_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Replace the clock, milliseconds since the epoch (tests).
    pub fn set_clock(&self, clock: Clock) {
        *self.clock.lock().unwrap_or_else(|p| p.into_inner()) = clock;
    }

    pub fn assets(&self) -> &Assets {
        &self.assets
    }

    fn now(&self) -> u64 {
        (self.clock.lock().unwrap_or_else(|p| p.into_inner()))()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State_> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Drop the session when it has expired. Returns the live one.
    fn live<'a>(&self, state: &'a mut State_) -> Option<&'a mut Session> {
        if state
            .session
            .as_ref()
            .is_some_and(|s| s.expires_ms <= self.now())
        {
            state.session = None;
        }
        state.session.as_mut()
    }

    fn info(session: &Session) -> SessionInfo {
        SessionInfo {
            id: session.id,
            token: session.token.clone(),
            expires_ms: session.expires_ms,
            lan_ip: session.lan_ip,
            port: session.port,
        }
    }

    /// A live session for this address, with its expiry pushed out to a full 30 minutes. It keeps
    /// its token and its request count.
    pub fn extend(&self, lan_ip: Ipv4Addr) -> Option<SessionInfo> {
        let mut state = self.lock();
        let expires = self.now() + SESSION_MS;
        let session = self.live(&mut state)?;
        if session.lan_ip != lan_ip {
            return None;
        }
        session.expires_ms = expires;
        Some(Self::info(session))
    }

    /// Start a session (a new random token), replacing and closing any earlier one.
    pub fn begin(
        &self,
        lan_ip: Ipv4Addr,
        port: u16,
        listener: Option<ListenerGuard>,
        started_connected: bool,
        initial_version: Option<String>,
    ) -> Result<SessionInfo, String> {
        let token = new_token()?;
        let mut state = self.lock();
        state.next_id += 1;
        let session = Session {
            id: state.next_id,
            token,
            expires_ms: self.now() + SESSION_MS,
            requests: 0,
            lan_ip,
            port,
            _listener: listener,
            started_connected,
            initial_version,
        };
        let info = Self::info(&session);
        state.session = Some(session);
        Ok(info)
    }

    /// End the session (drops its listener). `true` when there was one.
    pub fn end(&self) -> bool {
        self.lock().session.take().is_some()
    }

    pub fn is_active(&self) -> bool {
        let mut state = self.lock();
        self.live(&mut state).is_some()
    }

    /// The id of the live session, if any.
    pub fn current_id(&self) -> Option<u64> {
        let mut state = self.lock();
        self.live(&mut state).map(|s| s.id)
    }

    /// The helper is up: end the session once the thing it was opened for has happened. A session
    /// opened with no helper ends when the helper answers; one opened to update an older helper
    /// ends when the answering helper's version is no longer the one it started with.
    pub fn observe(&self, connected: bool, version: Option<&str>) -> bool {
        let mut state = self.lock();
        let Some(session) = self.live(&mut state) else {
            return false;
        };
        if !connected {
            return false;
        }
        let done = !session.started_connected
            || session.initial_version.as_deref() != version
            || version.is_some_and(|v| !stagepilot_lights::lightkey::helper_is_outdated(Some(v)));
        if done {
            state.session = None;
        }
        done
    }

    /// Check one request. Source first (so a stranger learns nothing), then the token.
    pub fn access(&self, peer: Option<IpAddr>, token: &str) -> Result<SessionInfo, Denied> {
        if !peer.is_some_and(is_private) {
            return Err(Denied::Forbidden);
        }
        let mut state = self.lock();
        let session = self.live(&mut state).ok_or(Denied::NotFound)?;
        // Constant time on the bytes; a different length is not a secret.
        if !bool::from(session.token.as_bytes().ct_eq(token.as_bytes())) {
            return Err(Denied::NotFound);
        }
        if session.requests >= MAX_REQUESTS {
            return Err(Denied::NotFound);
        }
        session.requests += 1;
        Ok(Self::info(session))
    }
}

fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "The system could not make a random token.")?;
    Ok(hex::encode(bytes))
}

/// RFC 1918, link-local and loopback (and the IPv6 equivalents; IPv4-mapped addresses count as
/// their IPv4 form).
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

// ---- finding this computer's address on the Lighting network -------------------------------------

/// The local address the system would use to reach `target` (the UDP `connect()` trick: nothing is
/// sent). `None` for an address that is no use: loopback or unspecified.
pub fn local_address_toward(target: SocketAddr) -> Option<Ipv4Addr> {
    let bind = if target.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = std::net::UdpSocket::bind(bind).ok()?;
    socket.connect(target).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Resolve `host` (an address or a name) and find the local address that routes to it.
pub async fn detect_lan_ip(host: &str, port: u16) -> Option<Ipv4Addr> {
    let found = tokio::net::lookup_host((host, port)).await.ok()?;
    found
        .filter(SocketAddr::is_ipv4)
        .find_map(local_address_toward)
}

/// Host and port of the saved Lighting API address.
fn lighting_target(rt: &Runtime) -> Option<(String, u16)> {
    let lights = rt.settings_service.effective_runtime_settings().lights;
    let url = stagepilot_lights::lightkey::effective_api_url(&lights.lightkey, &lights.osc_host);
    let parsed = url::Url::parse(&url).ok()?;
    Some((
        parsed.host_str()?.trim_matches(['[', ']']).to_string(),
        parsed.port().unwrap_or(8770),
    ))
}

// ---- serving -----------------------------------------------------------------------------------

fn empty(status: StatusCode) -> Response {
    let mut response = status.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn denied(reason: Denied) -> Response {
    match reason {
        Denied::Forbidden => empty(StatusCode::FORBIDDEN),
        Denied::NotFound => empty(StatusCode::NOT_FOUND),
    }
}

fn body(content_type: &'static str, bytes: Vec<u8>) -> Response {
    let mut response = (StatusCode::OK, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// The check every served request goes through.
fn admit(svc: &HelperInstall, request: &Request, token: &str) -> Result<SessionInfo, Denied> {
    // Never through the remote tunnel: only the trusted ingress sets this.
    if request.extensions().get::<RemoteContext>().is_some() {
        return Err(Denied::NotFound);
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    svc.access(peer, token)
}

/// What a path under the prefix asks for.
enum Wanted<'a> {
    Wrapper(&'a str),
    File(&'a str, &'a str),
}

/// `token/install.sh` or `token/files/<name>`; anything else (a listing, a deeper path, `..`) is
/// nothing.
fn wanted(rest: &str) -> Option<Wanted<'_>> {
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [token, "install.sh"] if !token.is_empty() => Some(Wanted::Wrapper(token)),
        [token, "files", name] if !token.is_empty() => Some(Wanted::File(token, name)),
        _ => None,
    }
}

async fn dispatch(
    State(svc): State<Arc<HelperInstall>>,
    Path(rest): Path<String>,
    request: Request,
) -> Response {
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) {
        return empty(StatusCode::NOT_FOUND);
    }
    match wanted(&rest) {
        None => empty(StatusCode::NOT_FOUND),
        Some(Wanted::Wrapper(token)) => match admit(&svc, &request, token) {
            Ok(info) => body(
                "text/x-shellscript; charset=utf-8",
                svc.assets
                    .wrapper(&info.base_url(), info.lan_ip)
                    .into_bytes(),
            ),
            Err(reason) => denied(reason),
        },
        Some(Wanted::File(token, name)) => {
            if let Err(reason) = admit(&svc, &request, token) {
                return denied(reason);
            }
            match svc.assets.get(name) {
                Some(bytes) => body(embedded::content_type(name), bytes.to_vec()),
                None => empty(StatusCode::NOT_FOUND),
            }
        }
    }
}

async fn nothing_here() -> Response {
    empty(StatusCode::NOT_FOUND)
}

/// The served routes alone: the main app merges them, and the extra listener serves only these.
/// One catch-all under the prefix, so no path below it ever falls through to the dashboard files.
pub fn served_routes<S: Clone + Send + Sync + 'static>(svc: Arc<HelperInstall>) -> Router<S> {
    Router::new()
        .route("/lightkey-helper", any(nothing_here))
        .route("/lightkey-helper/", any(nothing_here))
        .route("/lightkey-helper/{*rest}", any(dispatch))
        .with_state(svc)
}

/// The app the extra listener serves.
fn listener_app(svc: Arc<HelperInstall>) -> Router {
    served_routes::<()>(svc)
        .layer(axum::middleware::from_fn(crate::security::security_headers))
        .fallback(nothing_here)
}

/// Serve the install routes on `ip:0` until the guard is dropped.
async fn open_listener(
    svc: Arc<HelperInstall>,
    ip: Ipv4Addr,
) -> std::io::Result<(u16, ListenerGuard)> {
    let listener = tokio::net::TcpListener::bind((ip, 0)).await?;
    let port = listener.local_addr()?.port();
    let app = listener_app(svc);
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    Ok((port, ListenerGuard(task)))
}

// ---- the routes the dashboard calls ------------------------------------------------------------------

fn failure(status: u16, code: &str, detail: &str) -> Json {
    Json(
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        json!({ "detail": detail, "code": code }),
    )
}

/// The main listener already reaches the LAN (LAN access on).
fn main_listener_is_lan(rt: &Runtime, lan_ip: Ipv4Addr) -> bool {
    match rt.settings.bind_host.parse::<IpAddr>() {
        Ok(ip) => ip.is_unspecified() || ip == IpAddr::V4(lan_ip),
        Err(_) => false,
    }
}

fn expires_text(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn reply(info: &SessionInfo) -> Json {
    ok(json!({
        "command": info.command(),
        "url": info.url(),
        "expires_at": expires_text(info.expires_ms),
        "lan_ip": info.lan_ip.to_string(),
        "port": info.port,
        "bundled_version": embedded::bundled_version(),
    }))
}

/// `POST /api/v1/lights/lightkey/helper-install` `{address?}`.
pub async fn create(State(rt): State<Arc<Runtime>>, raw: Bytes) -> Json {
    let override_ip = match parse_address(&raw) {
        Ok(ip) => ip,
        Err(response) => return response,
    };
    let svc = rt.helper_install.clone();
    let _one_at_a_time = svc.create_lock.lock().await;
    let lan_ip = match override_ip {
        Some(ip) => Some(ip),
        None => match lighting_target(&rt) {
            Some((host, port)) => detect_lan_ip(&host, port).await,
            None => None,
        },
    };
    let Some(lan_ip) = lan_ip else {
        return failure(409, "no_lan_address", NO_ADDRESS);
    };
    if let Some(info) = svc.extend(lan_ip) {
        return reply(&info);
    }
    let (port, guard) = if main_listener_is_lan(&rt, lan_ip) {
        (rt.settings.bind_port, None)
    } else {
        match open_listener(svc.clone(), lan_ip).await {
            Ok((port, guard)) => (port, Some(guard)),
            Err(_) => {
                return failure(
                    409,
                    "no_lan_address",
                    &format!("{NO_ADDRESS} ({lan_ip} could not be opened for the installer)."),
                )
            }
        }
    };
    let status = rt.outputs.lights.lightkey_status().await;
    let connected = status.link == stagepilot_lights::lightkey::LightkeyLink::Connected;
    let version = status.helper.and_then(|h| h.version);
    let info = match svc.begin(lan_ip, port, guard, connected, version) {
        Ok(info) => info,
        Err(message) => return http_error(500, &message),
    };
    watch(Arc::downgrade(&rt), svc.clone(), info.id);
    reply(&info)
}

/// `DELETE /api/v1/lights/lightkey/helper-install`.
pub async fn delete(State(rt): State<Arc<Runtime>>) -> Json {
    ok(json!({ "ended": rt.helper_install.end() }))
}

fn parse_address(raw: &[u8]) -> Result<Option<Ipv4Addr>, Json> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let bad = || {
        failure(
            422,
            "bad_address",
            "That is not an address on your network. Use four numbers such as 192.168.1.20.",
        )
    };
    let value: Value = serde_json::from_slice(raw).map_err(|_| bad())?;
    let Some(object) = value.as_object() else {
        return Err(bad());
    };
    match object.get("address") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
        Some(Value::String(text)) => text
            .trim()
            .parse::<Ipv4Addr>()
            .ok()
            .filter(|ip| !ip.is_loopback() && !ip.is_unspecified() && is_private(IpAddr::V4(*ip)))
            .map(Some)
            .ok_or_else(bad),
        Some(_) => Err(bad()),
    }
}

/// Ends the session when it expires or the helper answers. One task per session.
fn watch(rt: Weak<Runtime>, svc: Arc<HelperInstall>, id: u64) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if svc.current_id() != Some(id) {
                return;
            }
            let Some(rt) = rt.upgrade() else {
                return;
            };
            let status = rt.outputs.lights.lightkey_status().await;
            let connected = status.link == stagepilot_lights::lightkey::LightkeyLink::Connected;
            let version = status.helper.and_then(|h| h.version);
            svc.observe(connected, version.as_deref());
        }
    });
}

/// The dashboard routes (`/api/v1/lights/lightkey/helper-install`).
pub fn api_routes() -> Router<Arc<Runtime>> {
    Router::new().route(
        "/api/v1/lights/lightkey/helper-install",
        post(create).delete(delete),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const LAN: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 20);
    const PEER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50));

    /// A service whose clock the test moves.
    fn service() -> (HelperInstall, Arc<AtomicU64>) {
        let now = Arc::new(AtomicU64::new(1_000_000));
        let svc = HelperInstall::new();
        let clock = now.clone();
        svc.set_clock(Box::new(move || clock.load(Ordering::SeqCst)));
        (svc, now)
    }

    fn begin(svc: &HelperInstall) -> SessionInfo {
        svc.begin(LAN, 8000, None, false, None).unwrap()
    }

    #[test]
    fn a_session_has_a_random_128_bit_token_and_a_30_minute_expiry() {
        let (svc, now) = service();
        let a = begin(&svc);
        assert_eq!(a.token.len(), 32);
        assert!(a.token.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(a.expires_ms, now.load(Ordering::SeqCst) + 30 * 60 * 1000);
        let b = begin(&svc);
        assert_ne!(a.token, b.token, "every session has its own token");
        assert!(svc.is_active());
    }

    #[test]
    fn extending_keeps_the_token_and_pushes_the_expiry_out() {
        let (svc, now) = service();
        let first = begin(&svc);
        now.fetch_add(10 * 60 * 1000, Ordering::SeqCst);
        let again = svc.extend(LAN).expect("a live session for this address");
        assert_eq!(again.token, first.token);
        assert_eq!(again.expires_ms, now.load(Ordering::SeqCst) + SESSION_MS);
        assert!(again.expires_ms > first.expires_ms);
        // Another address is another session.
        assert!(svc.extend(Ipv4Addr::new(10, 0, 0, 9)).is_none());
    }

    #[test]
    fn a_session_expires_and_an_expired_one_cannot_be_extended() {
        let (svc, now) = service();
        let info = begin(&svc);
        now.fetch_add(SESSION_MS - 1, Ordering::SeqCst);
        assert!(svc.access(Some(PEER), &info.token).is_ok());
        now.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            svc.access(Some(PEER), &info.token),
            Err(Denied::NotFound),
            "30 minutes after it began it is over"
        );
        assert!(!svc.is_active());
        assert!(svc.extend(LAN).is_none());
    }

    #[test]
    fn ending_a_session_stops_the_token_working() {
        let (svc, _) = service();
        let info = begin(&svc);
        assert!(svc.end());
        assert!(!svc.end(), "ending twice is harmless");
        assert_eq!(svc.access(Some(PEER), &info.token), Err(Denied::NotFound));
    }

    #[test]
    fn the_session_ends_when_the_helper_connects() {
        let (svc, _) = service();
        let info = begin(&svc);
        assert!(!svc.observe(false, None), "unreachable: keep waiting");
        assert!(svc.is_active());
        assert!(svc.observe(true, None), "the helper answered");
        assert_eq!(svc.access(Some(PEER), &info.token), Err(Denied::NotFound));
    }

    #[test]
    fn an_update_session_ends_when_the_helper_version_changes_or_is_current() {
        let (svc, _) = service();
        // Started while an old helper (no version) was connected: still the old one, keep waiting.
        svc.begin(LAN, 8000, None, true, None).unwrap();
        assert!(!svc.observe(true, None));
        assert!(!svc.observe(false, None));
        assert!(svc.observe(true, Some("0.5.0")), "now a current helper");
        svc.begin(LAN, 8000, None, true, Some("0.4.9".into()))
            .unwrap();
        assert!(!svc.observe(true, Some("0.4.9")));
        assert!(svc.observe(true, Some("0.4.10")), "the version changed");
    }

    #[test]
    fn a_bad_or_expired_token_is_not_found_and_a_public_source_is_forbidden() {
        let (svc, _) = service();
        let info = begin(&svc);
        assert_eq!(svc.access(Some(PEER), "nope"), Err(Denied::NotFound));
        assert_eq!(svc.access(Some(PEER), ""), Err(Denied::NotFound));
        let almost = format!("{}0", &info.token[..31]);
        assert_eq!(svc.access(Some(PEER), &almost), Err(Denied::NotFound));
        let public = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        // Source first: a stranger cannot tell a good token from a bad one.
        assert_eq!(
            svc.access(Some(public), &info.token),
            Err(Denied::Forbidden)
        );
        assert_eq!(svc.access(Some(public), "nope"), Err(Denied::Forbidden));
        assert_eq!(svc.access(None, &info.token), Err(Denied::Forbidden));
        assert!(svc.access(Some(PEER), &info.token).is_ok());
    }

    #[test]
    fn a_token_serves_at_most_30_requests() {
        let (svc, _) = service();
        let info = begin(&svc);
        for n in 1..=MAX_REQUESTS {
            assert!(svc.access(Some(PEER), &info.token).is_ok(), "request {n}");
        }
        assert_eq!(
            svc.access(Some(PEER), &info.token),
            Err(Denied::NotFound),
            "request 31"
        );
        // Refused requests from outside do not use the allowance up.
        let (svc, _) = service();
        let info = begin(&svc);
        for _ in 0..100 {
            let _ = svc.access(Some(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))), &info.token);
            let _ = svc.access(Some(PEER), "wrong");
        }
        assert!(svc.access(Some(PEER), &info.token).is_ok());
    }

    #[test]
    fn private_means_rfc1918_link_local_and_loopback() {
        let yes = [
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.0.1",
            "169.254.1.1",
            "127.0.0.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:192.168.1.5",
        ];
        let no = [
            "8.8.8.8",
            "172.32.0.1",
            "172.15.255.255",
            "192.169.0.1",
            "100.64.0.1",
            "0.0.0.0",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
        ];
        for ip in yes {
            assert!(is_private(ip.parse().unwrap()), "{ip}");
        }
        for ip in no {
            assert!(!is_private(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn the_command_and_url_carry_the_address_port_and_token() {
        let (svc, _) = service();
        let info = svc.begin(LAN, 8123, None, false, None).unwrap();
        let url = format!(
            "http://192.168.1.20:8123/lightkey-helper/{}/install.sh",
            info.token
        );
        assert_eq!(info.url(), url);
        assert_eq!(info.command(), format!("curl -fsS {url} | sh"));
    }

    fn stub_assets(installer: &str) -> Assets {
        let mut files: Vec<(String, Vec<u8>)> =
            ["lightkey_api.py", "lightkey_cues.py", "uninstall.sh"]
                .iter()
                .map(|n| (n.to_string(), format!("# {n}\n").into_bytes()))
                .collect();
        files.push(("install.sh".into(), installer.as_bytes().to_vec()));
        let mut sums = String::new();
        for (name, bytes) in &files {
            sums.push_str(&format!("{}  {name}\n", hex::encode(Sha256::digest(bytes))));
        }
        files.push(("SHA256SUMS".into(), sums.into_bytes()));
        Assets::from_files(files, "9.9.9")
    }

    #[test]
    fn the_wrapper_has_the_base_url_the_checksums_and_allow_for() {
        let assets = stub_assets("# understands --allow-for\n");
        let base = "http://192.168.1.20:8123/lightkey-helper/abc";
        let script = assets.wrapper(base, LAN);
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("set -eu"));
        assert!(script.contains(&format!("BASE='{base}'")));
        for name in embedded::FILE_NAMES {
            let want = hex::encode(Sha256::digest(assets.get(name).unwrap()));
            assert!(
                script.contains(&format!("{name}) want='{want}' ;;")),
                "checksum of {name}"
            );
        }
        assert!(script.contains("sh ./install.sh --allow-for 192.168.1.20 </dev/null"));
        assert!(script.contains("Done. Go back to StagePilot."));
        assert!(script.contains("curl -fsS"));
        assert!(script.contains("rm -rf \"$TMP\""));
        assert!(script.trim_end().ends_with("main"));
    }

    #[test]
    fn the_wrapper_leaves_allow_for_out_when_the_installer_lacks_it() {
        let assets = stub_assets("# the old installer\n");
        let script = assets.wrapper("http://192.168.1.20:1/lightkey-helper/x", LAN);
        assert!(!script.contains("--allow-for 192.168.1.20"));
        assert!(script.contains("sh ./install.sh </dev/null"));
    }

    #[test]
    fn the_wrapper_is_valid_shell() {
        // `sh -n` parses without running anything.
        for installer in ["# old\n", "# --allow-for\n"] {
            let script =
                stub_assets(installer).wrapper("http://192.168.1.20:1/lightkey-helper/x", LAN);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("wrapper.sh");
            std::fs::write(&path, script).unwrap();
            let status = std::process::Command::new("sh")
                .arg("-n")
                .arg(&path)
                .status();
            if let Ok(status) = status {
                assert!(status.success(), "sh -n rejected the wrapper");
            }
        }
    }

    #[test]
    fn only_the_five_files_can_be_served() {
        let assets = Assets::embedded();
        for name in embedded::FILE_NAMES {
            assert!(assets.get(name).is_some(), "{name}");
        }
        for name in ["VERSION", "config.json", "../install.sh", "", "INSTALL.SH"] {
            assert!(assets.get(name).is_none(), "{name:?}");
        }
        // Even a caller that hands over a sixth file cannot get it served.
        let mut files = stub_assets("").files;
        files.push(("secret.txt".into(), b"x".to_vec()));
        assert!(Assets::from_files(files, "1.0.0")
            .get("secret.txt")
            .is_none());
    }

    #[test]
    fn the_address_toward_loopback_or_an_unresolvable_name_is_not_found() {
        assert_eq!(
            local_address_toward("127.0.0.1:8770".parse().unwrap()),
            None
        );
        assert_eq!(
            local_address_toward("0.0.0.0:8770".parse().unwrap()),
            None,
            "an unspecified target has no useful local address"
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(detect_lan_ip("lighting-mac.invalid", 8770)),
            None
        );
        assert_eq!(rt.block_on(detect_lan_ip("localhost", 8770)), None);
        assert_eq!(rt.block_on(detect_lan_ip("", 8770)), None);
    }

    #[test]
    fn the_address_field_accepts_a_private_ipv4_address_only() {
        let ok = |text: &str| parse_address(text.as_bytes());
        assert!(matches!(ok(""), Ok(None)));
        assert!(matches!(ok("{}"), Ok(None)));
        assert!(matches!(ok(r#"{"address": ""}"#), Ok(None)));
        assert!(matches!(ok(r#"{"address": " 192.168.1.20 "}"#), Ok(Some(ip)) if ip == LAN));
        for bad in [
            r#"{"address": "8.8.8.8"}"#,
            r#"{"address": "127.0.0.1"}"#,
            r#"{"address": "0.0.0.0"}"#,
            r#"{"address": "my-mac.local"}"#,
            r#"{"address": 5}"#,
            "[]",
            "nope",
        ] {
            assert!(ok(bad).is_err(), "{bad}");
        }
    }
}
