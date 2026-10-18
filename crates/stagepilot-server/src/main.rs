//! `stagepilot-server`: run the Rust backend headless.
//!
//!   stagepilot-server                 saved settings, keychain, dashboard PIN enforced
//!   stagepilot-server --demo --port N in-memory demo settings, PIN not enforced (contract runs)
//!   stagepilot-server --contract-pco --pco-port N --port N
//!       the "pco" contract profile: Planning Center is the Services source and talks to a MOCK API
//!       and control plane on 127.0.0.1:N (never the real Planning Center). Mirrors
//!       `scripts/contract/pco_app.py`.
//!
//!   stagepilot-server --contract-outputs --propresenter-port N --port N
//!       the "outputs" contract profile: demo plan, fake MIDI devices, ProPresenter at 127.0.0.1:N
//!       (a mock), OSC to a loopback port nothing listens on. Mirrors `scripts/contract/outputs_app.py`.
//!
//!   stagepilot-server --contract-remote --remote-dir D --ingress-port N --web-root W --port N
//!       the "remote" and "static" contract profiles: demo app, a Remote identity store and
//!       feature file under D, the fixture dashboard in W, and the same app behind the trusted
//!       remote ingress (public origin https://remote.contract.test) on N. Mirrors
//!       `scripts/contract/remote_app.py` (`create_remote`).
//!
//!   stagepilot-server --contract-diagnostics --control-port N --remote-dir D --port N
//!       the "diagnostics" contract profile: a scripted Remote manager enrolled against the mock
//!       control plane at 127.0.0.1:N. Mirrors `remote_app.py` (`create_diagnostics`).
//!
//! The bind address comes from the settings (`STAGEPILOT_HOST` / `STAGEPILOT_PORT` override them);
//! `--host` and `--port` override both.

use stagepilot_core::auth::RemoteStore;
use stagepilot_core::config::{
    IntegrationModes, LightingMethod, LightsSettings, MidiSettings, MidiSource,
    ProPresenterSettings, ServiceSource, TimerOutput,
};
use stagepilot_core::settings::{
    PersistentPlanningCenterSettings, PersistentSettings, RuntimeSettings, SettingsService,
};
use stagepilot_pco::runtime::PcoOptions;
use stagepilot_server::remote_fakes::ContractManager;
use stagepilot_server::{
    build_app, default_web_root, AppOptions, OutputBackends, RemoteAccess, RemoteFeature,
    RemoteIngress, Runtime,
};

/// The public origin of the "remote" contract profile (`remote_profile.ORIGIN`).
const CONTRACT_PUBLIC_ORIGIN: &str = "https://remote.contract.test";
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
    let contract_pco = args.iter().any(|a| a == "--contract-pco");
    let contract_outputs = args.iter().any(|a| a == "--contract-outputs");
    let contract_remote = args.iter().any(|a| a == "--contract-remote");
    let contract_diagnostics = args.iter().any(|a| a == "--contract-diagnostics");
    let demo = contract_pco
        || contract_outputs
        || contract_remote
        || contract_diagnostics
        || args.iter().any(|a| a == "--demo");
    let service = if contract_pco {
        SettingsService::ephemeral_with_secret(
            contract_pco_settings(),
            Some("secret-1".to_string()),
        )
    } else if contract_outputs {
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
    let runtime = if contract_pco {
        let port = arg(&args, "--pco-port")
            .and_then(|p| p.parse::<u16>().ok())
            .ok_or("--contract-pco needs --pco-port")?;
        Runtime::with_pco(Arc::new(service), false, contract_pco_options(port)?)
    } else if contract_outputs {
        Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi())
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

    let remote_dir = arg(&args, "--remote-dir").map(std::path::PathBuf::from);
    let mut ingress_listener = None;
    if contract_remote || contract_diagnostics {
        let dir = remote_dir.ok_or("the remote contract profiles need --remote-dir")?;
        let access = RemoteAccess::new(RemoteStore::new(dir.join("identity.sqlite3")));
        runtime.set_remote_access(Some(access.clone()));
        if contract_remote {
            runtime.set_remote_feature(Some(Arc::new(RemoteFeature::new(
                dir.join("remote").join("remote.json"),
            ))));
            let port = arg(&args, "--ingress-port")
                .and_then(|p| p.parse::<u16>().ok())
                .ok_or("--contract-remote needs --ingress-port")?;
            let ingress = RemoteIngress::new(access, CONTRACT_PUBLIC_ORIGIN)?;
            ingress_listener = Some((
                tokio::net::TcpListener::bind(SocketAddr::new(address.ip(), port)).await?,
                ingress,
            ));
        } else {
            let port = arg(&args, "--control-port")
                .and_then(|p| p.parse::<u16>().ok())
                .ok_or("--contract-diagnostics needs --control-port")?;
            runtime.set_remote_manager(Some(Arc::new(ContractManager::new(&format!(
                "http://127.0.0.1:{port}"
            )))));
        }
    }
    let web_root = arg(&args, "--web-root")
        .map(std::path::PathBuf::from)
        .or_else(default_web_root);

    runtime.start().await;
    let app = build_app(runtime.clone(), AppOptions { web_root });
    if let Some((listener, ingress)) = ingress_listener {
        let wrapped = ingress.wrap(app.clone());
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                wrapped.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
        });
    }
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
