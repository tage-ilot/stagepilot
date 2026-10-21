//! Planning Center onboarding operations available before plugin startup: port of
//! `services/planning_center_setup.py`.

use crate::client::{PcoApi, PcoClientFactory};
use crate::errors::PcoError;
use crate::models::{PcoSettings, Secret, ServiceType};
use crate::oauth::OAuthService;
use crate::settings as pco_settings;
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::settings::SettingsService;
use std::sync::Arc;

/// Test credentials and discover service types without starting the plugin.
pub struct SetupService {
    settings: Arc<SettingsService>,
    factory: PcoClientFactory,
    oauth: Option<Arc<OAuthService>>,
}

impl SetupService {
    pub fn new(
        settings: Arc<SettingsService>,
        factory: PcoClientFactory,
        oauth: Option<Arc<OAuthService>>,
    ) -> Self {
        Self {
            settings,
            factory,
            oauth,
        }
    }

    /// The effective settings with the stored OAuth access token (when OAuth is selected).
    pub fn current_settings(&self) -> PcoSettings {
        current_settings(&self.settings, self.oauth.as_deref())
    }

    /// Authenticate and return active service types using temporary credential overrides.
    pub async fn test_connection(
        &self,
        app_id: Option<&str>,
        secret: Option<&str>,
    ) -> Result<Vec<ServiceType>, PcoError> {
        let mut settings = self.current_settings();
        if let Some(app_id) = app_id {
            settings.app_id = Some(Secret::new(app_id));
        }
        if let Some(secret) = secret {
            settings.secret = Some(Secret::new(secret));
        }
        self.list_service_types_with(settings).await
    }

    /// Active service types using the effective saved configuration. An OAuth access token cached
    /// earlier can have expired: refresh first (a no-op while it is still valid) so this request
    /// never fires with a stale Bearer token when a valid refresh token could have avoided a 401.
    pub async fn list_service_types(&self) -> Result<Vec<ServiceType>, PcoError> {
        if self.settings.snapshot().planning_center.connection_method == ConnectionMethod::Oauth {
            if let Some(oauth) = &self.oauth {
                // Failures are ignored on purpose: the request below reports what matters.
                let _ = oauth.valid_access_token().await;
            }
        }
        self.list_service_types_with(self.current_settings()).await
    }

    async fn list_service_types_with(
        &self,
        settings: PcoSettings,
    ) -> Result<Vec<ServiceType>, PcoError> {
        let client: Arc<dyn PcoApi> = match (self.factory)(&settings) {
            Ok(client) => client,
            Err(error) if error.is_planning_center_error() => return Err(error),
            Err(_) => {
                return Err(PcoError::Generic(
                    "Planning Center connection setup failed unexpectedly.".into(),
                ))
            }
        };
        let result = client.list_service_types().await;
        client.close().await;
        match result {
            Ok(types) => Ok(types.into_iter().filter(|t| !t.archived).collect()),
            Err(error) if error.is_planning_center_error() => Err(error),
            Err(_) => Err(PcoError::Generic(
                "Planning Center service-type discovery failed unexpectedly.".into(),
            )),
        }
    }
}

/// Effective settings plus the stored OAuth access token (never the refresh token).
pub fn current_settings(settings: &SettingsService, oauth: Option<&OAuthService>) -> PcoSettings {
    // The keychain is only read when OAuth is the selected connection method.
    let token = (settings.snapshot().planning_center.connection_method == ConnectionMethod::Oauth)
        .then(|| oauth.and_then(|o| o.stored().ok().flatten()))
        .flatten()
        .filter(|t| !t.needs_reconnect)
        .map(|t| t.access_token);
    pco_settings::effective(settings, token)
}
