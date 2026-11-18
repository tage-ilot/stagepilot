//! "Set up the Playback computer": StagePilot hands the Playback helper (`playback-library`, the
//! "Library Link") to the Playback Mac itself, with the key it will answer to, so installing it
//! needs no internet, no GitHub and no SSH. The same shape as the Lighting computer's install
//! (`helper_install`), with one difference: the served script carries a secret.
//!
//! * `POST /api/v1/playback-library/helper-install` opens (or extends) a **setup session**: a random
//!   128-bit token held in memory only, valid for 30 minutes, ended by `DELETE` of the same path or by
//!   the helper answering StagePilot's signed request. Opening the first session makes the key
//!   (`PlaybackLibrary::ensure_key`). The reply carries the one command to paste into Terminal, and that
//!   command holds only the one-time URL.
//! * `GET /playback-helper/<token>/install.sh` is a generated POSIX `sh` wrapper that carries the key;
//!   `GET /playback-helper/<token>/files/<name>` serves exactly the three bundled helper files. Unknown
//!   or expired token: `404`, empty body. More than [`MAX_REQUESTS`] requests: `404`. A source address
//!   that is not private (RFC 1918, link-local, loopback): `403`. The token is compared in constant
//!   time and is never logged. Nothing served is cached (`no-store`).
//! * These routes are never reachable through the remote tunnel: the remote ingress refuses the prefix,
//!   and the handlers refuse any request that carries a `RemoteContext`.
//! * When the main listener is on `0.0.0.0` (LAN access on) the routes are served there; otherwise a
//!   short-lived extra listener is bound to the one LAN address that routes to the Playback computer
//!   (never `0.0.0.0`) on an ephemeral port, and closed when the session ends.
//! * On the Playback computer the wrapper puts the key in a file only its owner can read (mode 600, in a
//!   private temporary folder), gives the installer that file (`--key-file`, never the key itself, which
//!   a process list would show), and removes the folder when it ends, whatever way it ends.

use crate::helper_install::{
    body, denied, detect_lan_ip, empty, expires_text, failure, is_private, main_listener_is_lan,
    new_token, nothing_here, parse_address, peer_of, Denied, ListenerGuard, MAX_REQUESTS,
    SESSION_MS,
};
use crate::playback_library::{base_url, effective_settings, note_in_use, PlaybackLibrary};
use crate::remote_types::RemoteContext;
use crate::runtime::Runtime;
use crate::validation::{http_error, ok, Json};
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{any, post};
use axum::Router;
use serde_json::json;
use sha2::{Digest, Sha256};
use stagepilot_playback::helper_assets as embedded;
use stagepilot_playback::library::HelperState;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use subtle::ConstantTimeEq;

/// Shown when no LAN address that routes to the Playback computer can be found.
pub const NO_ADDRESS: &str = "Could not find this computer's address on the Playback network. StagePilot found no route to that computer from here, so type this computer's address below.";
/// Shown when no Playback computer is chosen yet.
pub const NO_PLAYBACK_COMPUTER: &str = "Choose the Playback computer first.";
/// The URL prefix. Keep it off the remote allow-list (`remote_ingress`) and out of the logs (`security`).
pub const PREFIX: &str = "/playback-helper";
/// How often a running session looks at whether the helper has answered.
const WATCH_EVERY: Duration = Duration::from_secs(3);

type Clock = Box<dyn Fn() -> u64 + Send + Sync>;

// ---- the files and the wrapper -------------------------------------------------------------------

/// The three served files, the version they are, and what the wrapper must know about them.
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
            version: embedded::BUNDLED_HELPER_VERSION.to_string(),
        }
    }

    /// Files given by the caller (tests); the names must be the three served names.
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
        // Only the three names are ever served, whatever `files` holds.
        embedded::FILE_NAMES
            .contains(&name)
            .then(|| self.files.iter().find(|(n, _)| n == name))
            .flatten()
            .map(|(_, bytes)| bytes.as_slice())
    }

    fn sha256(&self, name: &str) -> String {
        hex::encode(Sha256::digest(self.get(name).unwrap_or_default()))
    }

    /// The generated `sh` wrapper for one session. `key` is the only secret in it; it is written to a
    /// private file, never put on a command line.
    pub fn wrapper(&self, base_url: &str, key: &str) -> String {
        let mut wants = String::new();
        for name in embedded::FILE_NAMES {
            wants.push_str(&format!("    {name}) want='{}' ;;\n", self.sha256(name)));
        }
        let names = embedded::FILE_NAMES.join(" ");
        format!(
            r#"#!/bin/sh
# StagePilot: set up the Playback computer (Library Link {version}).
# Downloads the Library Link from StagePilot at the address below (this local network only, no
# internet), checks every file against the checksums written in this script, then runs the script's
# own installer with the key StagePilot made for it. Run it in Terminal on the Playback computer.
set -eu

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
  umask 077
  TMP=$(mktemp -d "${{TMPDIR:-/tmp}}/stagepilot-playback.XXXXXX") || fail "could not make a temporary folder."
  trap 'cd /; rm -rf "$TMP"' EXIT
  trap 'exit 1' HUP INT TERM
  printf 'Downloading the Library Link {version} from StagePilot ...\n'
  for name in {names}; do
    case "$name" in
{wants}    esac
    curl -fsS "$BASE/files/$name" -o "$TMP/$name" ||
      fail "could not download $name from StagePilot. Is StagePilot still waiting? The command works for 30 minutes."
    have=$(sha256_of "$TMP/$name")
    [ "$have" = "$want" ] || fail "$name does not match its checksum; nothing was installed."
  done
  for name in install.sh playback_library.py; do
    listed=$(awk -v n="$name" '$2 == n {{print $1}}' "$TMP/SHA256SUMS")
    have=$(sha256_of "$TMP/$name")
    [ -n "$listed" ] && [ "$listed" = "$have" ] || fail "$name is not listed with the same checksum in SHA256SUMS; nothing was installed."
  done
  # The key goes into a file only this user can read, inside the private folder; the installer reads it
  # from there and deletes it. It is not on any command line.
  printf '%s\n' '{key}' > "$TMP/key.txt"
  chmod 600 "$TMP/key.txt"
  cd "$TMP"
  printf 'Installing ...\n'
  sh ./install.sh --key-file "$TMP/key.txt" </dev/null || fail "the installer reported an error (see above)."
  printf 'Done. Go back to StagePilot.\n'
}}

main
"#,
            version = self.version,
        )
    }
}

// ---- the session -------------------------------------------------------------------------------

struct Session {
    id: u64,
    token: String,
    expires_ms: u64,
    requests: u32,
    lan_ip: Ipv4Addr,
    port: u16,
    _listener: Option<ListenerGuard>,
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

    /// The command to paste: it holds the one-time address and nothing secret.
    pub fn command(&self) -> String {
        format!("curl -fsS {} | sh", self.url())
    }
}

pub struct PlaybackHelperInstall {
    state: Mutex<State_>,
    clock: Mutex<Clock>,
    assets: Assets,
    library: Arc<PlaybackLibrary>,
    /// Serialises "look for a session, maybe open a listener, start one" across requests.
    pub(crate) create_lock: tokio::sync::Mutex<()>,
}

impl PlaybackHelperInstall {
    pub fn new(library: Arc<PlaybackLibrary>) -> Self {
        Self::with_assets(library, Assets::embedded())
    }

    pub fn with_assets(library: Arc<PlaybackLibrary>, assets: Assets) -> Self {
        Self {
            state: Mutex::new(State_::default()),
            clock: Mutex::new(Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64)
            })),
            assets,
            library,
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

    /// A live session for this address, with its expiry pushed out to a full 30 minutes. It keeps its
    /// token and its request count.
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

    /// The helper answered StagePilot's signed request with a current version: the session has done its
    /// work. Any other state keeps it waiting (an older helper, one that refuses, silence).
    pub fn observe(&self, helper: HelperState) -> bool {
        let mut state = self.lock();
        if self.live(&mut state).is_none() || helper != HelperState::Ready {
            return false;
        }
        state.session = None;
        true
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

// ---- serving -----------------------------------------------------------------------------------

/// The check every served request goes through.
fn admit(
    svc: &PlaybackHelperInstall,
    request: &Request,
    token: &str,
) -> Result<SessionInfo, Denied> {
    // Never through the remote tunnel: only the trusted ingress sets this.
    if request.extensions().get::<RemoteContext>().is_some() {
        return Err(Denied::NotFound);
    }
    svc.access(peer_of(request), token)
}

/// What a path under the prefix asks for.
enum Wanted<'a> {
    Wrapper(&'a str),
    File(&'a str, &'a str),
}

/// `token/install.sh` or `token/files/<name>`; anything else (a listing, a deeper path, `..`) is nothing.
fn wanted(rest: &str) -> Option<Wanted<'_>> {
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [token, "install.sh"] if !token.is_empty() => Some(Wanted::Wrapper(token)),
        [token, "files", name] if !token.is_empty() => Some(Wanted::File(token, name)),
        _ => None,
    }
}

async fn dispatch(
    State(svc): State<Arc<PlaybackHelperInstall>>,
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
            Ok(info) => match svc.library.key_for_installer() {
                Some(key) => body(
                    "text/x-shellscript; charset=utf-8",
                    svc.assets.wrapper(&info.base_url(), &key).into_bytes(),
                ),
                // The key cannot be read (the credential store failed): there is nothing to install.
                None => empty(StatusCode::NOT_FOUND),
            },
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

/// The served routes alone: the main app merges them, and the extra listener serves only these. One
/// catch-all under the prefix, so no path below it ever falls through to the dashboard files.
pub fn served_routes<S: Clone + Send + Sync + 'static>(
    svc: Arc<PlaybackHelperInstall>,
) -> Router<S> {
    Router::new()
        .route("/playback-helper", any(nothing_here))
        .route("/playback-helper/", any(nothing_here))
        .route("/playback-helper/{*rest}", any(dispatch))
        .with_state(svc)
}

/// The app the extra listener serves.
fn listener_app(svc: Arc<PlaybackHelperInstall>) -> Router {
    served_routes::<()>(svc)
        .layer(axum::middleware::from_fn(crate::security::security_headers))
        .fallback(nothing_here)
}

/// Serve the install routes on `ip:0` until the guard is dropped.
async fn open_listener(
    svc: Arc<PlaybackHelperInstall>,
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

fn reply(info: &SessionInfo) -> Json {
    ok(json!({
        "command": info.command(),
        "url": info.url(),
        "expires_at": expires_text(info.expires_ms),
        "lan_ip": info.lan_ip.to_string(),
        "port": info.port,
        "bundled_version": embedded::BUNDLED_HELPER_VERSION,
    }))
}

/// Host and port of the helper's address (the saved Playback host unless Advanced names another).
fn helper_target(rt: &Runtime) -> Option<(String, u16)> {
    let base = base_url(&effective_settings(rt).address)?;
    let parsed = url::Url::parse(&base).ok()?;
    Some((
        parsed.host_str()?.trim_matches(['[', ']']).to_string(),
        parsed
            .port()
            .unwrap_or(stagepilot_core::config::DEFAULT_PLAYBACK_LIBRARY_PORT),
    ))
}

/// `POST /api/v1/playback-library/helper-install` `{address?}`. `address` is THIS computer's address as the
/// Playback computer reaches it (private IPv4), for when StagePilot cannot find it itself.
pub async fn create(State(rt): State<Arc<Runtime>>, raw: Bytes) -> Json {
    let override_ip = match parse_address(&raw) {
        Ok(ip) => ip,
        Err(response) => return response,
    };
    let svc = rt.playback_helper_install.clone();
    let _one_at_a_time = svc.create_lock.lock().await;
    let target = helper_target(&rt);
    let lan_ip = match (override_ip, &target) {
        (Some(ip), _) => Some(ip),
        (None, Some((host, port))) => detect_lan_ip(host, *port).await,
        (None, None) => {
            return failure(409, "no_playback_computer", NO_PLAYBACK_COMPUTER);
        }
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
    // The key is made once, the first time a session is opened, and kept: the next session reuses it.
    match rt.playback_library.ensure_key() {
        Ok(true) => rt.playback_library.reset().await,
        Ok(false) => {}
        Err(message) => return http_error(503, &message),
    }
    let info = match svc.begin(lan_ip, port, guard) {
        Ok(info) => info,
        Err(message) => return http_error(500, &message),
    };
    watch(Arc::downgrade(&rt), svc.clone(), info.id);
    reply(&info)
}

/// `DELETE /api/v1/playback-library/helper-install`.
pub async fn delete(State(rt): State<Arc<Runtime>>) -> Json {
    ok(json!({ "ended": rt.playback_helper_install.end() }))
}

/// Ends the session when it expires or the helper answers with a valid signature. One task per session.
fn watch(rt: Weak<Runtime>, svc: Arc<PlaybackHelperInstall>, id: u64) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(WATCH_EVERY).await;
            if svc.current_id() != Some(id) {
                return;
            }
            let Some(rt) = rt.upgrade() else {
                return;
            };
            let settings = effective_settings(&rt);
            let status = rt.playback_library.status(&settings, true).await;
            note_in_use(&rt, status.active).await;
            svc.observe(status.helper.state);
        }
    });
}

/// The dashboard routes (`/api/v1/playback-library/helper-install`).
pub fn api_routes() -> Router<Arc<Runtime>> {
    Router::new().route(
        "/api/v1/playback-library/helper-install",
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
    fn service() -> (PlaybackHelperInstall, Arc<AtomicU64>) {
        let now = Arc::new(AtomicU64::new(1_000_000));
        let svc = PlaybackHelperInstall::new(Arc::new(PlaybackLibrary::new()));
        let clock = now.clone();
        svc.set_clock(Box::new(move || clock.load(Ordering::SeqCst)));
        (svc, now)
    }

    fn begin(svc: &PlaybackHelperInstall) -> SessionInfo {
        svc.begin(LAN, 8000, None).unwrap()
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
        assert!(svc.extend(Ipv4Addr::new(10, 0, 0, 9)).is_none());
    }

    #[test]
    fn a_session_expires_ends_and_stops_its_token() {
        let (svc, now) = service();
        let info = begin(&svc);
        now.fetch_add(SESSION_MS - 1, Ordering::SeqCst);
        assert!(svc.access(Some(PEER), &info.token).is_ok());
        now.fetch_add(1, Ordering::SeqCst);
        assert_eq!(svc.access(Some(PEER), &info.token), Err(Denied::NotFound));
        assert!(!svc.is_active());
        let info = begin(&svc);
        assert!(svc.end());
        assert!(!svc.end(), "ending twice is harmless");
        assert_eq!(svc.access(Some(PEER), &info.token), Err(Denied::NotFound));
    }

    #[test]
    fn the_session_ends_only_when_the_helper_answers_with_the_key_and_is_current() {
        let (svc, _) = service();
        let info = begin(&svc);
        for waiting in [
            HelperState::NotInstalled,
            HelperState::NeedsKey,
            HelperState::Outdated,
            HelperState::Unreachable,
        ] {
            assert!(!svc.observe(waiting), "{waiting:?} keeps the session open");
            assert!(svc.is_active());
        }
        assert!(svc.observe(HelperState::Ready));
        assert_eq!(svc.access(Some(PEER), &info.token), Err(Denied::NotFound));
        assert!(!svc.observe(HelperState::Ready), "nothing left to end");
    }

    #[test]
    fn a_bad_token_is_not_found_and_a_public_source_is_forbidden() {
        let (svc, _) = service();
        let info = begin(&svc);
        assert_eq!(svc.access(Some(PEER), "nope"), Err(Denied::NotFound));
        let public = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(
            svc.access(Some(public), &info.token),
            Err(Denied::Forbidden)
        );
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
        assert_eq!(svc.access(Some(PEER), &info.token), Err(Denied::NotFound));
    }

    #[test]
    fn the_command_and_url_carry_the_address_port_and_token_and_nothing_else() {
        let (svc, _) = service();
        let info = svc.begin(LAN, 8123, None).unwrap();
        let url = format!(
            "http://192.168.1.20:8123/playback-helper/{}/install.sh",
            info.token
        );
        assert_eq!(info.url(), url);
        assert_eq!(info.command(), format!("curl -fsS {url} | sh"));
    }

    const KEY: &str = "k3y_for-the-wrapper-TEST-0123456789ABCDEFG";

    fn stub_assets(installer: &str) -> Assets {
        let mut files: Vec<(String, Vec<u8>)> = vec![(
            "playback_library.py".into(),
            b"# playback_library.py\n".to_vec(),
        )];
        files.push(("install.sh".into(), installer.as_bytes().to_vec()));
        let mut sums = String::new();
        for (name, bytes) in &files {
            sums.push_str(&format!("{}  {name}\n", hex::encode(Sha256::digest(bytes))));
        }
        files.push(("SHA256SUMS".into(), sums.into_bytes()));
        Assets::from_files(files, "9.9.9")
    }

    #[test]
    fn the_wrapper_has_the_base_url_the_checksums_and_the_key_in_a_file() {
        let assets = stub_assets("# installer\n");
        let base = "http://192.168.1.20:8123/playback-helper/abc";
        let script = assets.wrapper(base, KEY);
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
        assert!(script.contains("umask 077"));
        assert!(script.contains("chmod 600"));
        assert!(script.contains("sh ./install.sh --key-file \"$TMP/key.txt\" </dev/null"));
        assert!(script.contains("rm -rf \"$TMP\""));
        assert!(script.contains("Done. Go back to StagePilot."));
        assert!(script.trim_end().ends_with("main"));
        // The key appears once, as the text of a builtin `printf` (never an argument of another program).
        assert_eq!(script.matches(KEY).count(), 1);
        assert!(script.contains(&format!("printf '%s\\n' '{KEY}' > \"$TMP/key.txt\"")));
        assert!(
            !script.contains("--key "),
            "the installer is given a file, not the key"
        );
    }

    #[test]
    fn the_wrapper_is_valid_shell() {
        // `sh -n` parses without running anything.
        let script =
            stub_assets("# installer\n").wrapper("http://192.168.1.20:1/playback-helper/x", KEY);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrapper.sh");
        std::fs::write(&path, script).unwrap();
        if let Ok(status) = std::process::Command::new("sh")
            .arg("-n")
            .arg(&path)
            .status()
        {
            assert!(status.success(), "sh -n rejected the wrapper");
        }
    }

    #[test]
    fn only_the_three_files_can_be_served() {
        let assets = Assets::embedded();
        for name in embedded::FILE_NAMES {
            assert!(assets.get(name).is_some(), "{name}");
        }
        for name in [
            "uninstall.sh",
            "config.json",
            "../install.sh",
            "",
            "INSTALL.SH",
        ] {
            assert!(assets.get(name).is_none(), "{name:?}");
        }
        let mut files = stub_assets("").files;
        files.push(("secret.txt".into(), b"x".to_vec()));
        assert!(Assets::from_files(files, "1.0.0")
            .get("secret.txt")
            .is_none());
    }
}
