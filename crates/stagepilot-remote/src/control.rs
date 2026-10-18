//! Local-only Remote provisioning (`remote_control.py`): account secrets never enter
//! installations.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::error::{RemoteError, Res};
use crate::files::{atomic_write, create_private_dir, ControlConfig, DesiredRemote};
use crate::platform::FileLocker;
use crate::provider::TunnelProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Disabled,
    Enabling,
    Enabled,
    Revoking,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabling => "enabling",
            Self::Enabled => "enabled",
            Self::Revoking => "revoking",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvisionState {
    #[serde(default = "new_uuid")]
    pub installation: String,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub phase: Phase,
    #[serde(default)]
    pub binding: BTreeMap<String, Value>,
    #[serde(default)]
    pub tunnel_id: String,
}

fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

impl Default for ProvisionState {
    fn default() -> Self {
        Self {
            installation: new_uuid(),
            generation: String::new(),
            phase: Phase::Disabled,
            binding: BTreeMap::new(),
            tunnel_id: String::new(),
        }
    }
}

pub struct RemoteControl<P: TunnelProvider> {
    pub config: ControlConfig,
    provider: P,
    locker: Arc<dyn FileLocker>,
    pub state_path: std::path::PathBuf,
    pub desired_path: std::path::PathBuf,
    pub token_path: std::path::PathBuf,
}

impl<P: TunnelProvider> RemoteControl<P> {
    pub fn new(config: ControlConfig, provider: P, locker: Arc<dyn FileLocker>) -> Self {
        Self {
            state_path: config.control_dir.join("state.json"),
            desired_path: config.installation_dir.join("remote.json"),
            token_path: config.installation_dir.join("connector.token"),
            config,
            provider,
            locker,
        }
    }

    pub fn apply(&self, action: &str) -> Res<Map<String, Value>> {
        create_private_dir(&self.config.control_dir)?;
        let _guard = self
            .locker
            .acquire(&self.config.control_dir.join("control.lock"))?;
        self.apply_locked(action)
    }

    fn load_state(&self) -> Res<ProvisionState> {
        if !self.state_path.exists() {
            return Ok(ProvisionState::default());
        }
        let text = std::fs::read_to_string(&self.state_path)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn result(&self, state: &ProvisionState) -> Map<String, Value> {
        let Value::Object(map) = json!({
            "phase": state.phase.as_str(),
            "installation": state.installation,
            "hostname": self.config.hostname,
            "tunnel_id": state.tunnel_id,
        }) else {
            unreachable!()
        };
        map
    }

    fn save(&self, state: &ProvisionState) -> Res<()> {
        atomic_write(&self.state_path, &serde_json::to_string(state)?)
    }

    fn origin(&self) -> String {
        format!("https://{}", self.config.hostname)
    }

    fn apply_locked(&self, action: &str) -> Res<Map<String, Value>> {
        let mut state = match self.load_state() {
            Ok(state) => state,
            Err(error) => {
                if action == "disable" {
                    atomic_write(&self.desired_path, &DesiredRemote::default().to_json())?;
                    let _ = std::fs::remove_file(&self.token_path);
                }
                return Err(error);
            }
        };
        let binding: BTreeMap<String, Value> = BTreeMap::from([
            ("account".into(), json!(self.config.account_id)),
            ("zone".into(), json!(self.config.zone_id)),
            ("hostname".into(), json!(self.config.hostname)),
            ("port".into(), json!(self.config.remote_port)),
            (
                "export".into(),
                json!(self.config.installation_dir.to_string_lossy()),
            ),
        ]);
        if !state.binding.is_empty() && state.binding != binding {
            return Err(RemoteError::provider(
                "Configuration changed; preserve the original control state",
            ));
        }
        state.binding = binding;
        if action == "status" {
            return Ok(self.result(&state));
        }
        if action != "enable" && action != "disable" {
            return Err(RemoteError::value("Unknown Remote operation"));
        }
        if action == "enable" {
            if state.phase == Phase::Revoking {
                return Err(RemoteError::provider(
                    "Finish retrying disable before re-enabling",
                ));
            }
            if state.phase == Phase::Disabled {
                state.generation = new_uuid();
                state.phase = Phase::Enabling;
                atomic_write(
                    &self.desired_path,
                    &DesiredRemote::marker(&self.origin(), self.config.remote_port).to_json(),
                )?;
            }
            self.save(&state)?;
            let name = format!("stagepilot-{}-{}", state.installation, state.generation);
            let (tunnel_id, token) =
                self.provider
                    .ensure(&name, &self.config.hostname, self.config.remote_port)?;
            atomic_write(&self.token_path, &token)?;
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
            state.tunnel_id = tunnel_id;
            state.phase = Phase::Enabled;
        } else {
            // Journal intent first so an interrupted disable cannot be mistaken for an idempotent
            // enable of the old installation generation.
            if state.phase != Phase::Disabled {
                state.phase = Phase::Revoking;
                self.save(&state)?;
            }
            // Local fail-closed state always precedes remote network revocation. Keep only the
            // non-secret stable-origin marker for future UI requests.
            atomic_write(
                &self.desired_path,
                &DesiredRemote::marker(&self.origin(), self.config.remote_port).to_json(),
            )?;
            let _ = std::fs::remove_file(&self.token_path);
            if state.phase == Phase::Revoking {
                self.provider.revoke(
                    &format!("stagepilot-{}-{}", state.installation, state.generation),
                    &self.config.hostname,
                )?;
            }
            state.phase = Phase::Disabled;
            state.tunnel_id = String::new();
        }
        self.save(&state)?;
        Ok(self.result(&state))
    }
}
