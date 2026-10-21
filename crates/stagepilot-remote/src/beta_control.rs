//! Installation-side client for the authenticated private-beta control plane
//! (`remote_beta_control.py`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::bootstrap::{parse_retry_after, sanitize_device_name, Pacing};
use crate::control::Phase;
use crate::error::{RemoteError, Res};
use crate::files::{
    atomic_write, create_private_dir, is_group_or_world_readable, BetaControlConfig, DesiredRemote,
};
use crate::http::{HttpClient, HttpResponse};
use crate::platform::FileLocker;

/// The Worker's fixed `diagnostic` string, if present (never raw bodies).
fn worker_diagnostic(response: &HttpResponse) -> Option<String> {
    let body = response.json().ok()?;
    body.get("diagnostic")?.as_str().map(str::to_owned)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BetaProvisionState {
    #[serde(default)]
    pub phase: Phase,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub binding: BTreeMap<String, Value>,
    #[serde(default)]
    pub retire: bool,
}

pub type ActionHandler = Box<dyn Fn() -> Res<()> + Send + Sync>;

pub type CredentialProvider = Arc<dyn Fn() -> Res<String> + Send + Sync>;

pub struct BetaRemoteControl {
    pub config: BetaControlConfig,
    client: HttpClient,
    credential_provider: Option<CredentialProvider>,
    pacing: Pacing,
    locker: Arc<dyn FileLocker>,
    action_handlers: BTreeMap<String, ActionHandler>,
    pub connector_token: Option<String>,
    pub state_path: PathBuf,
    pub desired_path: PathBuf,
    /// Cleanup-only compatibility path for tokens written by older beta builds.
    pub token_path: PathBuf,
    device_name: Option<String>,
}

type Object = Map<String, Value>;

impl BetaRemoteControl {
    pub fn new(
        config: BetaControlConfig,
        client: HttpClient,
        locker: Arc<dyn FileLocker>,
        credential_provider: Option<CredentialProvider>,
        pacing: Pacing,
        device_name: Option<&str>,
    ) -> Self {
        Self {
            state_path: config.state_dir.join("state.json"),
            desired_path: config.installation_dir.join("remote.json"),
            token_path: config.installation_dir.join("connector.token"),
            config,
            client,
            credential_provider,
            pacing,
            locker,
            action_handlers: BTreeMap::new(),
            connector_token: None,
            device_name: sanitize_device_name(device_name),
        }
    }

    /// Handlers for operator-queued pending actions (see `apply_pending_actions`).
    pub fn with_action_handlers(mut self, handlers: BTreeMap<String, ActionHandler>) -> Self {
        self.action_handlers = handlers;
        self
    }

    pub fn apply(&mut self, action: &str) -> Res<Object> {
        create_private_dir(&self.config.state_dir)?;
        let locker = self.locker.clone();
        let _guard = locker.acquire(&self.config.state_dir.join("control.lock"))?;
        self.apply_locked(action)
    }

    fn apply_locked(&mut self, action: &str) -> Res<Object> {
        if action != "status" {
            self.connector_token = None;
        }
        let mut state = match self.load_state() {
            Ok(state) => state,
            Err(error) => {
                if action == "disable" {
                    self.write_disabled_marker()?;
                    let _ = std::fs::remove_file(&self.token_path);
                }
                return Err(error);
            }
        };
        let binding: BTreeMap<String, Value> = BTreeMap::from([
            (
                "control_plane".into(),
                json!(self.config.control_plane_url.trim_end_matches('/')),
            ),
            ("installation".into(), json!(self.config.installation_id)),
            ("hostname".into(), json!(self.config.hostname)),
            ("port".into(), json!(self.config.remote_port)),
            (
                "export".into(),
                json!(self.config.installation_dir.to_string_lossy()),
            ),
        ]);
        if !state.binding.is_empty() && state.binding != binding {
            return Err(RemoteError::provider(
                "Configuration changed; preserve the original beta control state",
            ));
        }
        state.binding = binding;
        if action == "status" {
            return Ok(self.status_of(&state));
        }
        if !matches!(action, "enable" | "disable" | "revoke" | "reconcile") {
            return Err(RemoteError::value("Unknown Remote operation"));
        }
        if action == "enable" {
            if state.phase == Phase::Revoking {
                return Err(RemoteError::provider(
                    "Finish retrying disable before re-enabling",
                ));
            }
            if state.phase == Phase::Disabled {
                state.generation = Uuid::new_v4().to_string();
                state.phase = Phase::Enabling;
                state.retire = false;
                self.write_disabled_marker()?;
                self.save(&state)?;
            }
            return self.provision(&mut state, "provision");
        }
        if matches!(action, "disable" | "revoke") || state.phase == Phase::Revoking {
            let retire = action == "revoke" || state.retire;
            return self.disable(&mut state, retire);
        }
        if state.phase == Phase::Enabling {
            return self.provision(&mut state, "provision");
        }
        if action == "reconcile" && state.phase == Phase::Enabled {
            return self.provision(&mut state, "reconcile");
        }
        Ok(self.status_of(&state))
    }

    fn load_state(&self) -> Res<BetaProvisionState> {
        if !self.state_path.exists() {
            return Ok(BetaProvisionState::default());
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(
            &self.state_path,
        )?)?)
    }

    fn save(&self, state: &BetaProvisionState) -> Res<()> {
        atomic_write(&self.state_path, &serde_json::to_string(state)?)
    }

    fn origin(&self) -> String {
        format!("https://{}", self.config.hostname)
    }

    fn provision(&mut self, state: &mut BetaProvisionState, operation: &str) -> Res<Object> {
        if state.generation.is_empty() {
            return Err(RemoteError::provider("Missing installation generation"));
        }
        let response = self.request(
            "POST",
            &format!(
                "/v1/installations/{}/{operation}",
                self.config.installation_id
            ),
            json!({"generation": state.generation}),
        )?;
        let text = |key: &str| response.get(key).and_then(Value::as_str);
        if text("installationId") != Some(&self.config.installation_id)
            || text("hostname") != Some(&self.config.hostname)
            || text("generation") != Some(&state.generation)
            || text("phase") != Some("provisioned")
        {
            return Err(RemoteError::provider(
                "Control-plane identity response did not match",
            ));
        }
        let token = match response.get("tunnelToken") {
            Some(Value::String(t))
                if t.chars().count() >= 20 && !t.chars().any(char::is_whitespace) =>
            {
                t.clone()
            }
            _ => {
                return Err(RemoteError::provider(
                    "Installation tunnel credential unavailable",
                ))
            }
        };
        atomic_write(
            &self.desired_path,
            &DesiredRemote {
                enabled: true,
                generation: state.generation.clone(),
                public_origin: self.origin(),
                port: self.config.remote_port,
            }
            .to_json(),
        )?;
        state.phase = Phase::Enabled;
        self.save(state)?;
        self.connector_token = Some(token);
        Ok(self.status_of(state))
    }

    fn disable(&mut self, state: &mut BetaProvisionState, retire: bool) -> Res<Object> {
        if state.phase != Phase::Disabled || retire {
            state.phase = Phase::Revoking;
        }
        state.retire = retire;
        self.save(state)?;
        self.write_disabled_marker()?;
        let _ = std::fs::remove_file(&self.token_path);
        if !state.generation.is_empty() || retire {
            let operation = if retire { "revoke" } else { "disable" };
            let response = self.request(
                "POST",
                &format!(
                    "/v1/installations/{}/{operation}",
                    self.config.installation_id
                ),
                json!({}),
            )?;
            let text = |key: &str| response.get(key).and_then(Value::as_str);
            if text("installationId") != Some(&self.config.installation_id)
                || text("hostname") != Some(&self.config.hostname)
                || text("phase") != Some("disabled")
                || (retire && response.get("revoked") != Some(&Value::Bool(true)))
            {
                return Err(RemoteError::provider(
                    "Control-plane revocation response did not match",
                ));
            }
        }
        state.phase = Phase::Disabled;
        state.generation = String::new();
        self.save(state)?;
        Ok(self.status_of(state))
    }

    /// Apply operator-queued actions surfaced by the status route. Each is applied locally and
    /// only then acknowledged, so a crash mid-apply retries it; handlers must be idempotent.
    pub fn apply_pending_actions(&mut self) -> Res<Vec<String>> {
        let id = self.config.installation_id.clone();
        let response = self.request("GET", &format!("/v1/installations/{id}/status"), json!({}))?;
        let Some(Value::Array(actions)) = response.get("pendingActions") else {
            return Ok(Vec::new());
        };
        let mut applied = Vec::new();
        for action in actions {
            let Value::Object(action) = action else {
                continue;
            };
            let (Some(kind), Some(action_id)) = (
                action.get("kind").and_then(Value::as_str),
                action.get("actionId").and_then(Value::as_str),
            ) else {
                continue;
            };
            // Never act on an action attributed to a different installation.
            if action.get("installationId").and_then(Value::as_str) != Some(id.as_str()) {
                continue;
            }
            let Some(handler) = self.action_handlers.get(kind) else {
                continue;
            };
            handler()?;
            self.request(
                "POST",
                &format!("/v1/installations/{id}/pending-actions/{action_id}/ack"),
                json!({}),
            )?;
            applied.push(kind.to_owned());
        }
        Ok(applied)
    }

    /// Minimal existence/deviceName check-in; only the GET status route, never provisioning.
    pub fn lightweight_status(&mut self) -> Res<Object> {
        let id = self.config.installation_id.clone();
        self.request("GET", &format!("/v1/installations/{id}/status"), json!({}))
    }

    pub fn request_approval(&mut self, reason: &str) -> Res<Object> {
        let id = self.config.installation_id.clone();
        self.request(
            "POST",
            &format!("/v1/installations/{id}/approval-requests"),
            json!({"reason": reason}),
        )
    }

    pub fn approval_requests(&mut self) -> Res<Vec<Value>> {
        let id = self.config.installation_id.clone();
        let response = self.request(
            "GET",
            &format!("/v1/installations/{id}/approval-requests"),
            json!({}),
        )?;
        match response.get("requests") {
            Some(Value::Array(items)) => {
                Ok(items.iter().filter(|i| i.is_object()).cloned().collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn write_disabled_marker(&self) -> Res<()> {
        atomic_write(
            &self.desired_path,
            &DesiredRemote::marker(&self.origin(), self.config.remote_port).to_json(),
        )
    }

    fn credential(&self) -> Res<String> {
        let invalid = || RemoteError::provider("Installation credential is invalid");
        let expected_prefix = format!("spi_{}.", self.config.installation_id);
        let well_formed = |token: &str| {
            token.starts_with(&expected_prefix) && !token.chars().any(char::is_whitespace)
        };
        if let Some(provider) = &self.credential_provider {
            let token = provider()?.trim().to_owned();
            return if well_formed(&token) {
                Ok(token)
            } else {
                Err(invalid())
            };
        }
        let Some(file) = &self.config.credential_file else {
            return Err(RemoteError::provider("Installation credential unavailable"));
        };
        let unavailable =
            |_: std::io::Error| RemoteError::provider("Installation credential unavailable");
        if std::fs::symlink_metadata(file)
            .map_err(unavailable)?
            .file_type()
            .is_symlink()
        {
            return Err(RemoteError::provider(
                "Installation credential file must not be a symlink",
            ));
        }
        if is_group_or_world_readable(file).map_err(unavailable)? {
            return Err(RemoteError::provider(
                "Installation credential file must be private (0600)",
            ));
        }
        let token = std::fs::read_to_string(file)
            .map_err(unavailable)?
            .trim()
            .to_owned();
        if well_formed(&token) {
            Ok(token)
        } else {
            Err(invalid())
        }
    }

    fn request(&mut self, method: &str, path: &str, mut payload: Value) -> Res<Object> {
        let mut query: Vec<(String, String)> = Vec::new();
        if let Some(name) = &self.device_name {
            if ["/status", "/provision", "/reconcile"]
                .iter()
                .any(|s| path.ends_with(s))
            {
                if method == "GET" {
                    // GET requests carry deviceName as a query parameter, not a JSON body.
                    query.push(("deviceName".into(), name.clone()));
                } else if let Value::Object(map) = &mut payload {
                    map.insert("deviceName".into(), json!(name));
                }
            }
        }
        let unavailable =
            || RemoteError::provider("Beta control plane unavailable; retry reconciliation");
        let mut response: Option<HttpResponse> = None;
        for attempt in 0..3_u32 {
            let headers = [(
                "Authorization".to_owned(),
                format!("Bearer {}", self.credential()?),
            )];
            let r = self
                .client
                .request(method, path, &headers, &query, Some(payload.clone()))
                .map_err(|_| unavailable())?;
            let stop = ![429, 503].contains(&r.status) || attempt == 2;
            let retry_after = parse_retry_after(r.header("retry-after"));
            response = Some(r);
            if stop {
                break;
            }
            (self.pacing.sleep)(self.pacing.backoff(retry_after, attempt));
        }
        let response = response.ok_or_else(unavailable)?;
        if [401, 403].contains(&response.status) {
            return Err(RemoteError::InstallationRevoked(
                "Installation enrollment is expired or revoked".to_owned(),
            ));
        }
        if !response.is_success() {
            if worker_diagnostic(&response).as_deref() == Some("hostname ownership conflict") {
                return Err(RemoteError::provider(
                    "Hostname ownership conflict; reset may be required",
                ));
            }
            return Err(RemoteError::provider(format!(
                "Beta control request failed (HTTP {})",
                response.status
            )));
        }
        match response.json().map_err(|_| unavailable())? {
            Value::Object(map) => Ok(map),
            _ => Err(RemoteError::provider("Beta control response was invalid")),
        }
    }

    fn status_of(&self, state: &BetaProvisionState) -> Object {
        let Value::Object(map) = json!({
            "phase": state.phase.as_str(),
            "installation": self.config.installation_id,
            "hostname": self.config.hostname,
            "generation": state.generation,
        }) else {
            unreachable!()
        };
        map
    }
}
