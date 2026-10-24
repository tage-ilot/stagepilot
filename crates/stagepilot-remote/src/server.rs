//! Loopback Remote listener (`remote_server.py`). A separately managed tunnel terminates public
//! HTTPS/WSS and forwards only to this dedicated port; transport is checked, never identity.

use std::net::SocketAddr;

use axum::extract::Request;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::{RemoteError, Res};

pub const REMOTE_HOST: &str = "127.0.0.1";

/// `HTTPSOnlyIngress`: exactly one `X-Forwarded-Proto: https` header, else 426 `no-store`.
/// (Python closes WebSocket handshakes with code 4403 after accepting; axum rejects the upgrade
/// before it with the same 426, which a browser reports as a failed connection.)
pub async fn https_only(request: Request, next: Next) -> Response {
    let values: Vec<&HeaderValue> = request
        .headers()
        .get_all("x-forwarded-proto")
        .iter()
        .collect();
    if values.len() != 1 || values[0].as_bytes() != b"https" {
        let mut response = StatusCode::UPGRADE_REQUIRED.into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    next.run(request).await
}

pub fn require_https(router: Router) -> Router {
    router.layer(middleware::from_fn(https_only))
}

/// A running Remote listener. Dropping it without `stop` aborts the server task.
pub struct RemoteListener {
    pub addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl RemoteListener {
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Graceful stop (`should_exit = True`, `await task`), bounded to 5 s like uvicorn's
    /// graceful timeout.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            if tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .is_err()
            {
                tracing::error!("Remote listener did not stop in time.");
            }
        }
    }
}

impl Drop for RemoteListener {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Bind `127.0.0.1:port` and serve `ingress`. `None` when the port is unavailable: a transport
/// failure must never take down local production.
pub async fn remote_listener(ingress: Router, port: u16) -> Option<RemoteListener> {
    let listener = match TcpListener::bind((REMOTE_HOST, port)).await {
        Ok(listener) => listener,
        Err(_) => {
            tracing::error!("Remote loopback port unavailable; LAN remains available.");
            return None;
        }
    };
    let addr = listener.local_addr().ok()?;
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let served = axum::serve(
            listener,
            // Handlers read the peer address (`ConnectInfo<SocketAddr>`); without it every
            // such route answers 500 (B6).
            ingress.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = stopped.await;
        })
        .await;
        if served.is_err() {
            // Never log request headers, credentials, or provider configuration here.
            tracing::error!("Remote listener stopped unexpectedly; LAN remains available.");
        }
    });
    Some(RemoteListener {
        addr,
        stop: Some(stop),
        task: Some(task),
    })
}

/// Tracks whether a listener was attached (`application.state.remote_listener_attached`).
#[derive(Default)]
pub struct ListenerAttachment {
    attached: bool,
}

impl ListenerAttachment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Validate and mark a fixed-origin listener attachment.
    pub fn attach_fixed(&mut self, public_origin: &str, port: u16) -> Res<()> {
        if port == 0 {
            return Err(RemoteError::value(
                "Remote port must be between 1 and 65535.",
            ));
        }
        if self.attached {
            return Err(RemoteError::value("Remote listener is already attached."));
        }
        // RemoteIngress requires an exact HTTPS origin.
        if !crate::files::is_exact_https_origin(public_origin, false) {
            return Err(RemoteError::value(
                "Remote public origin must be an exact HTTPS origin.",
            ));
        }
        self.mark()
    }

    pub fn mark(&mut self) -> Res<()> {
        if self.attached {
            return Err(RemoteError::value("Remote listener is already attached."));
        }
        self.attached = true;
        Ok(())
    }
}
