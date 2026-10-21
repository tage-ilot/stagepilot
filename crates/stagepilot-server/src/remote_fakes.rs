//! Test doubles for the Remote traits in [`crate::remote_types`]. The real manager and bootstrap
//! store are phase 6b-B; the routes only see the traits, so these fakes are all the route tests
//! and the contract profile need. They hold no secrets and touch no files or network.

use crate::remote_types::{ActiveEnrollment, DiagnosticsBootstrap, ManagerError, RemoteManager};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// Scripted `DesktopBootstrapStore`.
pub struct FakeBootstrap {
    pub trusted: Vec<String>,
    pub active: Option<ActiveEnrollment>,
    pub state_fails: bool,
    pub credential: Result<String, String>,
}

impl FakeBootstrap {
    /// Enrolled against `origin` (also trusted), installation id `abcd1234`.
    pub fn enrolled(origin: &str) -> Self {
        Self {
            trusted: vec![origin.to_string()],
            active: Some(ActiveEnrollment {
                control_plane_origin: origin.to_string(),
                installation_id: "abcd1234".to_string(),
            }),
            state_fails: false,
            credential: Ok(format!("spi_abcd1234.{}", "s".repeat(43))),
        }
    }
}

impl DiagnosticsBootstrap for FakeBootstrap {
    fn trusted_origins(&self) -> Vec<String> {
        self.trusted.clone()
    }
    fn active(&self) -> Result<Option<ActiveEnrollment>, String> {
        if self.state_fails {
            Err("boom".into())
        } else {
            Ok(self.active.clone())
        }
    }
    fn credential(&self, _active: &ActiveEnrollment) -> Result<String, String> {
        self.credential.clone()
    }
}

/// Scripted `DesktopRemoteManager`: records the calls it receives.
pub struct FakeManager {
    pub status: Mutex<Value>,
    pub enable_result: Mutex<Result<(), ManagerError>>,
    pub disable_result: Mutex<Result<(), ManagerError>>,
    pub reset_result: Mutex<Result<(), ManagerError>>,
    pub regenerate_result: Mutex<Result<(), ManagerError>>,
    pub reset_supported: bool,
    pub regenerate_supported: bool,
    pub bootstrap: Option<Arc<dyn DiagnosticsBootstrap>>,
    pub calls: Mutex<Vec<String>>,
}

impl Default for FakeManager {
    fn default() -> Self {
        Self {
            status: Mutex::new(json!({
                "available": true,
                "enabled": false,
                "state": "off",
                "url": null,
                "message": null,
                "temporary_url": false,
            })),
            enable_result: Mutex::new(Ok(())),
            disable_result: Mutex::new(Ok(())),
            reset_result: Mutex::new(Ok(())),
            regenerate_result: Mutex::new(Ok(())),
            reset_supported: true,
            regenerate_supported: true,
            bootstrap: None,
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl FakeManager {
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn record(&self, name: &str) {
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(name.to_string());
    }

    fn result(slot: &Mutex<Result<(), ManagerError>>) -> Result<(), ManagerError> {
        slot.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

#[async_trait]
impl RemoteManager for FakeManager {
    async fn status(&self) -> Value {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    async fn enable(&self) -> Result<(), ManagerError> {
        self.record("enable");
        Self::result(&self.enable_result)
    }
    async fn disable(&self) -> Result<(), ManagerError> {
        self.record("disable");
        Self::result(&self.disable_result)
    }
    fn supports_reset(&self) -> bool {
        self.reset_supported
    }
    async fn reset_identity(&self) -> Result<(), ManagerError> {
        self.record("reset_identity");
        Self::result(&self.reset_result)
    }
    fn supports_regenerate(&self) -> bool {
        self.regenerate_supported
    }
    async fn regenerate(&self) -> Result<(), ManagerError> {
        self.record("regenerate");
        Self::result(&self.regenerate_result)
    }
    fn bootstrap(&self) -> Option<Arc<dyn DiagnosticsBootstrap>> {
        self.bootstrap.clone()
    }
}

/// The scripted manager of the contract "diagnostics" profile (mirrors
/// `scripts/contract/remote_app.py::ContractManager`): stateful enable/disable, an identity that
/// reset un-enrols, `regenerate` failing while disabled.
pub struct ContractManager {
    enabled: Mutex<bool>,
    bootstrap: Arc<ContractBootstrap>,
}

pub struct ContractBootstrap {
    origin: String,
    enrolled: Mutex<bool>,
}

impl ContractManager {
    pub fn new(control_plane_origin: &str) -> Self {
        Self {
            enabled: Mutex::new(false),
            bootstrap: Arc::new(ContractBootstrap {
                origin: control_plane_origin.to_string(),
                enrolled: Mutex::new(true),
            }),
        }
    }
}

impl DiagnosticsBootstrap for ContractBootstrap {
    fn trusted_origins(&self) -> Vec<String> {
        vec![self.origin.clone()]
    }
    fn active(&self) -> Result<Option<ActiveEnrollment>, String> {
        let enrolled = *self.enrolled.lock().unwrap_or_else(|p| p.into_inner());
        Ok(enrolled.then(|| ActiveEnrollment {
            control_plane_origin: self.origin.clone(),
            installation_id: "abcd1234".to_string(),
        }))
    }
    fn credential(&self, _active: &ActiveEnrollment) -> Result<String, String> {
        Ok(format!("spi_abcd1234.{}", "s".repeat(43)))
    }
}

#[async_trait]
impl RemoteManager for ContractManager {
    async fn status(&self) -> Value {
        let enabled = *self.enabled.lock().unwrap_or_else(|p| p.into_inner());
        json!({
            "available": true,
            "enabled": enabled,
            "state": if enabled { "connected" } else { "off" },
            "url": if enabled { json!("https://contract.stagepilot.example") } else { Value::Null },
            "message": null,
            "temporary_url": false,
        })
    }
    async fn enable(&self) -> Result<(), ManagerError> {
        let mut enabled = self.enabled.lock().unwrap_or_else(|p| p.into_inner());
        if *enabled {
            return Err(ManagerError::Value("already enabled".into()));
        }
        *enabled = true;
        Ok(())
    }
    async fn disable(&self) -> Result<(), ManagerError> {
        *self.enabled.lock().unwrap_or_else(|p| p.into_inner()) = false;
        Ok(())
    }
    async fn reset_identity(&self) -> Result<(), ManagerError> {
        *self
            .bootstrap
            .enrolled
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = false;
        Ok(())
    }
    async fn regenerate(&self) -> Result<(), ManagerError> {
        if *self.enabled.lock().unwrap_or_else(|p| p.into_inner()) {
            Ok(())
        } else {
            Err(ManagerError::Provider("Could not reach the service".into()))
        }
    }
    fn bootstrap(&self) -> Option<Arc<dyn DiagnosticsBootstrap>> {
        Some(self.bootstrap.clone())
    }
}
