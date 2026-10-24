//! Planning Center OAuth 2.0 sign-in: PKCE, token exchange, storage, refresh. Port of
//! `planning_center_oauth.py`.
//!
//! The `client_secret` never reaches this machine: every call that needs it is relayed through the
//! StagePilot control-plane Worker (`/v1/planning-center/oauth/flow|token|refresh`).
//!
//! The transient-vs-permanent distinction is deliberate and kept: a network blip, timeout, 429 or
//! 5xx is TRANSIENT and must never flip the connection into "sign in again"; only Planning Center
//! rejecting the grant (`invalid_grant`, i.e. the refresh token was revoked or expired) is
//! PERMANENT and sets the explicit `needs_reconnect` state.
//!
//! Secrets: access/refresh tokens, authorization codes and the PKCE verifier are held in types
//! whose `Debug` is redacted, are never written to a log line, and are stored only through the
//! keyring-backed [`CredentialStore`] (the same `planning-center-oauth` account Python uses, same
//! JSON blob).

use crate::errors::PcoError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::settings::CredentialStore;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

pub const AUTHORIZE_ENDPOINT: &str = "https://api.planningcenteronline.com/oauth/authorize";
/// Only the Services scope is ever requested, never a broader product scope.
pub const OAUTH_SCOPE: &str = "services";
pub const DEFAULT_CONTROL_PLANE_ORIGIN: &str =
    "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev";
/// Access tokens live for 2 hours; refresh 15 minutes early.
pub const REFRESH_MARGIN_SECONDS: f64 = 15.0 * 60.0;
pub const REFRESH_POLL_SECONDS: f64 = 60.0;
/// A sign-in that never produced a callback is abandoned after the desktop listener's own
/// ~5 minute timeout, plus slack.
pub const FLOW_TTL_SECONDS: f64 = 420.0;
pub const OAUTH_STARTUP_REFRESH_TIMEOUT_SECONDS: f64 = 5.0;

const INVALID_RESPONSE: &str = "The Planning Center sign-in service returned an invalid response.";

/// A sign-in or refresh attempt failed. `permanent` distinguishes "this credential is dead, the
/// user must sign in again" from "this may well work on the next attempt".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct OAuthError {
    pub message: String,
    pub permanent: bool,
}

impl OAuthError {
    pub fn transient(message: &str) -> Self {
        Self {
            message: message.to_string(),
            permanent: false,
        }
    }
    pub fn permanent(message: &str) -> Self {
        Self {
            message: message.to_string(),
            permanent: true,
        }
    }
}

/// Seconds since the Unix epoch (Python `time.time()`).
pub fn system_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// The stored OAuth token blob (keyring-backed, never `settings.json`).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: f64,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default)]
    pub needs_reconnect: bool,
}

fn default_scope() -> String {
    OAUTH_SCOPE.to_string()
}

impl std::fmt::Debug for OAuthTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokens")
            .field("access_token", &"**********")
            .field("refresh_token", &"**********")
            .field("expires_at", &self.expires_at)
            .field("scope", &self.scope)
            .field("needs_reconnect", &self.needs_reconnect)
            .finish()
    }
}

impl OAuthTokens {
    pub fn expires_in(&self, now: f64) -> f64 {
        self.expires_at - now
    }
    pub fn needs_refresh(&self, now: f64) -> bool {
        self.expires_in(now) <= REFRESH_MARGIN_SECONDS
    }
}

/// One in-progress sign-in attempt, held in memory only.
#[derive(Clone)]
pub struct PendingFlow {
    pub state: String,
    pub code_verifier: String,
    pub flow_id: String,
    pub ticket: String,
    pub started_at: f64,
}

impl std::fmt::Debug for PendingFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingFlow")
            .field("flow_id", &self.flow_id)
            .field("started_at", &self.started_at)
            .finish_non_exhaustive()
    }
}

impl PendingFlow {
    pub fn expired(&self, now: f64) -> bool {
        now - self.started_at > FLOW_TTL_SECONDS
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthConnectionStatus {
    pub connection_method: ConnectionMethod,
    pub connected: bool,
    pub needs_reconnect: bool,
    pub expires_at: Option<f64>,
    pub scope: Option<String>,
}

// ---- PKCE and state ----------------------------------------------------------------------------

fn random_bytes(n: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; n];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

/// RFC 7636 code verifier: 43-128 chars of unreserved characters (64 random bytes -> 86 chars).
pub fn generate_code_verifier() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes(64))
}

/// S256 challenge. Plain challenges are never used.
pub fn code_challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Bind the control-plane flow id into the OAuth `state` value; the random suffix keeps `state`
/// unguessable for CSRF purposes.
pub fn build_state(flow_id: &str) -> String {
    format!("{flow_id}.{}", URL_SAFE_NO_PAD.encode(random_bytes(24)))
}

pub fn flow_id_from_state(state: &str) -> Option<String> {
    let (head, tail) = state.split_once('.')?;
    if head.is_empty() || tail.is_empty() {
        None
    } else {
        Some(head.to_string())
    }
}

/// Python `urllib.parse.quote_plus` (unreserved: letters, digits and `_.-~`; space is `+`).
pub fn quote_plus(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The authorize URL WITHOUT `redirect_uri`: the desktop side appends it because only it knows
/// which of the pre-registered loopback ports was actually free.
pub fn build_authorize_url(client_id: &str, state: &str, code_challenge: &str) -> String {
    let query = [
        ("client_id", client_id),
        ("response_type", "code"),
        ("scope", OAUTH_SCOPE),
        ("state", state),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
    ]
    .iter()
    .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{AUTHORIZE_ENDPOINT}?{query}")
}

// ---- token storage ------------------------------------------------------------------------------

/// Persist the OAuth token blob through the existing credential-store abstraction (a second
/// keyring account, `planning-center-oauth`): the blob is JSON so access token, refresh token,
/// expiry and reconnect marker travel together atomically.
#[derive(Clone)]
pub struct OAuthTokenStore {
    credentials: Arc<dyn CredentialStore>,
}

impl OAuthTokenStore {
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self { credentials }
    }

    /// `Err` is a credential-store failure; a corrupt blob reads as "disconnected" (`Ok(None)`).
    pub fn load(
        &self,
    ) -> Result<Option<OAuthTokens>, stagepilot_core::settings::CredentialStoreError> {
        let Some(raw) = self.credentials.get_secret()? else {
            return Ok(None);
        };
        if raw.is_empty() {
            return Ok(None);
        }
        match serde_json::from_str::<OAuthTokens>(&raw) {
            Ok(tokens) if !tokens.access_token.is_empty() && !tokens.refresh_token.is_empty() => {
                Ok(Some(tokens))
            }
            _ => {
                tracing::warn!("planning_center_oauth_tokens_unreadable");
                Ok(None)
            }
        }
    }

    pub fn save(
        &self,
        tokens: &OAuthTokens,
    ) -> Result<(), stagepilot_core::settings::CredentialStoreError> {
        let blob = serde_json::to_string(tokens).unwrap_or_default();
        self.credentials.set_secret(&blob)
    }

    pub fn clear(&self) -> Result<(), stagepilot_core::settings::CredentialStoreError> {
        self.credentials.remove_secret()
    }

    pub fn mark_needs_reconnect(
        &self,
    ) -> Result<(), stagepilot_core::settings::CredentialStoreError> {
        match self.load()? {
            Some(current) if !current.needs_reconnect => self.save(&OAuthTokens {
                needs_reconnect: true,
                ..current
            }),
            _ => Ok(()),
        }
    }
}

/// Run a blocking keyring call off the async executor (Python wraps these in `to_thread`).
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, OAuthError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| OAuthError::transient("The secure credential store is unavailable."))
}

// ---- control-plane client -------------------------------------------------------------------------

/// Calls the Worker routes that hold the Planning Center client secret.
#[derive(Clone)]
pub struct ControlPlaneClient {
    http: reqwest::Client,
    origin: Url,
}

impl ControlPlaneClient {
    /// The production origin is the only one accepted from configuration.
    pub fn new() -> Result<Self, OAuthError> {
        Self::with_origin(DEFAULT_CONTROL_PLANE_ORIGIN)
    }

    /// Mirrors Python's `origin not in TRUSTED_CONTROL_PLANE_ORIGINS` refusal.
    pub fn with_origin(origin: &str) -> Result<Self, OAuthError> {
        if origin != DEFAULT_CONTROL_PLANE_ORIGIN {
            return Err(OAuthError::transient(
                "The Planning Center sign-in service is not trusted.",
            ));
        }
        Self::build(origin)
    }

    /// An arbitrary origin, for tests and the mocked contract profile ONLY. It is not reachable
    /// from settings, the environment or any request: only code that calls it explicitly (the
    /// `--contract-pco` server flag and test harnesses) can use it.
    pub fn for_test_origin(origin: &str) -> Result<Self, OAuthError> {
        Self::build(origin)
    }

    fn build(origin: &str) -> Result<Self, OAuthError> {
        crate::client::install_crypto_provider();
        let origin = Url::parse(origin).map_err(|_| {
            OAuthError::transient("The Planning Center sign-in service is not trusted.")
        })?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(20))
            // `trust_env=False, follow_redirects=False`.
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                OAuthError::transient("The Planning Center sign-in service is not trusted.")
            })?;
        Ok(Self { http, origin })
    }

    async fn post(&self, path: &str, payload: Value) -> Result<Map<String, Value>, OAuthError> {
        let url = self
            .origin
            .join(path.trim_start_matches('/'))
            .map_err(|_| OAuthError::transient(INVALID_RESPONSE))?;
        // Transient by construction: we never reached the service.
        let response = self
            .http
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|_| {
                OAuthError::transient(
                    "Could not reach the Planning Center sign-in service. Check your Internet \
                     connection and try again.",
                )
            })?;
        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| OAuthError::transient(INVALID_RESPONSE))?;
        if status == 200 || status == 201 {
            return match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(map)) => Ok(map),
                _ => Err(OAuthError::transient(INVALID_RESPONSE)),
            };
        }
        Err(failure(status, &bytes))
    }

    pub async fn start_flow(&self) -> Result<(String, String), OAuthError> {
        let body = self
            .post("/v1/planning-center/oauth/flow", json!({}))
            .await?;
        match (
            body.get("flow_id").and_then(Value::as_str),
            body.get("ticket").and_then(Value::as_str),
        ) {
            (Some(flow_id), Some(ticket)) if !flow_id.is_empty() && !ticket.is_empty() => {
                Ok((flow_id.to_string(), ticket.to_string()))
            }
            _ => Err(OAuthError::transient(INVALID_RESPONSE)),
        }
    }

    pub async fn exchange_code(
        &self,
        flow: &PendingFlow,
        code: &str,
        redirect_uri: &str,
        now: f64,
    ) -> Result<OAuthTokens, OAuthError> {
        let body = self
            .post(
                "/v1/planning-center/oauth/token",
                json!({
                    "flow_id": flow.flow_id,
                    "ticket": flow.ticket,
                    "code": code,
                    "code_verifier": flow.code_verifier,
                    "redirect_uri": redirect_uri,
                }),
            )
            .await?;
        tokens_from_response(&body, now)
    }

    pub async fn refresh(
        &self,
        flow_id: &str,
        ticket: &str,
        refresh_token: &str,
        now: f64,
    ) -> Result<OAuthTokens, OAuthError> {
        let body = self
            .post(
                "/v1/planning-center/oauth/refresh",
                json!({"flow_id": flow_id, "ticket": ticket, "refresh_token": refresh_token}),
            )
            .await?;
        tokens_from_response(&body, now)
    }
}

fn failure(status: u16, body: &[u8]) -> OAuthError {
    let error = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(String::from))
        .unwrap_or_default();
    // Only Planning Center rejecting the grant itself is permanent. A 429/5xx (rate limit, Worker
    // hiccup, PCO outage) is transient and must not strand a still-valid connection.
    if matches!(
        error.as_str(),
        "invalid_grant" | "unauthorized_client" | "invalid_client"
    ) {
        return OAuthError::permanent(
            "Your Planning Center connection has expired. Sign in again.",
        );
    }
    if status == 429 {
        return OAuthError::transient(
            "Planning Center sign-in is busy right now. Try again in a moment.",
        );
    }
    OAuthError::transient("Planning Center sign-in could not be completed. Try again.")
}

fn tokens_from_response(body: &Map<String, Value>, now: f64) -> Result<OAuthTokens, OAuthError> {
    let text = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let expires_in = body
        .get("expires_in")
        .filter(|v| !v.is_boolean())
        .and_then(Value::as_f64);
    match (text("access_token"), text("refresh_token"), expires_in) {
        (Some(access_token), Some(refresh_token), Some(expires_in)) => Ok(OAuthTokens {
            access_token,
            refresh_token,
            expires_at: now + expires_in,
            scope: text("scope").unwrap_or_else(default_scope),
            needs_reconnect: false,
        }),
        _ => Err(OAuthError::transient(INVALID_RESPONSE)),
    }
}

// ---- the service -------------------------------------------------------------------------------------

/// Own the sign-in handshake, token storage and proactive refresh.
pub struct OAuthService {
    client_id: String,
    tokens: OAuthTokenStore,
    control_plane: ControlPlaneClient,
    clock: Clock,
    pending: Mutex<Option<PendingFlow>>,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl OAuthService {
    pub fn new(
        client_id: &str,
        tokens: OAuthTokenStore,
        control_plane: ControlPlaneClient,
    ) -> Self {
        Self::with_clock(client_id, tokens, control_plane, Arc::new(system_now))
    }

    pub fn with_clock(
        client_id: &str,
        tokens: OAuthTokenStore,
        control_plane: ControlPlaneClient,
        clock: Clock,
    ) -> Self {
        Self {
            client_id: client_id.to_string(),
            tokens,
            control_plane,
            clock,
            pending: Mutex::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, Option<PendingFlow>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn now(&self) -> f64 {
        (self.clock)()
    }

    /// Whether this build has a Planning Center OAuth client id at all.
    pub fn configured(&self) -> bool {
        !self.client_id.is_empty()
    }

    /// Begin a sign-in: returns `(authorize_url_without_redirect, state)`.
    pub async fn start(&self) -> Result<(String, String), OAuthError> {
        if self.client_id.is_empty() {
            return Err(OAuthError::transient(
                "This build of StagePilot cannot sign in to Planning Center.",
            ));
        }
        let (flow_id, ticket) = self.control_plane.start_flow().await?;
        let verifier = generate_code_verifier();
        let state = build_state(&flow_id);
        *self.pending() = Some(PendingFlow {
            state: state.clone(),
            code_verifier: verifier.clone(),
            flow_id,
            ticket,
            started_at: self.now(),
        });
        Ok((
            build_authorize_url(&self.client_id, &state, &code_challenge_for(&verifier)),
            state,
        ))
    }

    /// Finish a sign-in with the code from the desktop loopback listener.
    pub async fn complete(
        &self,
        state: &str,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokens, CompleteError> {
        let pending = self.pending().clone();
        let Some(pending) = pending.filter(|p| !p.expired(self.now())) else {
            *self.pending() = None;
            return Err(OAuthError::permanent(
                "This Planning Center sign-in expired. Start it again.",
            )
            .into());
        };
        // Constant-time compare: `state` is the CSRF token for this flow.
        let matches: bool = pending.state.as_bytes().ct_eq(state.as_bytes()).into();
        if !matches {
            return Err(OAuthError::permanent(
                "Planning Center sign-in could not be verified. Try signing in again.",
            )
            .into());
        }
        let tokens = self
            .control_plane
            .exchange_code(&pending, code, redirect_uri, self.now())
            .await?;
        *self.pending() = None;
        let store = self.tokens.clone();
        let to_save = tokens.clone();
        blocking(move || store.save(&to_save))
            .await?
            .map_err(CompleteError::Credential)?;
        Ok(tokens)
    }

    pub fn cancel(&self) {
        *self.pending() = None;
    }

    pub fn stored(
        &self,
    ) -> Result<Option<OAuthTokens>, stagepilot_core::settings::CredentialStoreError> {
        self.tokens.load()
    }

    pub fn disconnect(&self) -> Result<(), stagepilot_core::settings::CredentialStoreError> {
        *self.pending() = None;
        self.tokens.clear()
    }

    /// Exchange the stored refresh token for a fresh token pair. Planning Center issues a NEW
    /// refresh token on every refresh and invalidates the old one, so the stored blob is
    /// overwritten in full every time.
    pub async fn refresh_now(
        &self,
        tokens: Option<OAuthTokens>,
    ) -> Result<OAuthTokens, CompleteError> {
        let _guard = self.refresh_lock.lock().await;
        self.refresh_locked(tokens).await
    }

    /// `refresh_now` for a caller that already holds `refresh_lock`.
    async fn refresh_locked(
        &self,
        tokens: Option<OAuthTokens>,
    ) -> Result<OAuthTokens, CompleteError> {
        let current = match tokens {
            Some(t) => t,
            None => {
                let store = self.tokens.clone();
                blocking(move || store.load())
                    .await?
                    .map_err(CompleteError::Credential)?
                    .ok_or_else(|| {
                        OAuthError::permanent("Planning Center is disconnected. Sign in again.")
                    })?
            }
        };
        let (flow_id, ticket) = self.control_plane.start_flow().await?;
        let refreshed = match self
            .control_plane
            .refresh(&flow_id, &ticket, &current.refresh_token, self.now())
            .await
        {
            Ok(t) => t,
            Err(error) => {
                if error.permanent {
                    // Explicit, actionable state: the UI shows "sign in again".
                    let store = self.tokens.clone();
                    blocking(move || store.mark_needs_reconnect())
                        .await?
                        .map_err(CompleteError::Credential)?;
                }
                return Err(error.into());
            }
        };
        let store = self.tokens.clone();
        let to_save = refreshed.clone();
        blocking(move || store.save(&to_save))
            .await?
            .map_err(CompleteError::Credential)?;
        Ok(refreshed)
    }

    /// A usable access token, refreshing first when it is due.
    pub async fn valid_access_token(&self) -> Result<String, CompleteError> {
        let store = self.tokens.clone();
        let current = blocking(move || store.load())
            .await?
            .map_err(CompleteError::Credential)?
            .ok_or_else(|| {
                OAuthError::permanent("Planning Center is disconnected. Sign in again.")
            })?;
        if current.needs_reconnect {
            return Err(OAuthError::permanent(
                "Your Planning Center connection has expired. Sign in again.",
            )
            .into());
        }
        if current.needs_refresh(self.now()) {
            return Ok(self.refresh_now(Some(current)).await?.access_token);
        }
        Ok(current.access_token)
    }

    pub fn status(&self, connection_method: ConnectionMethod) -> OAuthConnectionStatus {
        let tokens = self.tokens.load().ok().flatten();
        OAuthConnectionStatus {
            connection_method,
            connected: tokens.as_ref().is_some_and(|t| !t.needs_reconnect),
            needs_reconnect: tokens.as_ref().is_some_and(|t| t.needs_reconnect),
            expires_at: tokens.as_ref().map(|t| t.expires_at),
            scope: tokens.map(|t| t.scope),
        }
    }
}

#[async_trait::async_trait]
impl crate::client::TokenSource for OAuthService {
    async fn current_token(&self) -> Result<String, PcoError> {
        self.valid_access_token()
            .await
            .map_err(|error| match error {
                CompleteError::OAuth(error) if error.permanent => {
                    PcoError::Authentication(error.message)
                }
                CompleteError::OAuth(_) => {
                    PcoError::Transport("Could not refresh the Planning Center sign-in.".into())
                }
                CompleteError::Credential(_) => {
                    PcoError::configuration("Planning Center credentials are not available.")
                }
            })
    }

    async fn token_after_rejection(&self, rejected: &str) -> Option<String> {
        // Serialise with every other refresh: Planning Center invalidates the old refresh token
        // on use, so two racing refreshes would turn a healthy connection into `invalid_grant`.
        let _guard = self.refresh_lock.lock().await;
        let store = self.tokens.clone();
        let stored = blocking(move || store.load()).await.ok()?.ok()??;
        if stored.needs_reconnect {
            return None;
        }
        if stored.access_token != rejected {
            // Refreshed meanwhile (for example by the proactive loop).
            return Some(stored.access_token);
        }
        match self.refresh_locked(Some(stored)).await {
            Ok(fresh) => Some(fresh.access_token),
            Err(_) => None,
        }
    }

    async fn mark_needs_reconnect(&self) {
        let store = self.tokens.clone();
        if blocking(move || store.mark_needs_reconnect()).await.is_ok() {
            tracing::warn!("planning_center_oauth_needs_reconnect");
        }
    }
}

/// An OAuth failure or a credential-store failure (Python raises `PlanningCenterOAuthError` or
/// `CredentialStoreError`; the routes map them to different statuses).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CompleteError {
    #[error("{0}")]
    OAuth(#[from] OAuthError),
    #[error("{0}")]
    Credential(stagepilot_core::settings::CredentialStoreError),
}

/// Refresh the access token ~15 minutes before it expires: one lifespan-owned task polling a stop
/// signal. A transient failure leaves the tokens alone and retries on the next tick; a permanent
/// one has already set `needs_reconnect`, so the loop stops trying until the user signs in again.
pub async fn proactive_refresh_loop(
    service: Arc<OAuthService>,
    mut stop: tokio::sync::watch::Receiver<bool>,
    poll: Duration,
) {
    while !*stop.borrow() {
        let store = service.tokens.clone();
        match blocking(move || store.load()).await {
            Ok(Ok(Some(tokens)))
                if !tokens.needs_reconnect && tokens.needs_refresh(service.now()) =>
            {
                match service.refresh_now(Some(tokens)).await {
                    Ok(_) => tracing::info!("planning_center_oauth_refreshed"),
                    Err(CompleteError::OAuth(error)) => tracing::warn!(
                        permanent = error.permanent,
                        "planning_center_oauth_refresh_failed"
                    ),
                    Err(CompleteError::Credential(_)) => {
                        tracing::warn!("planning_center_oauth_refresh_unavailable")
                    }
                }
            }
            Ok(Err(_)) | Err(_) => tracing::warn!("planning_center_oauth_refresh_unavailable"),
            _ => {}
        }
        tokio::select! {
            _ = tokio::time::sleep(poll) => {}
            changed = stop.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}
