//! The Planning Center runtime the server owns: OAuth service, setup service, client factory,
//! plan cache and the (optional) running plugin. Counterpart of the Planning Center parts of
//! `main.create_app` and `core/runtime.py`.

use crate::client::{factory_with_tokens, PcoClientFactory, TokenSource};
use crate::errors::PcoError;
use crate::models::PcoSettings;
use crate::oauth::{ControlPlaneClient, OAuthService, OAuthTokenStore};
use crate::plan_cache::{FilePlanCacheStore, MemoryPlanCacheStore, PlanCacheStore};
use crate::plugin::{PcoPlugin, TodayProvider};
use crate::setup::{current_settings, SetupService};
use stagepilot_core::bus::EventBus;
use stagepilot_core::settings::{
    CredentialStore, KeyringCredentialStore, MemoryCredentialStore, SettingsService,
};
use stagepilot_core::store::StateStore;
use std::sync::{Arc, Mutex};
use url::Url;

/// How the Planning Center side is wired. `Default` is the production wiring: keyring-backed
/// tokens, the real control plane, the real API, the real clock.
#[derive(Clone, Default)]
pub struct PcoOptions {
    /// `STAGEPILOT_PCO_CLIENT_ID` (not a secret; a product-wide registration value).
    pub client_id: Option<String>,
    /// In-memory OAuth tokens instead of the OS keyring (demo / ephemeral instances).
    pub memory_tokens: bool,
    /// Override the API origin (tests and the mocked contract profile only).
    pub api_base: Option<Url>,
    /// Override the control-plane origin (tests and the mocked contract profile only).
    pub control_plane_origin: Option<String>,
    pub today: Option<TodayProvider>,
    pub plan_cache: Option<Arc<dyn PlanCacheStore>>,
    pub oauth_credentials: Option<Arc<dyn CredentialStore>>,
    pub factory: Option<PcoClientFactory>,
}

pub struct PcoRuntime {
    pub oauth: Arc<OAuthService>,
    pub setup: SetupService,
    pub factory: PcoClientFactory,
    pub cache: Arc<dyn PlanCacheStore>,
    pub today: Option<TodayProvider>,
    pub settings: Arc<SettingsService>,
    /// Serialises connection-changing routes with the live source switch (`reconcile_lock`).
    pub lock: tokio::sync::Mutex<()>,
    plugin: Mutex<Option<PcoPlugin>>,
}

impl PcoRuntime {
    pub fn new(settings: Arc<SettingsService>, options: PcoOptions) -> Arc<Self> {
        let client_id = options
            .client_id
            .or_else(|| settings.environ().get("STAGEPILOT_PCO_CLIENT_ID").cloned())
            .or_else(|| std::env::var("STAGEPILOT_PCO_CLIENT_ID").ok())
            .unwrap_or_default();
        let credentials: Arc<dyn CredentialStore> =
            options.oauth_credentials.unwrap_or_else(|| {
                if options.memory_tokens {
                    Arc::new(MemoryCredentialStore::default())
                } else {
                    Arc::new(KeyringCredentialStore::oauth())
                }
            });
        let control_plane = match &options.control_plane_origin {
            Some(origin) => ControlPlaneClient::for_test_origin(origin),
            None => ControlPlaneClient::new(),
        };
        // A control plane that cannot be built (never in practice) is replaced by one that fails
        // every call as "transient", so the app still starts.
        let control_plane = control_plane.unwrap_or_else(|_| {
            ControlPlaneClient::for_test_origin("http://127.0.0.1:9")
                .unwrap_or_else(|_| std::process::abort())
        });
        let oauth = Arc::new(OAuthService::new(
            &client_id,
            OAuthTokenStore::new(credentials),
            control_plane,
        ));
        let factory = options.factory.unwrap_or_else(|| {
            factory_with_tokens(
                options.api_base.clone(),
                Some(oauth.clone() as Arc<dyn TokenSource>),
            )
        });
        let cache = options.plan_cache.unwrap_or_else(|| {
            if options.memory_tokens {
                Arc::new(MemoryPlanCacheStore::default())
            } else {
                Arc::new(FilePlanCacheStore::new(
                    crate::plan_cache::default_plan_cache_path(settings.environ()),
                ))
            }
        });
        let setup = SetupService::new(settings.clone(), factory.clone(), Some(oauth.clone()));
        Arc::new(Self {
            oauth,
            setup,
            factory,
            cache,
            today: options.today,
            settings,
            lock: tokio::sync::Mutex::new(()),
            plugin: Mutex::new(None),
        })
    }

    /// The effective Planning Center settings, with the stored OAuth access token.
    pub fn effective(&self) -> PcoSettings {
        current_settings(&self.settings, Some(&self.oauth))
    }

    pub fn build_plugin(&self, bus: EventBus, store: StateStore) -> Result<PcoPlugin, PcoError> {
        let timezone = self.settings.effective_runtime_settings().timezone;
        PcoPlugin::new(
            bus,
            store,
            self.effective(),
            &timezone,
            self.factory.clone(),
            self.today.clone(),
            Some(self.cache.clone()),
        )
    }

    /// The running plugin, if the Services source is Planning Center and it started.
    pub fn plugin(&self) -> Option<PcoPlugin> {
        self.plugin
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_plugin(&self, plugin: Option<PcoPlugin>) {
        *self.plugin.lock().unwrap_or_else(|p| p.into_inner()) = plugin;
    }
}
