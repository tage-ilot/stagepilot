//! Reconcile the local Remote listener without restarting production (`remote_runtime.py`).

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use uuid::Uuid;

use crate::error::{RemoteError, Res};
use crate::files::read_desired;
use crate::platform::StopFlag;
use crate::server::{remote_listener, require_https, ListenerAttachment};

/// The Remote identity store's generation call (`RemoteAccess.call(store.installation_generation)`).
pub trait GenerationSink: Send + Sync {
    fn installation_generation(&self, generation: &str) -> Res<()>;
}

/// Builds the ingress for one public origin (`RemoteIngress(application, public_origin=...)`,
/// which belongs to the server crate).
pub type IngressFactory = Arc<dyn Fn(&str) -> Router + Send + Sync>;

async fn read_desired_async(path: &Path) -> Res<crate::files::DesiredRemote> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || read_desired(&path))
        .await
        .map_err(|e| RemoteError::io(e.to_string()))
}

/// One pass of the reconcile loop body. Errors of the caught kinds are logged by the caller.
async fn reconcile_once(
    ingress: &IngressFactory,
    sink: &Arc<dyn GenerationSink>,
    path: &Path,
    lan_port: u16,
    stop: &StopFlag,
) -> Res<()> {
    let desired = read_desired_async(path).await?;
    if !desired.enabled {
        return Ok(());
    }
    Uuid::parse_str(&desired.generation).map_err(|e| RemoteError::value(e.to_string()))?;
    if desired.port == lan_port {
        return Err(RemoteError::value("Remote cannot bind the LAN port"));
    }
    let router = require_https(ingress(&desired.public_origin));
    sink.installation_generation(&desired.generation)?;
    if let Some(listener) = remote_listener(router, desired.port).await {
        while !listener.is_finished() && !stop.is_set() {
            if read_desired_async(path).await? != desired {
                break;
            }
            stop.wait(0.25).await;
        }
        listener.stop().await;
    }
    Ok(())
}

pub async fn reconcile(
    ingress: IngressFactory,
    sink: Arc<dyn GenerationSink>,
    path: &Path,
    lan_port: u16,
    stop: Arc<StopFlag>,
) {
    while !stop.is_set() {
        if reconcile_once(&ingress, &sink, path, lan_port, &stop)
            .await
            .is_err()
        {
            tracing::error!("Managed Remote unavailable; local production continues.");
        }
        stop.wait(1.0).await;
    }
}

/// Validated managed-Remote attachment; the reconcile task runs until `finish`.
pub struct ManagedRemote {
    stop: Arc<StopFlag>,
    task: tokio::task::JoinHandle<()>,
}

impl ManagedRemote {
    pub fn attach(
        path: &Path,
        lan_port: u16,
        ingress: IngressFactory,
        sink: Arc<dyn GenerationSink>,
        attachment: &mut ListenerAttachment,
    ) -> Res<Self> {
        if !path.is_absolute() {
            return Err(RemoteError::value("Remote control file must be absolute"));
        }
        attachment
            .mark()
            .map_err(|_| RemoteError::value("Remote listener is already attached"))?;
        let stop = StopFlag::new();
        let task = {
            let (stop, path) = (stop.clone(), path.to_path_buf());
            tokio::spawn(async move { reconcile(ingress, sink, &path, lan_port, stop).await })
        };
        Ok(Self { stop, task })
    }

    pub async fn finish(self) {
        self.stop.set();
        let _ = self.task.await;
    }
}
