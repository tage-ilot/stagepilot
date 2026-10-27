//! Shared Remote types: the worker-limited store handle (`RemoteAccess`), the per-request
//! context the trusted ingress attaches, the wiring handed to the router, and the traits the
//! OS-facing Remote modules (manager, bootstrap store, credential broker,
//! `cloudflared`, in `stagepilot-remote`) implement. Python counterparts: `api/remote_ingress.py` (`RemoteAccess`,
//! `RemoteContext`), `api/remote_feature.py` (the `manager`/`feature` lookups) and
//! `api/diagnostics.py` (`DesktopBootstrapStore`).

use crate::remote_feature::RemoteFeature;
use crate::runtime::Runtime;
use async_trait::async_trait;
use serde_json::Value;
use stagepilot_core::auth::{RemoteAuthError, RemoteSession, RemoteStore};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub const COOKIE_NAME: &str = "__Host-stagepilot_remote";
pub const AUTH_PREFIX: &str = "/api/v1/remote-auth";
pub const ACCESS_PATH: &str = "/api/v1/access";

/// `RemoteAccess`: the store plus its own worker limit (two), separate from the shared pool so
/// remote password/database work cannot starve the production integrations.
pub struct RemoteAccess {
    pub store: Arc<RemoteStore>,
    limiter: Arc<Semaphore>,
}

impl RemoteAccess {
    pub fn new(store: RemoteStore) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(store),
            limiter: Arc::new(Semaphore::new(2)),
        })
    }

    /// `RemoteAccess.call`: run a blocking store operation on the limited worker pool.
    pub async fn call<T, F>(&self, work: F) -> Result<T, RemoteAuthError>
    where
        T: Send + 'static,
        F: FnOnce(&RemoteStore) -> Result<T, RemoteAuthError> + Send + 'static,
    {
        let store = self.store.clone();
        self.run(move || work(&store)).await
    }

    /// The same limit for blocking work that is not a store call (the file-based feature).
    pub async fn run<T, F>(&self, work: F) -> Result<T, RemoteAuthError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, RemoteAuthError> + Send + 'static,
    {
        let unavailable = || RemoteAuthError {
            status: 503,
            detail: "Remote access storage is unavailable.".to_string(),
        };
        let permit = self
            .limiter
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| unavailable())?;
        let result = tokio::task::spawn_blocking(work).await;
        drop(permit);
        result.map_err(|_| unavailable())?
    }
}

/// `RemoteContext`: set on a request only by the trusted ingress, after remote authorisation.
#[derive(Clone)]
pub struct RemoteContext {
    pub access: Arc<RemoteAccess>,
    pub token: Option<String>,
    pub session: Option<RemoteSession>,
}

/// Why a `RemoteManager` operation failed (the Python exception classes the routes map).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerError {
    /// `ProviderError` / `InstallationPermanentlyRevokedError`: sanitised text, no secrets.
    Provider(String),
    /// `ValueError`: not provisioned, or already managed elsewhere.
    Value(String),
}

/// `DesktopRemoteManager` as the routes use it. `stagepilot-remote` implements this; tests use
/// `remote_fakes::FakeManager`. `reset_identity` and `regenerate` are optional in Python
/// (`callable(getattr(...))`), hence the `supports_*` switches.
#[async_trait]
pub trait RemoteManager: Send + Sync {
    /// `status()`: the product status object (a JSON object).
    async fn status(&self) -> Value;
    async fn enable(&self) -> Result<(), ManagerError>;
    async fn disable(&self) -> Result<(), ManagerError>;
    fn supports_reset(&self) -> bool {
        true
    }
    async fn reset_identity(&self) -> Result<(), ManagerError> {
        Ok(())
    }
    fn supports_regenerate(&self) -> bool {
        true
    }
    async fn regenerate(&self) -> Result<(), ManagerError> {
        Ok(())
    }
    /// `manager.bootstrap`: the enrolled identity the diagnostics upload reuses.
    fn bootstrap(&self) -> Option<Arc<dyn DiagnosticsBootstrap>> {
        None
    }
}

/// The active enrolment (`BootstrapMetadata`): where to send the bundle and as whom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveEnrollment {
    pub control_plane_origin: String,
    pub installation_id: String,
}

/// `DesktopBootstrapStore` as the diagnostics route uses it.
pub trait DiagnosticsBootstrap: Send + Sync {
    fn trusted_origins(&self) -> Vec<String>;
    /// `state().active`. `Err` is any failure reading the state (Python catches `Exception`).
    fn active(&self) -> Result<Option<ActiveEnrollment>, String>;
    /// `credential(active)`: the bearer credential. `Err` ends as a 502.
    fn credential(&self, active: &ActiveEnrollment) -> Result<String, String>;
}

/// What `create_app` stores on `app.state` for Remote: `remote_access`, `remote_feature` and
/// `remote_manager`. Everything is optional, as in Python.
#[derive(Clone, Default)]
pub struct RemoteWiring {
    pub access: Option<Arc<RemoteAccess>>,
    pub feature: Option<Arc<RemoteFeature>>,
    pub manager: Option<Arc<dyn RemoteManager>>,
}

/// The state the Remote routes share.
#[derive(Clone)]
pub struct RemoteState {
    pub rt: Arc<Runtime>,
    pub wiring: RemoteWiring,
}
