//! `stagepilot-server`: run the Rust backend headless.
//!
//!   stagepilot-server                 saved settings, keychain, dashboard PIN enforced
//!   stagepilot-server --demo --port N in-memory demo settings, PIN not enforced (contract runs)
//!   stagepilot-server --contract-pco --pco-port N --port N
//!       the "pco" contract profile: Planning Center is the Services source and talks to a MOCK API
//!       and control plane on 127.0.0.1:N (never the real Planning Center). Mirrors
//!       `scripts/contract/pco_app.py`.
//!
//! The bind address comes from the settings (`STAGEPILOT_HOST` / `STAGEPILOT_PORT` override them);
//! `--host` and `--port` override both.

use stagepilot_core::config::{IntegrationModes, MidiSource, ServiceSource, TimerOutput};
use stagepilot_core::settings::{
    PersistentPlanningCenterSettings, PersistentSettings, RuntimeSettings, SettingsService,
};
use stagepilot_pco::runtime::PcoOptions;
use stagepilot_server::{build_app, default_web_root, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;

/// The "pco" contract profile (see `scripts/contract/pco_app.py`): Planning Center is the Services
/// source, MIDI and the timer output are simulated, nothing is saved.
fn contract_pco_settings() -> PersistentSettings {
    PersistentSettings {
        integration_modes: IntegrationModes {
            service_source: ServiceSource::PlanningCenter,
            midi_source: MidiSource::Simulated,
            timer_output: TimerOutput::Simulated,
        },
        // A personal access token for one service type, so the plugin connects at boot.
        planning_center: PersistentPlanningCenterSettings {
            app_id: Some("app-1".to_string()),
            service_type_id: Some("111".to_string()),
            request_timeout_seconds: 5.0,
            ..PersistentPlanningCenterSettings::default()
        },
        ..PersistentSettings::default()
    }
}

/// Planning Center talks to the MOCK on 127.0.0.1:`port` (API and control plane). Today is pinned
/// to the date the mock's plans are built around. Only this flag can point either origin away from
/// the real services.
fn contract_pco_options(port: u16) -> Result<PcoOptions, Box<dyn std::error::Error>> {
    let origin = format!("http://127.0.0.1:{port}");
    Ok(PcoOptions {
        client_id: Some("client-id-1".to_string()),
        memory_tokens: true,
        api_base: Some(url::Url::parse(&format!("{origin}/"))?),
        control_plane_origin: Some(origin),
        today: Some(std::sync::Arc::new(|_| {
            chrono::NaiveDate::from_ymd_opt(2026, 10, 18).unwrap_or_default()
        })),
        ..PcoOptions::default()
    })
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().collect();
    let contract_pco = args.iter().any(|a| a == "--contract-pco");
    let demo = contract_pco || args.iter().any(|a| a == "--demo");
    let service = if contract_pco {
        SettingsService::ephemeral_with_secret(
            contract_pco_settings(),
            Some("secret-1".to_string()),
        )
    } else if demo {
        SettingsService::ephemeral(RuntimeSettings::demo())
    } else {
        let service = SettingsService::default_service();
        service.load();
        service
    };
    let runtime = if contract_pco {
        let port = arg(&args, "--pco-port")
            .and_then(|p| p.parse::<u16>().ok())
            .ok_or("--contract-pco needs --pco-port")?;
        Runtime::with_pco(Arc::new(service), false, contract_pco_options(port)?)
    } else if demo {
        // In-memory instances never touch the OS keychain.
        Runtime::new(Arc::new(service), false)
    } else {
        Runtime::with_pco(Arc::new(service), true, PcoOptions::default())
    };
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
