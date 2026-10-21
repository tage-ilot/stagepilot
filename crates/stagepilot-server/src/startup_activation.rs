//! `services/startup_activation.py`: once desktop startup has settled (macOS and vendor apps need
//! a moment), retry plugins that failed their first start, then reconcile the saved integrations
//! concurrently. Each integration sits behind a small trait so the decision logic is testable with
//! fakes; [`RuntimeActivation`] connects them to the real plugins.

use crate::runtime::Runtime;
use async_trait::async_trait;
use stagepilot_core::state::ConnectionStatus;
use std::sync::Arc;
use std::time::Duration;

/// Seconds `_run_startup_activation` waits before running.
pub const SETTLE_SECONDS: u64 = 1;

#[async_trait]
pub trait ActivationTargets: Send + Sync {
    async fn restart_failed(&self);
    /// `midi.input_snapshot(refresh=True)`; `false` when there is no MIDI plugin.
    async fn refresh_midi(&self);
    /// `None` when there is no Presentation plugin; otherwise whether it is connected with its
    /// timer found.
    async fn presentation_ready(&self) -> Option<bool>;
    async fn reconfigure_presentation(&self);
    /// `None` when lights are off or absent; otherwise whether the output is connected.
    async fn lights_connected(&self) -> Option<bool>;
    async fn reconfigure_lights(&self);
    /// `None` when there is no Planning Center plugin; otherwise whether it is connected.
    async fn planning_center_connected(&self) -> Option<bool>;
    async fn refresh_planning_center(&self);
}

/// `StartupActivationService.run`.
pub async fn run(targets: &dyn ActivationTargets) {
    targets.restart_failed().await;
    let midi = targets.refresh_midi();
    let presentation = async {
        if targets.presentation_ready().await == Some(false) {
            targets.reconfigure_presentation().await;
        }
    };
    let lights = async {
        if targets.lights_connected().await == Some(false) {
            targets.reconfigure_lights().await;
        }
    };
    let pco = async {
        if targets.planning_center_connected().await == Some(false) {
            targets.refresh_planning_center().await;
        }
    };
    tokio::join!(midi, presentation, lights, pco);
}

/// `_run_startup_activation`: settle, then run.
pub async fn run_after_settle(targets: Arc<dyn ActivationTargets>) {
    tokio::time::sleep(Duration::from_secs(SETTLE_SECONDS)).await;
    run(targets.as_ref()).await;
}

/// The real plugins behind the traits.
pub struct RuntimeActivation(pub std::sync::Weak<Runtime>);

impl RuntimeActivation {
    fn rt(&self) -> Option<Arc<Runtime>> {
        self.0.upgrade()
    }
}

#[async_trait]
impl ActivationTargets for RuntimeActivation {
    async fn restart_failed(&self) {
        if let Some(rt) = self.rt() {
            rt.plugins.restart_failed().await;
        }
    }

    async fn refresh_midi(&self) {
        if let Some(midi) = self.rt().and_then(|rt| rt.outputs.midi()) {
            let _ = midi.input_snapshot(true).await;
        }
    }

    async fn presentation_ready(&self) -> Option<bool> {
        let plugin = self.rt()?.outputs.propresenter()?;
        let snapshot = plugin.snapshot(false).await;
        Some(snapshot.connection_status == ConnectionStatus::Connected && snapshot.timer_found)
    }

    async fn reconfigure_presentation(&self) {
        if let Some(rt) = self.rt() {
            if let Some(plugin) = rt.outputs.propresenter() {
                let settings = rt
                    .settings_service
                    .effective_runtime_settings()
                    .propresenter;
                plugin.reconfigure(settings).await;
            }
        }
    }

    async fn lights_connected(&self) -> Option<bool> {
        let rt = self.rt()?;
        if !rt
            .settings_service
            .effective_runtime_settings()
            .lights
            .enabled
        {
            return None;
        }
        Some(
            rt.outputs.lights.snapshot(false).await.connection_status
                == ConnectionStatus::Connected,
        )
    }

    async fn reconfigure_lights(&self) {
        if let Some(rt) = self.rt() {
            let settings = rt.settings_service.effective_runtime_settings().lights;
            rt.outputs.lights.reconfigure(settings).await;
        }
    }

    async fn planning_center_connected(&self) -> Option<bool> {
        let rt = self.rt()?;
        rt.pco.plugin()?;
        Some(rt.store.snapshot().planning_center_status == ConnectionStatus::Connected)
    }

    async fn refresh_planning_center(&self) {
        if let Some(plugin) = self.rt().and_then(|rt| rt.pco.plugin()) {
            plugin.refresh_now().await;
        }
    }
}
