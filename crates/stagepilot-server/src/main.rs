//! `stagepilot-server`: run the Rust backend headless.
//!
//!   stagepilot-server                 saved settings, keychain, dashboard PIN enforced
//!   stagepilot-server --demo --port N in-memory demo settings, PIN not enforced (contract runs)
//!
//!   stagepilot-server --contract-outputs --propresenter-port N --port N
//!       the "outputs" contract profile: demo plan, fake MIDI devices, ProPresenter at 127.0.0.1:N
//!       (a mock), OSC to a loopback port nothing listens on. Mirrors `scripts/contract/outputs_app.py`.
//!
//! The bind address comes from the settings (`STAGEPILOT_HOST` / `STAGEPILOT_PORT` override them);
//! `--host` and `--port` override both.

use stagepilot_core::config::{
    IntegrationModes, LightingMethod, LightsSettings, MidiSettings, MidiSource,
    ProPresenterSettings, ServiceSource, TimerOutput,
};
use stagepilot_core::settings::{PersistentSettings, RuntimeSettings, SettingsService};
use stagepilot_server::{build_app, default_web_root, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// The "outputs" contract profile (see `scripts/contract/outputs_app.py`).
fn contract_outputs_settings(propresenter_port: u16) -> PersistentSettings {
    PersistentSettings {
        integration_modes: IntegrationModes {
            service_source: ServiceSource::Demo,
            midi_source: MidiSource::Real,
            timer_output: TimerOutput::Propresenter,
        },
        midi: MidiSettings {
            enabled: true,
            input_name: Some("StagePilot Test In".into()),
            ..MidiSettings::default()
        },
        propresenter: ProPresenterSettings {
            enabled: true,
            host: "127.0.0.1".into(),
            port: propresenter_port,
            health_check_interval_seconds: 300.0,
            reconnect_initial_seconds: 0.5,
            reconnect_max_seconds: 1.0,
            ..ProPresenterSettings::default()
        },
        lights: LightsSettings {
            enabled: true,
            method: LightingMethod::Osc,
            osc_host: "127.0.0.1".into(),
            osc_port: 47999,
            ..LightsSettings::default()
        },
        ..PersistentSettings::default()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().collect();
    let contract_outputs = args.iter().any(|a| a == "--contract-outputs");
    let demo = contract_outputs || args.iter().any(|a| a == "--demo");
    let service = if contract_outputs {
        let port = arg(&args, "--propresenter-port")
            .and_then(|p| p.parse::<u16>().ok())
            .ok_or("--contract-outputs needs --propresenter-port")?;
        SettingsService::ephemeral_persistent(contract_outputs_settings(port))
    } else if demo {
        SettingsService::ephemeral(RuntimeSettings::demo())
    } else {
        let service = SettingsService::default_service();
        service.load();
        service
    };
    let backends = if contract_outputs {
        OutputBackends::fake_midi()
    } else {
        OutputBackends::default()
    };
    let runtime = Runtime::with_backends(Arc::new(service), !demo, backends);
    let host = arg(&args, "--host").unwrap_or_else(|| runtime.settings.bind_host.clone());
    let port = arg(&args, "--port")
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(runtime.settings.bind_port);
    let address: SocketAddr = format!("{host}:{port}").parse()?;

    runtime.start().await;
    let app = build_app(
        runtime.clone(),
        AppOptions {
            web_root: default_web_root(),
        },
    );
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "stagepilot_server_listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    runtime.stop().await;
    Ok(())
}
