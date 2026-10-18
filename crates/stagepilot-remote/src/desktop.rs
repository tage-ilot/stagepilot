//! Packaged-desktop Remote lifecycle and bundled connector supervision (`remote_desktop.py`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};

use crate::beta_control::BetaRemoteControl;
use crate::bootstrap::{
    sanitize_device_name, BootstrapMetadata, DesktopBootstrapStore, Pacing,
    DEFAULT_CONTROL_PLANE_ORIGIN, DEFAULT_REMOTE_PORT,
};
use crate::connector::Connector;
use crate::error::{RemoteError, Res};
use crate::feature::RemoteFeature;
use crate::files::{atomic_write, read_desired, safe_status, BetaControlConfig};
use crate::http::{HttpClient, HttpTransport};
use crate::platform::{Platform, StopFlag};
use crate::status_map;

/// How often the always-on lightweight check-in re-runs after its first run. Conservative on
/// purpose: fleet visibility, not a liveness/health system.
pub const LIGHTWEIGHT_CHECKIN_INTERVAL_SECONDS: f64 = 6.0 * 60.0 * 60.0;

pub struct DesktopRemoteManager {
    pub root: PathBuf,
    pub cloudflared_binary: PathBuf,
    pub installation_dir: PathBuf,
    pub state_dir: PathBuf,
    pub desired_path: PathBuf,
    pub connector_status_path: PathBuf,
    permanently_revoked_marker: PathBuf,
    pub feature: RemoteFeature,
    pub bootstrap: Arc<DesktopBootstrapStore>,
    pub revoke_sessions: Option<Arc<dyn Fn() + Send + Sync>>,
    pub lan_port: u16,
    pub transport: Option<Arc<dyn HttpTransport>>,
    control_plane_origin: String,
    remote_port: u16,
    connector_token: Arc<Mutex<Option<String>>>,
    platform: Platform,
    pacing: Pacing,
    device_name: Option<String>,
}

pub struct DesktopOptions {
    pub bootstrap_store: Option<Arc<DesktopBootstrapStore>>,
    pub revoke_sessions: Option<Arc<dyn Fn() + Send + Sync>>,
    pub lan_port: u16,
    pub transport: Option<Arc<dyn HttpTransport>>,
    pub control_plane_origin: String,
    pub remote_port: u16,
    pub pacing: Pacing,
    pub device_name: Option<String>,
}

impl Default for DesktopOptions {
    fn default() -> Self {
        Self {
            bootstrap_store: None,
            revoke_sessions: None,
            lan_port: 8765,
            transport: None,
            control_plane_origin: DEFAULT_CONTROL_PLANE_ORIGIN.to_owned(),
            remote_port: DEFAULT_REMOTE_PORT,
            pacing: Pacing::real(),
            device_name: None,
        }
    }
}

type Object = Map<String, Value>;

/// The host name, sanitised for the control plane (`platform.node()`).
pub fn local_device_name() -> Option<String> {
    sanitize_device_name(
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .as_deref(),
    )
}

impl DesktopRemoteManager {
    pub fn new(
        root: &Path,
        cloudflared_binary: &Path,
        platform: Platform,
        options: DesktopOptions,
    ) -> Res<Self> {
        if !root.is_absolute() || !cloudflared_binary.is_absolute() {
            return Err(RemoteError::value("Desktop Remote paths must be absolute"));
        }
        let installation_dir = root.join("connector");
        let desired_path = installation_dir.join("remote.json");
        let bootstrap = match options.bootstrap_store {
            Some(store) => store,
            None => Arc::new(DesktopBootstrapStore::new(
                root.join("bootstrap.json"),
                None,
                None,
            )?),
        };
        Ok(Self {
            root: root.to_path_buf(),
            cloudflared_binary: cloudflared_binary.to_path_buf(),
            connector_status_path: installation_dir.join("connector-status.json"),
            installation_dir,
            state_dir: root.join("control"),
            permanently_revoked_marker: root.join("control").join("revoked.json"),
            feature: RemoteFeature::new(
                &desired_path,
                8766,
                platform.locker.clone(),
                platform.clock.clone(),
            ),
            desired_path,
            bootstrap,
            revoke_sessions: options.revoke_sessions,
            lan_port: options.lan_port,
            transport: options.transport,
            control_plane_origin: options.control_plane_origin,
            remote_port: options.remote_port,
            connector_token: Arc::new(Mutex::new(None)),
            platform,
            pacing: options.pacing,
            // Captured once per process, untrusted and display-only.
            device_name: options.device_name.or_else(local_device_name),
        })
    }

    fn binary_exists(&self) -> bool {
        self.cloudflared_binary.is_file()
    }

    pub fn status(&self) -> Object {
        let mut result = self.feature.status();
        let (provisioned, active) = match self.bootstrap.state() {
            Ok(state) => (state.active.is_some(), state.active),
            Err(_) => (false, None),
        };
        let credential_available = active
            .as_ref()
            .is_some_and(|a| self.bootstrap.credential(a).is_ok_and(|c| !c.is_empty()));
        result.insert("provisioned".into(), json!(provisioned));
        result.insert("credential_available".into(), json!(credential_available));
        result.insert("temporary_url".into(), json!(false));
        result.insert(
            "permanently_revoked".into(),
            json!(self.permanently_revoked_marker.exists()),
        );
        if !provisioned {
            result.insert("available".into(), json!(self.binary_exists()));
            result.insert("enabled".into(), json!(false));
            result.insert("state".into(), json!("off"));
            result.insert("url".into(), Value::Null);
            result.insert(
                "message".into(),
                json!(
                    "This private beta enrolls automatically when Remote Access is first enabled."
                ),
            );
        }
        result
    }

    pub fn enable(&self) -> Res<Object> {
        let metadata = self.active()?;
        let mut result = self.apply(&metadata, "enable")?;
        self.capture_connector_token(&mut result)?;
        let generation = result.get("generation").map(value_str).unwrap_or_default();
        if generation.is_empty() {
            return Err(RemoteError::provider(
                "Remote provisioning did not return a generation",
            ));
        }
        self.feature.set_managed_enabled(true, &generation)?;
        Ok(self.status())
    }

    /// Explicit recovery for a `retired` installation whose credential the control plane has
    /// permanently revoked: discard the local identity so the next enable mints a new one. Never
    /// silent, and never touches local production state (Remote users, local config).
    pub fn reset_identity(&self) -> Res<Object> {
        if let Some(retired) = self.bootstrap.state()?.retired {
            self.bootstrap.discard_identity(&retired)?;
        }
        self.feature.set_managed_enabled(false, "")?;
        self.publish_off_status()?;
        let _ = std::fs::remove_file(&self.permanently_revoked_marker);
        Ok(self.status())
    }

    pub fn disable(&self) -> Res<Object> {
        self.feature.set_managed_enabled(false, "")?;
        self.clear_connector_credential();
        let Some(active) = self.bootstrap.state()?.active else {
            self.publish_off_status()?;
            return Ok(self.status());
        };
        match self.apply(&active, "revoke") {
            Ok(_) | Err(RemoteError::InstallationRevoked(_)) => {}
            Err(other) => return Err(other),
        }
        self.retire_local(&active, false)?;
        self.publish_off_status()?;
        Ok(self.status())
    }

    /// Retire this identity and provision a new one through the authenticated re-enrollment
    /// route (not the quota-limited anonymous one). Idempotent and restart-safe; Remote users
    /// live in the local identity store and are untouched.
    pub fn regenerate(&self) -> Res<Object> {
        let active = self.bootstrap.state()?.active;
        let was_enabled = self.feature.intent().enabled;
        let Some(active) = active else {
            // Nothing to regenerate from; behave like a first-time enable.
            self.feature.set_managed_enabled(false, "")?;
            self.publish_off_status()?;
            if was_enabled {
                self.enable()?;
            } else {
                self.active()?;
            }
            return Ok(self.status());
        };
        // A reenroll failure is atomic server-side (or never arrived): the old identity and
        // credential stay usable, so surface the error and tear nothing down.
        let new_active = self.bootstrap.reenroll(&active, self.transport.as_ref())?;
        self.clear_connector_credential();
        if let Some(revoke) = &self.revoke_sessions {
            revoke();
        }
        let _ = std::fs::remove_file(self.state_dir.join("state.json"));
        self.feature.set_managed_enabled(false, "")?;
        self.publish_off_status()?;
        if was_enabled {
            self.enable()?;
        } else {
            // Confirm the new identity is reachable without forcing Remote back on.
            self.bootstrap.credential(&new_active)?;
        }
        Ok(self.status())
    }

    pub fn reconcile_control(&self) -> Res<()> {
        let Some(active) = self.bootstrap.state()?.active else {
            self.feature.set_managed_enabled(false, "")?;
            self.clear_connector_credential();
            return self.publish_off_status();
        };
        let mut result = match self.apply(&active, "reconcile") {
            Ok(result) => result,
            Err(RemoteError::InstallationRevoked(_)) => {
                self.feature.set_managed_enabled(false, "")?;
                self.retire_local(&active, true)?;
                return self.publish_off_status();
            }
            Err(other) => return Err(other),
        };
        let phase = result
            .get("phase")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if phase.as_deref() == Some("enabled") {
            self.capture_connector_token(&mut result)?;
            let generation = result.get("generation").map(value_str).unwrap_or_default();
            self.feature.set_managed_enabled(true, &generation)?;
        } else if phase.as_deref() == Some("disabled") {
            self.feature.set_managed_enabled(false, "")?;
            self.clear_connector_credential();
            // A persisted retiring state means provider revocation completed after a restart.
            let state: Value = std::fs::read_to_string(self.state_dir.join("state.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_else(|| json!({}));
            if state.get("retire") == Some(&Value::Bool(true)) {
                self.retire_local(&active, false)?;
            }
        }
        self.publish_off_status_if_disabled()
    }

    /// The supervisor loop: lightweight check-in, control reconcile and connector stepping.
    pub async fn run(self: Arc<Self>, stop: Arc<StopFlag>) {
        let mut reconcile_at = 0.0_f64;
        let mut reconcile_delay = 1.0_f64;
        // Always-on check-in: once at start and every interval, UNCONDITIONALLY (independent of
        // Remote Access). Tunnel/DNS provisioning below stays gated as before.
        let mut checkin_at = 0.0_f64;
        let token_slot = self.connector_token.clone();
        let provider: crate::connector::TokenProvider = Arc::new(move || {
            token_slot
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| RemoteError::io("Installation tunnel credential unavailable"))
        });
        let metrics = self.metrics_port();
        let connector = Arc::new(Mutex::new(Connector::new(
            &self.cloudflared_binary,
            &self.desired_path,
            &self.connector_status_path,
            metrics,
            Some(provider),
            Some(self.root.join("run/connector.token")),
            &self.platform,
        )));
        let clock = self.platform.clock.clone();
        while !stop.is_set() {
            if clock.monotonic() >= checkin_at {
                let me = self.clone();
                let _ = tokio::task::spawn_blocking(move || me.lightweight_checkin()).await;
                checkin_at = clock.monotonic() + LIGHTWEIGHT_CHECKIN_INTERVAL_SECONDS;
            }
            if self.feature.intent().enabled
                && self.connector_token.lock().unwrap().is_none()
                && clock.monotonic() >= reconcile_at
            {
                let me = self.clone();
                match tokio::task::spawn_blocking(move || me.reconcile_control()).await {
                    Ok(Ok(())) => reconcile_delay = 1.0,
                    Ok(Err(e)) if e.is_provider() || e.is_io_or_value() => {
                        reconcile_at = clock.monotonic() + reconcile_delay;
                        reconcile_delay = (reconcile_delay * 2.0).min(30.0);
                    }
                    _ => {}
                }
            }
            let step = {
                let (me, connector) = (self.clone(), connector.clone());
                tokio::task::spawn_blocking(move || {
                    let stepped = connector.lock().unwrap().step();
                    stepped.and_then(|()| me.publish_connector_status())
                })
                .await
            };
            if !matches!(step, Ok(Ok(()))) {
                let (me, connector) = (self.clone(), connector.clone());
                let _ = tokio::task::spawn_blocking(move || {
                    connector.lock().unwrap().stop();
                    let _ = me.publish_reconnecting_status();
                })
                .await;
            }
            stop.wait(0.5).await;
        }
        let (me, connector) = (self.clone(), connector.clone());
        let _ = tokio::task::spawn_blocking(move || {
            connector.lock().unwrap().stop();
            let _ = me.publish_off_status_if_disabled();
        })
        .await;
    }

    pub fn active(&self) -> Res<BootstrapMetadata> {
        let found = (|| -> Res<BootstrapMetadata> {
            match self.bootstrap.state()?.active {
                // ensure_enrolled() already validates any credential it needs; the brand-new
                // anonymous path just wrote one, so it is not re-read (avoids a second Keychain
                // prompt for the same enable attempt).
                None => self.bootstrap.ensure_enrolled(
                    &self.control_plane_origin,
                    self.remote_port,
                    self.transport.as_ref(),
                    &self.pacing,
                    None,
                ),
                Some(active) => {
                    self.bootstrap.credential(&active)?;
                    Ok(active)
                }
            }
        })();
        match found {
            Err(error @ RemoteError::PermanentlyRevoked(_)) => {
                // Durable, visible "needs reset" signal for status().
                atomic_write(&self.permanently_revoked_marker, "{}")?;
                Err(error)
            }
            Err(other) => Err(other),
            Ok(active) => {
                let _ = std::fs::remove_file(&self.permanently_revoked_marker);
                Ok(active)
            }
        }
    }

    fn control_for(&self, metadata: &BootstrapMetadata) -> Res<BetaRemoteControl> {
        let config = BetaControlConfig {
            control_plane_url: metadata.control_plane_origin.clone(),
            installation_id: metadata.installation_id.clone(),
            hostname: metadata.hostname.clone(),
            credential_file: None,
            state_dir: self.state_dir.clone(),
            installation_dir: self.installation_dir.clone(),
            remote_port: metadata.remote_port,
            lan_port: self.lan_port,
        };
        config.validate()?;
        let client = HttpClient::new(
            &config.control_plane_url,
            20.0,
            self.transport
                .clone()
                .unwrap_or_else(|| self.platform.http.clone()),
        );
        let (store, meta) = (self.bootstrap.clone(), metadata.clone());
        Ok(BetaRemoteControl::new(
            config,
            client,
            self.platform.locker.clone(),
            Some(Arc::new(move || store.credential(&meta))),
            self.pacing.clone(),
            self.device_name.as_deref(),
        ))
    }

    fn apply(&self, metadata: &BootstrapMetadata, action: &str) -> Res<Object> {
        let mut control = self.control_for(metadata)?;
        let mut result = control.apply(action)?;
        if let Some(token) = &control.connector_token {
            result.insert("tunnel_token".into(), json!(token));
        }
        Ok(result)
    }

    /// Always-on "I exist" signal, independent of the Remote Access toggle: enrolment (the
    /// existing anonymous flow) plus the authenticated GET status route. Never provisions.
    pub fn lightweight_checkin(&self) {
        let Ok(active) = self.bootstrap.ensure_enrolled(
            &self.control_plane_origin,
            self.remote_port,
            self.transport.as_ref(),
            &self.pacing,
            self.device_name.as_deref(),
        ) else {
            return;
        };
        if let Ok(mut control) = self.control_for(&active) {
            let _ = control.lightweight_status();
        }
    }

    fn capture_connector_token(&self, result: &mut Object) -> Res<()> {
        *self.connector_token.lock().unwrap() = None;
        match result.remove("tunnel_token") {
            Some(Value::String(t))
                if t.chars().count() >= 20 && !t.chars().any(char::is_whitespace) =>
            {
                *self.connector_token.lock().unwrap() = Some(t);
                Ok(())
            }
            _ => Err(RemoteError::provider(
                "Installation tunnel credential unavailable",
            )),
        }
    }

    pub fn connector_token(&self) -> Option<String> {
        self.connector_token.lock().unwrap().clone()
    }

    fn clear_connector_credential(&self) {
        *self.connector_token.lock().unwrap() = None;
        let _ = std::fs::remove_file(self.installation_dir.join("connector.token"));
        let _ = std::fs::remove_file(self.root.join("run/connector.token"));
    }

    fn retire_local(&self, metadata: &BootstrapMetadata, hard: bool) -> Res<()> {
        self.clear_connector_credential();
        if let Some(revoke) = &self.revoke_sessions {
            revoke();
        }
        self.bootstrap.finish_revoke(metadata, hard)?;
        let _ = std::fs::remove_file(self.state_dir.join("state.json"));
        Ok(())
    }

    pub fn metrics_port(&self) -> u16 {
        let remote_port = match self.bootstrap.state() {
            Ok(state) => state.active.map_or(self.remote_port, |a| a.remote_port),
            Err(_) => self.remote_port,
        };
        let candidate = if remote_port < 65535 {
            remote_port + 1
        } else {
            remote_port - 1
        };
        if candidate == self.lan_port || candidate == remote_port {
            18767
        } else {
            candidate
        }
    }

    fn publish(&self, values: Object) -> Res<()> {
        safe_status(&self.feature.status_path, values)
    }

    pub fn publish_connector_status(&self) -> Res<()> {
        let intent = self.feature.intent();
        let desired = read_desired(&self.desired_path);
        let mut state = "off".to_owned();
        let mut url: Option<String> = None;
        if intent.enabled {
            state = "enabling".to_owned();
            if desired.enabled {
                if let Some(current) = std::fs::read_to_string(&self.connector_status_path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                {
                    if current.get("generation").and_then(Value::as_str)
                        == Some(desired.generation.as_str())
                    {
                        if let Some(c @ ("connected" | "reconnecting")) =
                            current.get("state").and_then(Value::as_str)
                        {
                            state = c.to_owned();
                        }
                    }
                }
                if state == "connected" {
                    url = Some(desired.public_origin.clone());
                }
            }
        }
        self.publish(status_map!(
            "state" => state,
            "available" => self.binary_exists(),
            "checked_at" => self.platform.clock.wall(),
            "generation" => intent.generation,
            "url" => url,
            "temporary_url" => false,
        ))
    }

    pub fn publish_reconnecting_status(&self) -> Res<()> {
        let intent = self.feature.intent();
        self.publish(status_map!(
            "state" => if intent.enabled { "reconnecting" } else { "off" },
            "available" => self.binary_exists(),
            "checked_at" => self.platform.clock.wall(),
            "generation" => intent.generation,
            "url" => Value::Null,
            "temporary_url" => false,
        ))
    }

    pub fn publish_off_status(&self) -> Res<()> {
        self.publish(status_map!(
            "state" => "off",
            "available" => self.binary_exists(),
            "checked_at" => self.platform.clock.wall(),
            "generation" => self.feature.intent().generation,
            "url" => Value::Null,
            "temporary_url" => false,
        ))
    }

    fn publish_off_status_if_disabled(&self) -> Res<()> {
        if !self.feature.intent().enabled {
            self.publish_off_status()?;
        }
        Ok(())
    }
}

fn value_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
