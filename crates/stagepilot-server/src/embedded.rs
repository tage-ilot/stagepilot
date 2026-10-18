//! The server as a library-managed task, for the Tauri shell's `in-process` feature.
//!
//! The Python sidecar is a child process the shell spawns, watches and kills. Here the same
//! lifecycle is a tokio task in the shell's own process:
//!
//! * start: [`EmbeddedServer::start`] runs `launch::run_production_until` (the same startup
//!   order as `stagepilot-headless`) on a runtime the server owns, so it is independent of the
//!   shell's own async runtime and callable from any thread;
//! * readiness: the shell keeps probing `/api/health` on the port, exactly as it does for the
//!   sidecar, so "ready" means the same thing in both modes;
//! * crash handling: the receiver returned by `start` yields an [`ExitReport`] when the task ends
//!   for any reason other than a requested stop (an `Err` from startup such as a port conflict,
//!   or a panic), which the shell turns into the same `Failed` state a sidecar exit produces;
//! * graceful stop: [`EmbeddedServer::stop`] triggers axum's graceful shutdown (in-flight
//!   requests finish, then Remote, plugins and the state service stop in reverse order) and
//!   aborts the task if it has not finished within the grace period. It is synchronous so the
//!   shell can call it from its exit handler and from async commands alike.
//!
//! The shell owns port selection (saved setting, then the occupied/external probe) and passes the
//! result in as `STAGEPILOT_*` environment variables before calling `start`, as it does for the
//! sidecar.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tokio::runtime::{Builder, Runtime};
use tokio::sync::oneshot;
use tokio::task::AbortHandle;

/// How long [`EmbeddedServer::stop`] waits for the graceful shutdown before aborting the task.
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(8);

/// Why the server task ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitReport {
    /// Returned `Ok` without a stop request.
    Finished,
    /// Startup or serving failed (port conflict, bad bind address, ...).
    Failed(String),
    /// The task panicked.
    Panicked(String),
}

impl ExitReport {
    /// Human-readable text for the crash log and the status message.
    pub fn message(&self) -> String {
        match self {
            ExitReport::Finished => "The embedded StagePilot server stopped unexpectedly.".into(),
            ExitReport::Failed(error) => {
                format!("The embedded StagePilot server failed: {error}")
            }
            ExitReport::Panicked(error) => {
                format!("The embedded StagePilot server crashed: {error}")
            }
        }
    }
}

type StopFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// A running in-process server.
pub struct EmbeddedServer {
    runtime: Option<Runtime>,
    stop: Option<oneshot::Sender<()>>,
    task: AbortHandle,
    stopping: Arc<AtomicBool>,
}

impl EmbeddedServer {
    /// Run the production server on `host:port` (saved settings and `STAGEPILOT_*` variables
    /// decide the rest). Server logs go to `log_path` (JSON lines) when given and no subscriber
    /// is installed yet. Returns the handle and a channel that yields once if the server ends
    /// without having been asked to stop.
    pub fn start(
        host: String,
        port: u16,
        log_path: Option<PathBuf>,
    ) -> std::io::Result<(Self, mpsc::Receiver<ExitReport>)> {
        if let Some(path) = log_path {
            install_file_logging(&path);
        }
        Self::start_with(move |stop| async move {
            crate::launch::run_production_until(Some(host), Some(port), stop)
                .await
                .map_err(|error| error.to_string())
        })
    }

    /// Run any server body. `run` receives the stop future and must return when it resolves.
    /// The production path is [`EmbeddedServer::start`]; tests drive this with small bodies.
    pub fn start_with<R, Fut>(run: R) -> std::io::Result<(Self, mpsc::Receiver<ExitReport>)>
    where
        R: FnOnce(StopFuture) -> Fut,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("stagepilot-server")
            .enable_all()
            .build()?;
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        // A dropped sender (the shell going away) also stops the server.
        let stop: StopFuture = Box::pin(async move {
            let _ = stop_rx.await;
        });
        let stopping = Arc::new(AtomicBool::new(false));
        let (report_tx, report_rx) = mpsc::channel();
        let inner = runtime.spawn(run(stop));
        let task = inner.abort_handle();
        let watcher_stopping = stopping.clone();
        runtime.spawn(async move {
            let report = match inner.await {
                Ok(Ok(())) => ExitReport::Finished,
                Ok(Err(error)) => ExitReport::Failed(error),
                Err(join) if join.is_panic() => ExitReport::Panicked(join.to_string()),
                Err(join) => ExitReport::Failed(join.to_string()),
            };
            if !watcher_stopping.load(Ordering::SeqCst) {
                let _ = report_tx.send(report);
            }
        });
        Ok((
            Self {
                runtime: Some(runtime),
                stop: Some(stop_tx),
                task,
                stopping,
            },
            report_rx,
        ))
    }

    /// True while the server task has not finished.
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    /// Graceful stop, blocking the calling thread for at most `grace` (plus a moment to tear the
    /// runtime down). Returns `true` when the server finished by itself within `grace`, `false`
    /// when it had to be aborted. Safe to call from async code: it never blocks on the runtime
    /// it stops.
    pub fn stop(mut self, grace: Duration) -> bool {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let deadline = Instant::now() + grace;
        while self.is_running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let graceful = !self.is_running();
        if !graceful {
            self.task.abort();
        }
        if let Some(runtime) = self.runtime.take() {
            // `shutdown_background` never blocks, so it is valid inside an async context.
            runtime.shutdown_background();
        }
        graceful
    }
}

impl Drop for EmbeddedServer {
    fn drop(&mut self) {
        // Dropped without `stop` (the shell is going away): the stop sender drops with us, which
        // asks the server to stop; abort so a wedged server never outlives the handle.
        self.stopping.store(true, Ordering::SeqCst);
        self.task.abort();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Send the server's `tracing` output to `path` as JSON lines (the headless binary writes the
/// same lines to stdout). A no-op when a global subscriber already exists.
fn install_file_logging(path: &std::path::Path) {
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let level = std::env::var("STAGEPILOT_LOG_LEVEL")
        .map(|v| v.to_lowercase())
        .unwrap_or_else(|_| "info".into());
    let filter = tracing_subscriber::EnvFilter::try_new(&level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file))
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRACE: Duration = Duration::from_secs(3);

    #[test]
    fn stop_is_graceful_when_the_body_returns() {
        let (server, reports) = EmbeddedServer::start_with(|stop| async move {
            stop.await;
            Ok(())
        })
        .unwrap();
        assert!(server.is_running());
        assert!(server.stop(GRACE));
        // A requested stop is not a crash: no report.
        assert!(reports.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn stop_aborts_a_wedged_server_after_the_grace_period() {
        let (server, reports) = EmbeddedServer::start_with(|_stop| async move {
            std::future::pending::<()>().await;
            Ok(())
        })
        .unwrap();
        assert!(!server.stop(Duration::from_millis(100)));
        assert!(reports.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn startup_failure_is_reported_as_failed() {
        let (server, reports) =
            EmbeddedServer::start_with(|_stop| async move { Err("address in use".to_string()) })
                .unwrap();
        let report = reports.recv_timeout(GRACE).expect("a report");
        assert_eq!(report, ExitReport::Failed("address in use".into()));
        assert!(report.message().contains("address in use"));
        assert!(!server.is_running() || server.stop(GRACE));
    }

    #[test]
    fn a_panic_is_reported_as_a_crash() {
        let (_server, reports) = EmbeddedServer::start_with(|_stop| async move {
            if true {
                panic!("boom");
            }
            Ok(())
        })
        .unwrap();
        assert!(matches!(
            reports.recv_timeout(GRACE),
            Ok(ExitReport::Panicked(_))
        ));
    }

    #[test]
    fn an_unrequested_clean_return_is_reported_as_finished() {
        let (_server, reports) = EmbeddedServer::start_with(|_stop| async move { Ok(()) }).unwrap();
        assert_eq!(reports.recv_timeout(GRACE), Ok(ExitReport::Finished));
    }

    #[test]
    fn stop_is_safe_from_inside_an_async_runtime() {
        // The shell calls stop from async Tauri commands.
        let outer = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        outer.block_on(async {
            let (server, _reports) = EmbeddedServer::start_with(|stop| async move {
                stop.await;
                Ok(())
            })
            .unwrap();
            assert!(server.stop(GRACE));
        });
    }

    #[test]
    fn dropping_the_handle_ends_the_server() {
        let (server, _reports) = EmbeddedServer::start_with(|_stop| async move {
            std::future::pending::<()>().await;
            Ok(())
        })
        .unwrap();
        let task = server.task.clone();
        drop(server);
        let deadline = Instant::now() + GRACE;
        while !task.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(task.is_finished());
    }

    #[test]
    fn serves_real_http_and_stops_gracefully() {
        // The production body on an ephemeral port with isolated settings: the real server, the
        // real health route, then a graceful stop.
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::env::set_var("STAGEPILOT_SETTINGS_PATH", &settings);
        let port = {
            let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            probe.local_addr().unwrap().port()
        };
        let (server, reports) = EmbeddedServer::start("127.0.0.1".into(), port, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut body = None;
        while Instant::now() < deadline && body.is_none() {
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                use std::io::{Read, Write};
                let _ = stream.write_all(
                    b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                );
                let mut text = String::new();
                let _ = stream.read_to_string(&mut text);
                if text.starts_with("HTTP/1.1 200") {
                    body = Some(text);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let text = body.expect("the embedded server answered /api/v1/health");
        assert!(
            text.contains("\"application_status\"") && text.contains("\"version\""),
            "{text}"
        );
        assert!(server.stop(DEFAULT_STOP_GRACE));
        assert!(reports.recv_timeout(Duration::from_millis(200)).is_err());
        // The port is released.
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    #[test]
    fn a_port_conflict_is_a_reported_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("STAGEPILOT_SETTINGS_PATH", dir.path().join("settings.json"));
        let holder = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = holder.local_addr().unwrap().port();
        let (_server, reports) = EmbeddedServer::start("127.0.0.1".into(), port, None).unwrap();
        match reports.recv_timeout(Duration::from_secs(20)) {
            Ok(ExitReport::Failed(message)) => assert!(!message.is_empty()),
            other => panic!("expected a startup failure, got {other:?}"),
        }
    }
}
