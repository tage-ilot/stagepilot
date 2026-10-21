//! Production wiring shared by `stagepilot-server` and `stagepilot-headless`: the Rust
//! counterpart of the desktop-Remote half of `main.create_app` plus
//! `remote_runtime.attach_managed_remote` / `remote_desktop.attach_desktop_remote`.
//!
//! Python wraps the FastAPI lifespan twice. Startup order: the application lifespan (state
//! service, plugins, `application.started`), then the managed-Remote reconcile task, then the
//! desktop supervisor (`DesktopRemoteManager.run`). Shutdown is the reverse: the supervisor stops
//! first (it stops the bundled `cloudflared` connector), then the reconcile task closes the
//! dedicated listener, then the application lifespan runs. [`RemoteLifecycle`] keeps that order.

use crate::remote_feature::RemoteFeature;
use crate::remote_ingress::RemoteIngress;
use crate::remote_types::{
    ActiveEnrollment, DiagnosticsBootstrap, ManagerError, RemoteAccess, RemoteManager,
};
use crate::runtime::Runtime;
use async_trait::async_trait;
use axum::http::StatusCode;
use axum::Router;
use serde_json::Value;
use stagepilot_core::auth::RemoteStore;
use stagepilot_remote::bootstrap::{trusted_control_plane_origins, DesktopBootstrapStore};
use stagepilot_remote::desktop::{DesktopOptions, DesktopRemoteManager};
use stagepilot_remote::error::{RemoteError, Res};
use stagepilot_remote::platform::{Platform, StopFlag};
use stagepilot_remote::runtime::{GenerationSink, IngressFactory, ManagedRemote};
use stagepilot_remote::server::ListenerAttachment;
use std::path::PathBuf;
use std::sync::Arc;

/// `STAGEPILOT_DESKTOP_REMOTE_ROOT` and `STAGEPILOT_CLOUDFLARED_BINARY`.
pub const ENV_REMOTE_ROOT: &str = "STAGEPILOT_DESKTOP_REMOTE_ROOT";
pub const ENV_CLOUDFLARED: &str = "STAGEPILOT_CLOUDFLARED_BINARY";

fn manager_error(error: RemoteError) -> ManagerError {
    // Python's routes catch `ProviderError` (and the revoked subclasses) and `ValueError`; an
    // `OSError` escapes both and ends as a 500, which is what the `Value` arm answers here.
    if error.is_provider() {
        ManagerError::Provider(error.to_string())
    } else {
        ManagerError::Value(error.to_string())
    }
}

/// `DesktopBootstrapStore` as the diagnostics route reads it.
pub struct RealBootstrap(pub Arc<DesktopBootstrapStore>);

impl DiagnosticsBootstrap for RealBootstrap {
    fn trusted_origins(&self) -> Vec<String> {
        trusted_control_plane_origins()
    }

    fn active(&self) -> Result<Option<ActiveEnrollment>, String> {
        self.0
            .state()
            .map(|state| {
                state.active.map(|a| ActiveEnrollment {
                    control_plane_origin: a.control_plane_origin,
                    installation_id: a.installation_id,
                })
            })
            .map_err(|e| e.to_string())
    }

    fn credential(&self, active: &ActiveEnrollment) -> Result<String, String> {
        let state = self.0.state().map_err(|e| e.to_string())?;
        let metadata = state
            .active
            .filter(|m| m.installation_id == active.installation_id)
            .ok_or_else(|| "The enrolment changed".to_string())?;
        self.0.credential(&metadata).map_err(|e| e.to_string())
    }
}

/// `DesktopRemoteManager` behind the route trait. The manager is blocking (files, HTTP), so each
/// call runs on the blocking pool.
pub struct RealRemoteManager {
    pub manager: Arc<DesktopRemoteManager>,
}

impl RealRemoteManager {
    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&DesktopRemoteManager) -> Res<T> + Send + 'static,
    ) -> Result<T, ManagerError> {
        let manager = self.manager.clone();
        match tokio::task::spawn_blocking(move || work(&manager)).await {
            Ok(result) => result.map_err(manager_error),
            Err(error) => Err(ManagerError::Value(error.to_string())),
        }
    }
}

#[async_trait]
impl RemoteManager for RealRemoteManager {
    async fn status(&self) -> Value {
        let manager = self.manager.clone();
        tokio::task::spawn_blocking(move || Value::Object(manager.status()))
            .await
            .unwrap_or(Value::Null)
    }

    async fn enable(&self) -> Result<(), ManagerError> {
        self.blocking(|m| m.enable().map(drop)).await
    }

    async fn disable(&self) -> Result<(), ManagerError> {
        self.blocking(|m| m.disable().map(drop)).await
    }

    async fn reset_identity(&self) -> Result<(), ManagerError> {
        self.blocking(|m| m.reset_identity().map(drop)).await
    }

    async fn regenerate(&self) -> Result<(), ManagerError> {
        self.blocking(|m| m.regenerate().map(drop)).await
    }

    fn bootstrap(&self) -> Option<Arc<dyn DiagnosticsBootstrap>> {
        Some(Arc::new(RealBootstrap(self.manager.bootstrap.clone())))
    }
}

/// `RemoteAccess.call(store.installation_generation, generation)`.
pub struct StoreGeneration(pub Arc<RemoteAccess>);

impl GenerationSink for StoreGeneration {
    fn installation_generation(&self, generation: &str) -> Res<()> {
        self.0
            .store
            .installation_generation(generation)
            .map_err(|e| RemoteError::value(e.detail))
    }
}

/// `RemoteIngress(application, public_origin=...)` around the application router. An origin the
/// ingress rejects yields a router that answers 503 (the reconcile loop validated it already).
pub fn ingress_factory(access: Arc<RemoteAccess>, app: Router) -> IngressFactory {
    Arc::new(
        move |origin: &str| match RemoteIngress::new(access.clone(), origin) {
            Ok(ingress) => ingress.wrap(app.clone()),
            Err(_) => Router::new().fallback(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        },
    )
}

/// The running Remote background tasks.
pub struct RemoteLifecycle {
    managed: ManagedRemote,
    stop: Arc<StopFlag>,
    supervisor: tokio::task::JoinHandle<()>,
}

impl RemoteLifecycle {
    /// Stop in the Python order: supervisor (and its connector) first, then the listener.
    pub async fn stop(self) {
        self.stop.set();
        let _ = self.supervisor.await;
        self.managed.finish().await;
    }
}

/// Where the desktop Remote lives, from the environment. `None` unless both variables are set
/// and non-empty (the same test as `main.create_app`).
pub fn desktop_remote_from_env() -> Option<(PathBuf, PathBuf)> {
    let get = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    Some((
        PathBuf::from(get(ENV_REMOTE_ROOT)?),
        PathBuf::from(get(ENV_CLOUDFLARED)?),
    ))
}

/// Wire the packaged-desktop Remote into `runtime` and start its tasks. Call after
/// `Runtime::start` (the application lifespan is the innermost one) with the application router.
/// Returns `Ok(None)` when the environment does not configure Remote.
pub async fn attach_desktop_remote(
    runtime: &Arc<Runtime>,
    app: Router,
) -> Result<Option<RemoteLifecycle>, Box<dyn std::error::Error>> {
    let Some((root, cloudflared)) = desktop_remote_from_env() else {
        return Ok(None);
    };
    attach_with(runtime, app, &root, &cloudflared, Platform::system(), None).await
}

/// The same with explicit paths, platform seams and manager options (tests).
pub async fn attach_with(
    runtime: &Arc<Runtime>,
    app: Router,
    root: &std::path::Path,
    cloudflared: &std::path::Path,
    platform: Platform,
    options: Option<DesktopOptions>,
) -> Result<Option<RemoteLifecycle>, Box<dyn std::error::Error>> {
    let lan_port = runtime.settings.bind_port;
    let access = RemoteAccess::new(RemoteStore::new(root.join("identity.sqlite3")));
    runtime.set_remote_access(Some(access.clone()));
    let revoke = {
        let access = access.clone();
        Arc::new(move || {
            let _ = access
                .store
                .installation_generation(&uuid::Uuid::new_v4().to_string());
        }) as Arc<dyn Fn() + Send + Sync>
    };
    let options = DesktopOptions {
        revoke_sessions: Some(revoke),
        lan_port,
        ..options.unwrap_or_default()
    };
    let manager = Arc::new(DesktopRemoteManager::new(
        root,
        cloudflared,
        platform,
        options,
    )?);
    // attach_managed_remote: the reconcile task and the RemoteFeature the routes read.
    let desired = manager.desired_path.clone();
    let mut attachment = ListenerAttachment::new();
    let managed = ManagedRemote::attach(
        &desired,
        lan_port,
        ingress_factory(access.clone(), app),
        Arc::new(StoreGeneration(access)),
        &mut attachment,
    )?;
    runtime.set_remote_feature(Some(Arc::new(RemoteFeature::new(desired))));
    runtime.set_remote_manager(Some(Arc::new(RealRemoteManager {
        manager: manager.clone(),
    })));
    // attach_desktop_remote: the supervisor loop.
    let stop = StopFlag::new();
    let supervisor = tokio::spawn(manager.run(stop.clone()));
    Ok(Some(RemoteLifecycle {
        managed,
        stop,
        supervisor,
    }))
}

/// Resolves on Ctrl-C or (Unix) SIGTERM, as uvicorn does.
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// The settings ask for real MIDI (MIDI input, or lights sent as MIDI) but `hardware` is false,
/// i.e. the binary was built without `midir-backend`. Such a build would silently send no MIDI,
/// so startup refuses with this message instead.
pub fn midi_build_problem(
    settings: &stagepilot_core::settings::RuntimeSettings,
    hardware: bool,
) -> Option<String> {
    use stagepilot_core::config::LightingMethod;
    if hardware {
        return None;
    }
    let mut wanted = Vec::new();
    if settings.integration_modes.midi_source == stagepilot_core::config::MidiSource::Real {
        wanted.push("MIDI input (the MIDI source is Real)");
    }
    if settings.lights.enabled && settings.lights.method == LightingMethod::Midi {
        wanted.push("lights sent as MIDI");
    }
    if wanted.is_empty() {
        return None;
    }
    Some(format!(
        "the settings use {}, but this binary was built without MIDI hardware support \
         (cargo feature `midir-backend`); it would send no MIDI. Rebuild with \
         `--features midir-backend`, or change the settings.",
        wanted.join(" and ")
    ))
}

/// `main.run()`: build the production runtime from saved settings and the keychain, start it,
/// attach the desktop Remote when the environment configures it, serve until a signal, then stop
/// in reverse order. The bind address is `runtime.settings` (which already folds in
/// `STAGEPILOT_HOST` / `STAGEPILOT_PORT`) unless overridden.
pub async fn run_production(
    host: Option<String>,
    port: Option<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_production_until(host, port, shutdown_signal()).await
}

/// [`run_production`] with a caller-supplied stop trigger instead of the process signals.
/// The Tauri shell's `in-process` feature uses this: the shell, not a signal, decides when the
/// server stops.
pub async fn run_production_until<F>(
    host: Option<String>,
    port: Option<u16>,
    stop: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let service = stagepilot_core::settings::SettingsService::default_service();
    service.load();
    let runtime = Runtime::with_pco(
        Arc::new(service),
        true,
        stagepilot_pco::runtime::PcoOptions::default(),
    );
    if let Some(problem) = midi_build_problem(&runtime.settings, stagepilot_midi::HARDWARE_SUPPORT)
    {
        return Err(problem.into());
    }
    tracing::info!(
        midi_backend = stagepilot_midi::backend_name(),
        "stagepilot_midi_backend"
    );
    let host = host.unwrap_or_else(|| runtime.settings.bind_host.clone());
    let port = port.unwrap_or(runtime.settings.bind_port);
    let address: std::net::SocketAddr = format!("{host}:{port}").parse()?;
    serve_until(runtime, address, crate::default_web_root(), stop).await
}

/// Start `runtime`, serve the application on `address`, and shut down cleanly on a signal.
pub async fn serve(
    runtime: Arc<Runtime>,
    address: std::net::SocketAddr,
    web_root: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    serve_until(runtime, address, web_root, shutdown_signal()).await
}

/// [`serve`] that stops when `stop` resolves (graceful: in-flight requests finish, then the
/// Remote tasks and the runtime stop in reverse start order).
pub async fn serve_until<F>(
    runtime: Arc<Runtime>,
    address: std::net::SocketAddr,
    web_root: Option<PathBuf>,
    stop: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // Bind first so a port conflict fails before any plugin or connector starts.
    let listener = tokio::net::TcpListener::bind(address).await?;
    runtime.enable_startup_activation();
    runtime.start().await;
    let app = crate::build_app(runtime.clone(), crate::AppOptions { web_root });
    // Flatten the error before any further await: `Box<dyn Error>` is not `Send`, and the
    // in-process shell spawns this future on a multi-threaded runtime.
    let attached = attach_desktop_remote(&runtime, app.clone())
        .await
        .map_err(|error| error.to_string());
    let remote = match attached {
        Ok(remote) => remote,
        Err(message) => {
            runtime.stop().await;
            return Err(message.into());
        }
    };
    tracing::info!(%address, "stagepilot_server_listening");
    let served = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(stop)
    .await;
    if let Some(remote) = remote {
        remote.stop().await;
    }
    runtime.stop().await;
    served?;
    Ok(())
}
