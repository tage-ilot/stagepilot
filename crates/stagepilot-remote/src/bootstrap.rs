//! Native credential storage and transparent private-beta enrollment (`remote_bootstrap.py`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::error::{RemoteError, Res};
use crate::files::{atomic_write, is_hostname, is_identifier, urlsplit};
use crate::http::{HttpClient, HttpRequest, HttpTransport, ReqwestTransport};

pub const INSTALLATION_SCHEMA: &str = "org.stagepilot.private-beta-installation";
pub const DEFAULT_CONTROL_PLANE_ORIGIN: &str =
    "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev";
pub const DEFAULT_REMOTE_PORT: u16 = 18766;

pub fn trusted_control_plane_origins() -> Vec<String> {
    vec![DEFAULT_CONTROL_PLANE_ORIGIN.to_owned()]
}

/// Validate/truncate a self-reported device hostname before it is sent. Untrusted, display-only.
pub fn sanitize_device_name(value: Option<&str>) -> Option<String> {
    let value = value?;
    let cleaned: String = value
        .chars()
        .filter(|c| !matches!(*c, '\u{0}'..='\u{1f}' | '\u{7f}'))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.chars().take(253).collect())
}

fn credential_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^spi_([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\.([A-Za-z0-9_-]{32,})$")
            .expect("re")
    })
}

/// The installation id embedded in a well-formed credential.
pub fn credential_installation(credential: &str) -> Option<String> {
    credential_re()
        .captures(credential)
        .map(|c| c[1].to_owned())
}

/// Storage for the installation credential (OS Keychain / Credential Manager in production).
pub trait RemoteCredentialStore: Send + Sync {
    fn get(&self, installation_id: &str) -> Res<Option<String>>;
    fn set(&self, installation_id: &str, credential: &str) -> Res<()>;
    fn delete(&self, installation_id: &str) -> Res<()>;
}

/// In-memory store used by tests and by hosts without an OS secret store.
#[derive(Default)]
pub struct MemoryCredentials {
    pub values: Mutex<BTreeMap<String, String>>,
}

impl RemoteCredentialStore for MemoryCredentials {
    fn get(&self, id: &str) -> Res<Option<String>> {
        Ok(self.values.lock().unwrap().get(id).cloned())
    }

    fn set(&self, id: &str, credential: &str) -> Res<()> {
        self.values
            .lock()
            .unwrap()
            .insert(id.to_owned(), credential.to_owned());
        Ok(())
    }

    fn delete(&self, id: &str) -> Res<()> {
        self.values.lock().unwrap().remove(id);
        Ok(())
    }
}

/// Talks to the authenticated Tauri credential broker on loopback.
pub struct NativeRemoteCredentialStore {
    origin: String,
    authorization: String,
    transport: Arc<dyn HttpTransport>,
}

const BROKER_UNAVAILABLE: &str = "The operating-system credential store is unavailable";

/// Real human Keychain/Credential Manager password entry takes far longer than a few seconds.
pub const BROKER_TIMEOUT_SECS: f64 = 90.0;

impl NativeRemoteCredentialStore {
    pub fn new(
        origin: Option<&str>,
        authorization: Option<&str>,
        transport: Arc<dyn HttpTransport>,
    ) -> Self {
        let env = |name: &str| std::env::var(name).unwrap_or_default();
        Self {
            origin: origin
                .filter(|v| !v.is_empty())
                .map_or_else(|| env("STAGEPILOT_CREDENTIAL_BROKER_ORIGIN"), str::to_owned),
            authorization: authorization
                .filter(|v| !v.is_empty())
                .map_or_else(|| env("STAGEPILOT_CREDENTIAL_BROKER_TOKEN"), str::to_owned),
            transport,
        }
    }

    pub fn from_environment() -> Self {
        Self::new(None, None, Arc::new(ReqwestTransport::new()))
    }

    fn request(
        &self,
        method: &str,
        id: &str,
        credential: Option<&str>,
    ) -> Res<crate::http::HttpResponse> {
        let parsed = urlsplit(&self.origin);
        if parsed.scheme != "http"
            || parsed.hostname.as_deref() != Some("127.0.0.1")
            || parsed.port.is_none()
            || parsed.port_invalid
            || !(parsed.path.is_empty() || parsed.path == "/")
            || parsed.query
            || parsed.fragment
            || parsed.username
            || parsed.password
            || self.authorization.is_empty()
        {
            return Err(RemoteError::provider(BROKER_UNAVAILABLE));
        }
        let request = HttpRequest {
            method: method.to_owned(),
            url: format!("{}/v1/credentials/{id}", self.origin),
            headers: vec![(
                "Authorization".into(),
                format!("Bearer {}", self.authorization),
            )],
            body: Some(credential.unwrap_or_default().to_owned()),
            timeout_secs: BROKER_TIMEOUT_SECS,
            ..HttpRequest::default()
        };
        self.transport
            .send(&request)
            .map_err(|_| RemoteError::provider(BROKER_UNAVAILABLE))
    }
}

impl RemoteCredentialStore for NativeRemoteCredentialStore {
    fn get(&self, id: &str) -> Res<Option<String>> {
        let response = self.request("GET", id, None)?;
        match response.status {
            404 => Ok(None),
            200 => Ok(Some(response.body)),
            _ => Err(RemoteError::provider(BROKER_UNAVAILABLE)),
        }
    }

    fn set(&self, id: &str, credential: &str) -> Res<()> {
        if self.request("PUT", id, Some(credential))?.status != 204 {
            return Err(RemoteError::provider(
                "The installation credential could not be saved",
            ));
        }
        Ok(())
    }

    fn delete(&self, id: &str) -> Res<()> {
        if self.request("DELETE", id, None)?.status != 204 {
            return Err(RemoteError::provider(
                "The installation credential could not be removed",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapMetadata {
    #[serde(rename = "schema")]
    pub schema_name: String,
    pub version: i64,
    #[serde(rename = "bundleId")]
    pub bundle_id: String,
    #[serde(rename = "controlPlaneOrigin")]
    pub control_plane_origin: String,
    #[serde(rename = "installationId")]
    pub installation_id: String,
    pub hostname: String,
    #[serde(rename = "remotePort")]
    pub remote_port: u16,
}

impl BootstrapMetadata {
    fn validate(&self) -> Res<()> {
        if !is_identifier(&self.installation_id) {
            return Err(RemoteError::value("invalid installationId"));
        }
        if self.remote_port < 1024 {
            return Err(RemoteError::value("remotePort out of range"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapState {
    #[serde(default)]
    pub active: Option<BootstrapMetadata>,
    /// A disabled/revoked-but-known installation, kept with its still-valid credential so a future
    /// genuine re-enable proves ownership through the authenticated reactivate path.
    #[serde(default)]
    pub retired: Option<BootstrapMetadata>,
    #[serde(default, rename = "enrollmentNonce")]
    pub enrollment_nonce: Option<String>,
    /// Accepted on input, never written back.
    #[serde(default, rename = "consumedBundleIds", skip_serializing)]
    pub legacy_consumed_bundle_ids: Vec<String>,
}

impl BootstrapState {
    fn validate(&self) -> Res<()> {
        for metadata in [&self.active, &self.retired].into_iter().flatten() {
            metadata.validate()?;
        }
        Ok(())
    }
}

/// Retry pacing, injectable so tests do not sleep.
#[derive(Clone)]
pub struct Pacing {
    pub sleep: Arc<dyn Fn(f64) + Send + Sync>,
    pub random: Arc<dyn Fn() -> f64 + Send + Sync>,
}

impl Pacing {
    pub fn real() -> Self {
        Self {
            sleep: Arc::new(|seconds| {
                std::thread::sleep(std::time::Duration::from_secs_f64(seconds))
            }),
            random: Arc::new(rand::random::<f64>),
        }
    }

    pub fn instant() -> Self {
        Self {
            sleep: Arc::new(|_| {}),
            random: Arc::new(|| 0.0),
        }
    }

    /// `max(retry_after, min(0.5 * 2**attempt + random * 0.25, 5.0))`.
    pub fn backoff(&self, retry_after: u32, attempt: u32) -> f64 {
        let computed = (0.5 * f64::from(2_u32.pow(attempt)) + (self.random)() * 0.25).min(5.0);
        f64::from(retry_after).max(computed)
    }
}

/// `max(0, min(300, int(Retry-After)))`, 0 on anything unparsable.
pub fn parse_retry_after(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.trim().parse::<i64>().ok())
        .map_or(0, |v| v.clamp(0, 300) as u32)
}

pub struct DesktopBootstrapStore {
    pub path: PathBuf,
    credentials: Arc<dyn RemoteCredentialStore>,
    trusted_origins: Vec<String>,
    /// Process-lifetime cache: the OS store is hit once per installation id, not on every poll.
    /// Never persisted.
    cache: Mutex<BTreeMap<String, String>>,
    default_transport: Arc<dyn HttpTransport>,
}

fn unavailable_enrollment() -> RemoteError {
    RemoteError::provider(
        "Could not reach the enrollment service. Check your Internet connection and try again.",
    )
}

struct Enrolled {
    installation_id: String,
    hostname: String,
    credential: String,
}

/// Extract and validate `installationId`, `hostname`, `installationCredential` from a response.
fn parse_binding(
    response: &crate::http::HttpResponse,
    invalid: &str,
    unbound: &str,
) -> Res<Enrolled> {
    let invalid_err = || RemoteError::provider(invalid);
    let payload = response.json().map_err(|_| invalid_err())?;
    let field = |key: &str| payload.get(key).cloned().ok_or_else(invalid_err);
    let installation_id = field("installationId")?;
    let hostname = field("hostname")?;
    let credential = field("installationCredential")?;
    let unbound_err = || RemoteError::provider(unbound);
    let (Some(id), Some(host), Some(cred)) = (
        installation_id.as_str(),
        hostname.as_str(),
        credential.as_str(),
    ) else {
        return Err(unbound_err());
    };
    let bound = is_identifier(id)
        && credential_installation(cred).as_deref() == Some(id)
        && is_hostname(host)
        && host
            .split_once('.')
            .is_some_and(|(_, rest)| host == format!("sp-{id}.{rest}"));
    if !bound {
        return Err(unbound_err());
    }
    Ok(Enrolled {
        installation_id: id.to_owned(),
        hostname: host.to_owned(),
        credential: cred.to_owned(),
    })
}

impl DesktopBootstrapStore {
    pub fn new(
        path: PathBuf,
        credentials: Option<Arc<dyn RemoteCredentialStore>>,
        trusted_origins: Option<Vec<String>>,
    ) -> Res<Self> {
        if !path.is_absolute() {
            return Err(RemoteError::value(
                "Desktop bootstrap state path must be absolute",
            ));
        }
        let transport: Arc<dyn HttpTransport> = Arc::new(ReqwestTransport::new());
        Ok(Self {
            path,
            credentials: credentials.unwrap_or_else(|| {
                Arc::new(NativeRemoteCredentialStore::new(
                    None,
                    None,
                    transport.clone(),
                ))
            }),
            trusted_origins: trusted_origins.unwrap_or_else(trusted_control_plane_origins),
            cache: Mutex::new(BTreeMap::new()),
            default_transport: transport,
        })
    }

    fn client(&self, origin: &str, transport: Option<&Arc<dyn HttpTransport>>) -> HttpClient {
        HttpClient::new(
            origin,
            20.0,
            transport
                .cloned()
                .unwrap_or_else(|| self.default_transport.clone()),
        )
    }

    fn trusted(&self, origin: &str) -> Res<()> {
        if self.trusted_origins.iter().any(|o| o == origin) {
            Ok(())
        } else {
            Err(RemoteError::provider(
                "The enrollment service is not trusted",
            ))
        }
    }

    /// Test/diagnostic access to the in-memory credential cache.
    pub fn cache_clear(&self) {
        self.cache.lock().unwrap().clear();
    }

    pub fn cache_contains(&self, id: &str) -> bool {
        self.cache.lock().unwrap().contains_key(id)
    }

    pub fn credentials(&self) -> Arc<dyn RemoteCredentialStore> {
        self.credentials.clone()
    }

    fn cache_set(&self, id: &str, credential: &str) {
        self.cache
            .lock()
            .unwrap()
            .insert(id.to_owned(), credential.to_owned());
    }

    fn cache_drop(&self, id: &str) {
        self.cache.lock().unwrap().remove(id);
    }

    fn metadata_from(
        &self,
        template: &BootstrapMetadata,
        enrolled: &Enrolled,
    ) -> BootstrapMetadata {
        BootstrapMetadata {
            schema_name: INSTALLATION_SCHEMA.to_owned(),
            version: template.version,
            bundle_id: template.bundle_id.clone(),
            control_plane_origin: template.control_plane_origin.clone(),
            installation_id: enrolled.installation_id.clone(),
            hostname: enrolled.hostname.clone(),
            remote_port: template.remote_port,
        }
    }

    /// Transparently create and securely retain this installation identity.
    pub fn ensure_enrolled(
        &self,
        control_plane_origin: &str,
        remote_port: u16,
        transport: Option<&Arc<dyn HttpTransport>>,
        pacing: &Pacing,
        device_name: Option<&str>,
    ) -> Res<BootstrapMetadata> {
        self.trusted(control_plane_origin)?;
        let mut current = self.state()?;
        if let Some(active) = current.active.clone() {
            self.credential(&active)?;
            return Ok(active);
        }
        if let Some(retired) = current.retired.clone() {
            // A known prior installation: reactivate through the authenticated path so the
            // anonymous per-network enrollment quota is never touched. Errors propagate; they
            // must never fall through to minting a second installation.
            return self.reactivate(&retired, transport);
        }
        if current.enrollment_nonce.is_none() {
            current.enrollment_nonce = Some(Uuid::new_v4().to_string());
            self.write(&current)?;
        }
        let client = self.client(control_plane_origin, transport);
        let mut payload = json!({"nonce": current.enrollment_nonce});
        if let Some(name) = sanitize_device_name(device_name) {
            payload["deviceName"] = json!(name);
        }
        let mut response: Option<crate::http::HttpResponse> = None;
        for attempt in 0..3_u32 {
            match client.request(
                "POST",
                "/v1/installations/enroll",
                &[],
                &[],
                Some(payload.clone()),
            ) {
                Err(_) => {
                    if attempt == 2 {
                        return Err(unavailable_enrollment());
                    }
                }
                Ok(r) => {
                    let done = ![429, 503].contains(&r.status) || attempt == 2;
                    response = Some(r);
                    if done {
                        break;
                    }
                }
            }
            let retry_after = response
                .as_ref()
                .map_or(0, |r| parse_retry_after(r.header("retry-after")));
            (pacing.sleep)(pacing.backoff(retry_after, attempt));
        }
        let Some(response) = response else {
            return Err(unavailable_enrollment());
        };
        if response.status == 429 {
            return Err(RemoteError::provider(
                "This computer has reached its enrollment limit for now. Try again later.",
            ));
        }
        if ![200, 201].contains(&response.status) {
            return Err(RemoteError::provider(
                "The enrollment service is unavailable; retry later",
            ));
        }
        let enrolled = parse_binding(
            &response,
            "The enrollment response was invalid",
            "The enrollment response was not installation-bound",
        )?;
        let metadata = BootstrapMetadata {
            schema_name: INSTALLATION_SCHEMA.to_owned(),
            version: 1,
            bundle_id: enrolled.installation_id.clone(),
            control_plane_origin: control_plane_origin.to_owned(),
            installation_id: enrolled.installation_id.clone(),
            hostname: enrolled.hostname.clone(),
            remote_port,
        };
        metadata.validate()?;
        self.credentials
            .set(&enrolled.installation_id, &enrolled.credential)?;
        self.cache_set(&enrolled.installation_id, &enrolled.credential);
        current.active = Some(metadata.clone());
        // The nonce is deliberately retained: it is the durable installation identity, so a
        // replayed enrollment re-provisions the SAME hostname instead of minting a new one.
        if let Err(error) = self.write(&current) {
            let _ = self.credentials.delete(&enrolled.installation_id);
            self.cache_drop(&enrolled.installation_id);
            return Err(error);
        }
        Ok(metadata)
    }

    pub fn state(&self) -> Res<BootstrapState> {
        if !self.path.exists() {
            return Ok(BootstrapState::default());
        }
        let unavailable =
            |_: std::io::Error| RemoteError::provider("Bootstrap metadata is unavailable");
        let link = std::fs::symlink_metadata(&self.path).map_err(unavailable)?;
        if link.file_type().is_symlink() || link.len() > 32_768 {
            return Err(RemoteError::value("Invalid bootstrap metadata file"));
        }
        let text = std::fs::read_to_string(&self.path).map_err(unavailable)?;
        let state: BootstrapState = serde_json::from_str(&text)?;
        state.validate()?;
        Ok(state)
    }

    pub fn credential(&self, metadata: &BootstrapMetadata) -> Res<String> {
        if let Some(cached) = self.cache.lock().unwrap().get(&metadata.installation_id) {
            return Ok(cached.clone());
        }
        let value = self
            .credentials
            .get(&metadata.installation_id)?
            .unwrap_or_default();
        if credential_installation(&value).as_deref() != Some(metadata.installation_id.as_str()) {
            return Err(RemoteError::provider(
                "The installation credential is unavailable",
            ));
        }
        self.cache_set(&metadata.installation_id, &value);
        Ok(value)
    }

    fn authenticated_post(
        &self,
        metadata: &BootstrapMetadata,
        route: &str,
        transport: Option<&Arc<dyn HttpTransport>>,
        on_transport_error: RemoteError,
    ) -> Res<crate::http::HttpResponse> {
        self.trusted(&metadata.control_plane_origin)?;
        let credential = self.credential(metadata)?;
        let client = self.client(&metadata.control_plane_origin, transport);
        client
            .request(
                "POST",
                &format!("/v1/installations/{}/{route}", metadata.installation_id),
                &[("authorization".into(), format!("Bearer {credential}"))],
                &[],
                None,
            )
            .map_err(|_| on_transport_error)
    }

    /// Authenticated "Regenerate Remote link": mint a brand-new installation id/hostname while
    /// proving ownership of the current one through its credential, never the anonymous quota.
    pub fn reenroll(
        &self,
        metadata: &BootstrapMetadata,
        transport: Option<&Arc<dyn HttpTransport>>,
    ) -> Res<BootstrapMetadata> {
        let response = self.authenticated_post(
            metadata,
            "reenroll",
            transport,
            RemoteError::provider("The Remote link could not be regenerated"),
        )?;
        if ![200, 201].contains(&response.status) {
            return Err(RemoteError::provider(
                "The Remote link could not be regenerated; try again",
            ));
        }
        let enrolled = parse_binding(
            &response,
            "The re-enrollment response was invalid",
            "The re-enrollment response was not installation-bound",
        )?;
        let new_metadata = self.metadata_from(metadata, &enrolled);
        new_metadata.validate()?;
        // Store the new credential before swapping `active`, so a crash between the two writes
        // still leaves a usable (old) identity, never a state with neither credential present.
        self.credentials
            .set(&enrolled.installation_id, &enrolled.credential)?;
        self.cache_set(&enrolled.installation_id, &enrolled.credential);
        let mut state = self.state()?;
        state.active = Some(new_metadata.clone());
        self.write(&state)?;
        self.credentials.delete(&metadata.installation_id)?;
        self.cache_drop(&metadata.installation_id);
        Ok(new_metadata)
    }

    /// Local bookkeeping after the caller revoked the control-plane tunnel/DNS for `metadata`.
    /// By default the credential is KEPT and `active` moves to `retired`; `hard` purges it (the
    /// control plane already refused it).
    pub fn finish_revoke(&self, metadata: &BootstrapMetadata, hard: bool) -> Res<()> {
        if hard {
            self.credentials.delete(&metadata.installation_id)?;
            self.cache_drop(&metadata.installation_id);
        }
        let mut state = self.state()?;
        if state
            .active
            .as_ref()
            .is_some_and(|a| a.installation_id == metadata.installation_id)
        {
            state.active = None;
            state.retired = if hard { None } else { Some(metadata.clone()) };
            // enrollment_nonce intentionally stays: it is the durable identity, not a secret.
            self.write(&state)?;
        }
        Ok(())
    }

    /// Re-enable a known, disabled/revoked installation by proving ownership with its credential.
    pub fn reactivate(
        &self,
        metadata: &BootstrapMetadata,
        transport: Option<&Arc<dyn HttpTransport>>,
    ) -> Res<BootstrapMetadata> {
        let response =
            self.authenticated_post(metadata, "reactivate", transport, unavailable_enrollment())?;
        if response.status == 429 {
            return Err(RemoteError::provider(
                "This computer has reached its enrollment limit for now. Try again later.",
            ));
        }
        if [401, 403].contains(&response.status) {
            return Err(RemoteError::PermanentlyRevoked(
                "This installation's credential was revoked. Contact beta support to recover this installation."
                    .to_owned(),
            ));
        }
        if ![200, 201].contains(&response.status) {
            return Err(RemoteError::provider(
                "The enrollment service is unavailable; retry later",
            ));
        }
        let enrolled = parse_binding(
            &response,
            "The reactivation response was invalid",
            "The reactivation response was not installation-bound",
        )?;
        if enrolled.installation_id != metadata.installation_id
            || enrolled.hostname != metadata.hostname
        {
            return Err(RemoteError::provider(
                "The reactivation response was not installation-bound",
            ));
        }
        let new_metadata = self.metadata_from(metadata, &enrolled);
        new_metadata.validate()?;
        self.credentials
            .set(&enrolled.installation_id, &enrolled.credential)?;
        self.cache_set(&enrolled.installation_id, &enrolled.credential);
        let mut state = self.state()?;
        state.retired = None;
        state.active = Some(new_metadata.clone());
        self.write(&state)?;
        Ok(new_metadata)
    }

    /// Force a genuinely new installation/hostname on the next enrollment (clears the nonce too).
    pub fn discard_identity(&self, metadata: &BootstrapMetadata) -> Res<()> {
        self.credentials.delete(&metadata.installation_id)?;
        self.cache_drop(&metadata.installation_id);
        let mut state = self.state()?;
        if state
            .active
            .as_ref()
            .is_some_and(|a| a.installation_id == metadata.installation_id)
        {
            state.active = None;
        }
        if state
            .retired
            .as_ref()
            .is_some_and(|a| a.installation_id == metadata.installation_id)
        {
            state.retired = None;
        }
        state.enrollment_nonce = None;
        self.write(&state)
    }

    fn write(&self, state: &BootstrapState) -> Res<()> {
        atomic_write(&self.path, &serde_json::to_string(state)?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
