//! B1: the OAuth-backed Planning Center client must follow token refreshes. rc.4 copied the access
//! token into the HTTP client once, so ~2 h after launch every request was a 401 until restart.
//!
//! Everything here goes through the real `PcoRuntime` wiring (settings -> factory -> client) against
//! an in-process fake Planning Center API and a fake control-plane token endpoint, with fake
//! credentials only.

mod support;

use serde_json::{json, Value};
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::settings::{
    MemoryCredentialStore, PersistentPlanningCenterSettings, PersistentSettings, SettingsService,
};
use stagepilot_pco::client::PcoApi;
use stagepilot_pco::errors::PcoError;
use stagepilot_pco::oauth::{system_now, OAuthTokenStore, OAuthTokens};
use stagepilot_pco::runtime::{PcoOptions, PcoRuntime};
use std::sync::{Arc, Mutex};
use support::*;

fn api_handler(accepted: &'static [&'static str]) -> impl Fn(&Req) -> Resp + Send + Sync {
    move |r| {
        let bearer = r.header("authorization").unwrap_or_default();
        if accepted
            .iter()
            .any(|token| bearer == format!("Bearer {token}"))
        {
            Resp::json(200, page(vec![service_type("42", "Weekend", 1)]))
        } else {
            Resp::json(401, json!({"errors": [{"status": "401"}]}))
        }
    }
}

/// A control plane whose `/refresh` hands out `next-access-N` tokens in order.
fn control_plane_handler(refreshes: Arc<Mutex<u32>>) -> impl Fn(&Req) -> Resp + Send + Sync {
    move |r| {
        if r.path.ends_with("/flow") {
            return Resp::json(201, json!({"flow_id": "aabbccdd", "ticket": "ticket-1"}));
        }
        let mut n = refreshes.lock().unwrap();
        *n += 1;
        let body: Value = json!({"access_token": format!("next-access-{n}"),
            "refresh_token": format!("next-refresh-{n}"), "expires_in": 7200,
            "scope": "services", "token_type": "Bearer"});
        Resp::json(200, body)
    }
}

struct Rig {
    runtime: Arc<PcoRuntime>,
    api: Mock,
    refreshes: Arc<Mutex<u32>>,
}

async fn rig(accepted: &'static [&'static str], stored_expires_in: f64) -> Rig {
    let api = Mock::start(api_handler(accepted)).await;
    let refreshes = Arc::new(Mutex::new(0u32));
    let control = Mock::start(control_plane_handler(refreshes.clone())).await;
    let settings = Arc::new(SettingsService::ephemeral_persistent(PersistentSettings {
        planning_center: PersistentPlanningCenterSettings {
            connection_method: ConnectionMethod::Oauth,
            ..serde_json::from_value(json!({})).unwrap()
        },
        ..PersistentSettings::default()
    }));
    let credentials = Arc::new(MemoryCredentialStore::default());
    let shared = credentials.clone();
    let runtime = PcoRuntime::new(
        settings,
        PcoOptions {
            client_id: Some("test-client-id".into()),
            memory_tokens: true,
            api_base: Some(api.url.clone()),
            control_plane_origin: Some(control.origin.clone()),
            oauth_credentials: Some(shared),
            ..PcoOptions::default()
        },
    );
    // Keep the control-plane mock alive for the whole test.
    std::mem::forget(control);
    OAuthTokenStore::new(credentials)
        .save(&OAuthTokens {
            access_token: "stored-access".into(),
            refresh_token: "stored-refresh".into(),
            expires_at: system_now() + stored_expires_in,
            scope: "services".into(),
            needs_reconnect: false,
        })
        .unwrap();
    Rig {
        runtime,
        api,
        refreshes,
    }
}

fn client(r: &Rig) -> Arc<dyn PcoApi> {
    (r.runtime.factory)(&r.runtime.effective()).expect("client builds")
}

#[tokio::test]
async fn a_refreshed_token_is_used_by_an_existing_client_without_reconfigure() {
    let r = rig(&["next-access-1"], 7200.0).await;
    let client = client(&r);
    // The proactive loop refreshes the stored token while the client lives on.
    r.runtime.oauth.refresh_now(None).await.unwrap();
    assert_eq!(*r.refreshes.lock().unwrap(), 1);
    let types = client
        .list_service_types()
        .await
        .expect("uses the new token");
    assert_eq!(types.len(), 1);
    let sent: Vec<_> = r
        .api
        .requests()
        .iter()
        .filter_map(|q| q.header("authorization").map(String::from))
        .collect();
    assert_eq!(sent, vec!["Bearer next-access-1".to_string()]);
}

#[tokio::test]
async fn a_401_refreshes_once_and_retries_once() {
    let r = rig(&["next-access-1"], 7200.0).await;
    let client = client(&r);
    let types = client.list_service_types().await.expect("retry succeeds");
    assert_eq!(types.len(), 1);
    let sent: Vec<_> = r
        .api
        .requests()
        .iter()
        .filter_map(|q| q.header("authorization").map(String::from))
        .collect();
    assert_eq!(
        sent,
        vec![
            "Bearer stored-access".to_string(),
            "Bearer next-access-1".to_string()
        ]
    );
    assert_eq!(*r.refreshes.lock().unwrap(), 1);
}

#[tokio::test]
async fn two_401s_mark_needs_reconnect_and_stop_calling_planning_center() {
    let r = rig(&[], 7200.0).await;
    let client = client(&r);
    let error = client.list_service_types().await.unwrap_err();
    assert!(matches!(error, PcoError::Authentication(_)), "{error:?}");
    assert_eq!(r.api.requests().len(), 2, "one request plus one retry");
    assert_eq!(*r.refreshes.lock().unwrap(), 1, "one refresh, no loop");
    assert!(r.runtime.oauth.stored().unwrap().unwrap().needs_reconnect);
    // Further calls fail locally: no new API traffic and no new refresh.
    let again = client.list_service_types().await.unwrap_err();
    assert!(matches!(again, PcoError::Authentication(_)), "{again:?}");
    assert_eq!(r.api.requests().len(), 2);
    assert_eq!(*r.refreshes.lock().unwrap(), 1);
}

#[tokio::test]
async fn manual_mode_does_not_refresh_on_401() {
    let api = Mock::start(api_handler(&[])).await;
    let client = stagepilot_pco::client::PcoClient::new(&pat_settings(), Some(api.url.clone()))
        .expect("client builds");
    let error = client.list_service_types().await.unwrap_err();
    assert!(matches!(error, PcoError::Authentication(_)));
    assert_eq!(api.requests().len(), 1);
}
