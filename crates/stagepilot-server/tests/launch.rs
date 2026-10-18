//! The real Remote wiring in `launch` (lifespan order, status, diagnostics identity, shutdown).
//! Uses a temp root and a `cloudflared` path that does not exist, so no process is ever started
//! and no network is touched (Remote stays disabled).

use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_remote::platform::Platform;
use stagepilot_server::launch::{attach_with, desktop_remote_from_env};
use stagepilot_server::{build_app, AppOptions, ManagerError, Runtime};
use std::sync::Arc;
use std::time::Duration;

fn runtime() -> Arc<Runtime> {
    Runtime::new(
        Arc::new(SettingsService::ephemeral(RuntimeSettings::demo())),
        false,
    )
}

#[tokio::test]
async fn real_manager_is_wired_and_shut_down_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime();
    rt.start().await;
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let binary = dir.path().join("cloudflared");
    let lifecycle = attach_with(&rt, app, dir.path(), &binary, Platform::system(), None)
        .await
        .unwrap()
        .expect("attached");

    let wiring = rt.remote_wiring();
    assert!(wiring.access.is_some());
    assert!(wiring.feature.is_some());
    let manager = wiring.manager.expect("manager");
    // Not enrolled: unprovisioned, and the connector binary is missing so Remote is unavailable.
    let status = manager.status().await;
    assert_eq!(status["provisioned"], false);
    assert_eq!(status["available"], false);
    assert_eq!(status["enabled"], false);
    // The diagnostics route reads the same bootstrap store: nothing enrolled yet.
    let bootstrap = manager.bootstrap().expect("bootstrap");
    assert_eq!(bootstrap.active(), Ok(None));
    assert!(bootstrap
        .trusted_origins()
        .iter()
        .all(|o| o.starts_with("https://")));
    // Disabling while not provisioned is a no-op success (no network).
    assert_eq!(manager.disable().await, Ok(()));
    // Resetting with nothing retired succeeds.
    assert_eq!(manager.reset_identity().await, Ok(()));

    tokio::time::timeout(Duration::from_secs(20), lifecycle.stop())
        .await
        .expect("supervisor and reconcile task stop promptly");
    rt.stop().await;
}

#[tokio::test]
async fn a_relative_root_is_refused_and_nothing_is_left_attached() {
    let rt = runtime();
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let result = attach_with(
        &rt,
        app,
        std::path::Path::new("relative"),
        std::path::Path::new("/nonexistent/cloudflared"),
        Platform::system(),
        None,
    )
    .await;
    assert!(result.is_err());
    assert!(rt.remote_wiring().manager.is_none());
}

#[tokio::test]
async fn enable_without_a_reachable_control_plane_is_a_provider_error() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime();
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let options = stagepilot_remote::desktop::DesktopOptions {
        // A closed loopback port: the connection is refused, never reaching the real service.
        control_plane_origin: "http://127.0.0.1:9".into(),
        pacing: stagepilot_remote::bootstrap::Pacing::instant(),
        ..Default::default()
    };
    let lifecycle = attach_with(
        &rt,
        app,
        dir.path(),
        &dir.path().join("cloudflared"),
        Platform::system(),
        Some(options),
    )
    .await
    .unwrap()
    .unwrap();
    let manager = rt.remote_wiring().manager.unwrap();
    // The origin is not a trusted control plane, so enrolment is refused before any request.
    assert!(matches!(
        manager.enable().await,
        Err(ManagerError::Provider(_))
    ));
    lifecycle.stop().await;
}

#[test]
fn environment_needs_both_variables() {
    // Neither variable is set in the test environment.
    if std::env::var("STAGEPILOT_DESKTOP_REMOTE_ROOT").is_err()
        || std::env::var("STAGEPILOT_CLOUDFLARED_BINARY").is_err()
    {
        assert!(desktop_remote_from_env().is_none());
    }
}

mod activation {
    use async_trait::async_trait;
    use stagepilot_server::startup_activation::{run, ActivationTargets};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<&'static str>>,
        presentation: Option<bool>,
        lights: Option<bool>,
        pco: Option<bool>,
    }

    impl Fake {
        fn log(&self, name: &'static str) {
            self.calls.lock().unwrap().push(name);
        }
        fn calls(&self) -> Vec<&'static str> {
            let mut calls = self.calls.lock().unwrap().clone();
            calls.sort();
            calls
        }
    }

    #[async_trait]
    impl ActivationTargets for Fake {
        async fn restart_failed(&self) {
            self.log("restart_failed");
        }
        async fn refresh_midi(&self) {
            self.log("midi_refresh");
        }
        async fn presentation_ready(&self) -> Option<bool> {
            self.presentation
        }
        async fn reconfigure_presentation(&self) {
            self.log("presentation");
        }
        async fn lights_connected(&self) -> Option<bool> {
            self.lights
        }
        async fn reconfigure_lights(&self) {
            self.log("lights");
        }
        async fn planning_center_connected(&self) -> Option<bool> {
            self.pco
        }
        async fn refresh_planning_center(&self) {
            self.log("pco");
        }
    }

    // test_startup_activation_retries_and_connects_saved_integrations
    #[tokio::test]
    async fn retries_and_connects_saved_integrations() {
        let fake = Fake {
            presentation: Some(false),
            lights: Some(false),
            pco: Some(false),
            ..Fake::default()
        };
        run(&fake).await;
        assert_eq!(
            fake.calls(),
            vec![
                "lights",
                "midi_refresh",
                "pco",
                "presentation",
                "restart_failed"
            ]
        );
    }

    // test_startup_activation_does_not_disrupt_ready_integrations
    #[tokio::test]
    async fn does_not_disrupt_ready_integrations() {
        let fake = Fake {
            presentation: Some(true),
            lights: Some(true),
            pco: Some(true),
            ..Fake::default()
        };
        run(&fake).await;
        assert_eq!(fake.calls(), vec!["midi_refresh", "restart_failed"]);
    }

    // Absent or disabled integrations are skipped (Python's `is None` / `enabled` guards).
    #[tokio::test]
    async fn absent_integrations_are_skipped() {
        let fake = Fake::default();
        run(&fake).await;
        assert_eq!(fake.calls(), vec!["midi_refresh", "restart_failed"]);
    }
}
