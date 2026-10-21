//! Quick-tunnel transport adapter (`remote_quick.py`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use regex::bytes::Regex;
use uuid::Uuid;

use crate::connector::{child_env, stop_child};
use crate::error::Res;
use crate::feature::RemoteFeature;
use crate::files::{atomic_write, safe_status, DesiredRemote};
use crate::http::HttpClient;
use crate::platform::{ChildProcess, Platform, SpawnSpec};
use crate::status_map;

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com\b").expect("quick url regex")
    })
}

/// State written by the output reader thread. `epoch` identifies the process it belongs to so a
/// stale reader never publishes (`self.process is process` in Python).
#[derive(Default)]
struct Shared {
    epoch: u64,
    url: Option<String>,
    rate_limited: bool,
}

pub struct QuickConnector {
    binary: PathBuf,
    feature: RemoteFeature,
    metrics_port: u16,
    process: Option<Box<dyn ChildProcess>>,
    reader: Option<JoinHandle<()>>,
    shared: Arc<Mutex<Shared>>,
    next_epoch: AtomicU64,
    generation: String,
    listener_generation: String,
    retry_at: f64,
    delay: f64,
    started_at: f64,
    platform: Platform,
}

/// Read bounded lines (4096 bytes), discarding everything except the quick URL and a 429 marker.
fn capture(mut stream: Box<dyn Read + Send>, shared: Arc<Mutex<Shared>>, epoch: u64) {
    let mut line: Vec<u8> = Vec::new();
    let mut buf = [0_u8; 4096];
    let handle = |line: &[u8]| {
        let mut shared = shared.lock().unwrap();
        if shared.epoch != epoch {
            return;
        }
        if line.windows(10).any(|w| w == b"status 429") {
            shared.rate_limited = true;
        }
        if let Some(m) = url_re().find(line) {
            shared.url = Some(String::from_utf8_lossy(m.as_bytes()).into_owned());
        }
    };
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            line.push(byte);
            if byte == b'\n' || line.len() >= 4096 {
                handle(&line);
                line.clear();
            }
        }
    }
    if !line.is_empty() {
        handle(&line);
    }
}

impl QuickConnector {
    pub fn new(
        binary: &Path,
        feature: RemoteFeature,
        metrics_port: u16,
        platform: &Platform,
    ) -> Self {
        Self {
            binary: binary.to_path_buf(),
            feature,
            metrics_port,
            process: None,
            reader: None,
            shared: Arc::new(Mutex::new(Shared::default())),
            next_epoch: AtomicU64::new(1),
            generation: String::new(),
            listener_generation: String::new(),
            retry_at: 0.0,
            delay: 1.0,
            started_at: 0.0,
            platform: platform.clone(),
        }
    }

    pub fn url(&self) -> Option<String> {
        self.shared.lock().unwrap().url.clone()
    }

    pub fn has_process(&self) -> bool {
        self.process.is_some()
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            stop_child(&mut child);
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
        let mut shared = self.shared.lock().unwrap();
        shared.url = None;
        shared.epoch = 0;
    }

    fn exited(&mut self) -> bool {
        self.process
            .as_mut()
            .is_some_and(|p| !matches!(p.try_wait(), Ok(None)))
    }

    fn clear_desired(&self) -> Res<()> {
        atomic_write(&self.feature.control, &DesiredRemote::default().to_json())
    }

    fn start(&mut self, port: u16) -> Res<()> {
        self.platform.ports.check_free(self.metrics_port)?;
        let home = self
            .feature
            .control
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        let null_config = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let spec = SpawnSpec {
            program: self.binary.clone(),
            args: vec![
                "tunnel".into(),
                "--config".into(),
                null_config.into(),
                "--no-autoupdate".into(),
                "--metrics".into(),
                format!("127.0.0.1:{}", self.metrics_port),
                "--url".into(),
                format!("http://127.0.0.1:{port}"),
            ],
            env: child_env(&home),
            capture_output: true,
        };
        let mut child = self.platform.spawner.spawn(&spec)?;
        let epoch = self.next_epoch.fetch_add(1, Ordering::SeqCst);
        self.shared.lock().unwrap().epoch = epoch;
        self.started_at = self.platform.clock.monotonic();
        self.listener_generation = Uuid::new_v4().to_string();
        let streams = child.take_output();
        self.process = Some(child);
        let shared = self.shared.clone();
        self.reader = Some(std::thread::spawn(move || {
            // Merge stdout and stderr as `stderr=STDOUT`: one reader per stream.
            let handles: Vec<_> = streams
                .into_iter()
                .map(|s| {
                    let shared = shared.clone();
                    std::thread::spawn(move || capture(s, shared, epoch))
                })
                .collect();
            for h in handles {
                let _ = h.join();
            }
        }));
        Ok(())
    }

    pub fn step(&mut self) -> Res<()> {
        let _guard = self.feature.locked()?;
        let clock = self.platform.clock.clone();
        let intent = self.feature.intent();
        if intent.generation != self.generation || !intent.enabled {
            self.stop();
            self.generation = intent.generation.clone();
            self.retry_at = 0.0;
            self.delay = 1.0;
            self.shared.lock().unwrap().rate_limited = false;
            self.clear_desired()?;
        }
        let mut state = "off";
        let available = is_executable(&self.binary);
        if intent.enabled {
            state = "enabling";
            if self.exited() {
                self.stop();
                self.clear_desired()?;
                self.retry_at = clock.monotonic() + self.delay;
                self.delay = (30.0_f64).min(self.delay * 2.0);
            }
            if self.shared.lock().unwrap().rate_limited {
                self.stop();
                self.clear_desired()?;
                state = "error";
            } else if !available {
                state = "error";
            } else if self.process.is_none() && clock.monotonic() >= self.retry_at {
                if let Err(error) = self.start(intent.port) {
                    if !error.is_io_or_value() {
                        return Err(error);
                    }
                    self.retry_at = clock.monotonic() + self.delay;
                    self.delay = (30.0_f64).min(self.delay * 2.0);
                    state = "error";
                }
            }
            let url = self.url();
            if let Some(url) = &url {
                let desired = DesiredRemote {
                    enabled: true,
                    generation: self.listener_generation.clone(),
                    public_origin: url.clone(),
                    port: intent.port,
                };
                let text = desired.to_json();
                if std::fs::read_to_string(&self.feature.control)? != text {
                    atomic_write(&self.feature.control, &text)?;
                }
                state = "reconnecting";
                let edge = HttpClient::new(
                    &format!("http://127.0.0.1:{}", self.metrics_port),
                    1.0,
                    self.platform.http.clone(),
                )
                .get("/ready");
                let local = HttpClient::new(
                    &format!("http://127.0.0.1:{}", intent.port),
                    1.0,
                    self.platform.http.clone(),
                )
                .request(
                    "GET",
                    "/api/v1/access",
                    &[("X-Forwarded-Proto".into(), "https".into())],
                    &[],
                    None,
                );
                if let (Ok(edge), Ok(local)) = (edge, local) {
                    if edge.status == 200 && local.status == 200 {
                        state = "connected";
                        self.delay = 1.0;
                    }
                }
            } else if self.process.is_some() && clock.monotonic() - self.started_at > 60.0 {
                self.stop();
                self.retry_at = clock.monotonic() + 30.0;
                state = "error";
            } else if self.process.is_none() {
                state = "error";
            }
        }
        let url = if state == "connected" {
            self.url()
        } else {
            None
        };
        safe_status(
            &self.feature.status_path,
            status_map!(
                "state" => state,
                "available" => available,
                "checked_at" => clock.wall(),
                "generation" => intent.generation,
                "url" => url,
            ),
        )
    }
}

impl Drop for QuickConnector {
    fn drop(&mut self) {
        self.stop();
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
