//! `stagepilot-server`: run the Rust backend headless.
//!
//!   stagepilot-server                 saved settings, keychain, dashboard PIN enforced
//!   stagepilot-server --demo --port N in-memory demo settings, PIN not enforced (contract runs)
//!
//! The bind address comes from the settings (`STAGEPILOT_HOST` / `STAGEPILOT_PORT` override them);
//! `--host` and `--port` override both.

use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_server::{build_app, default_web_root, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;

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
    let demo = args.iter().any(|a| a == "--demo");
    let service = if demo {
        SettingsService::ephemeral(RuntimeSettings::demo())
    } else {
        let service = SettingsService::default_service();
        service.load();
        service
    };
    let runtime = Runtime::new(Arc::new(service), !demo);
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
