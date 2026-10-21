//! Independent connector supervisor (`remote_connector.py`): no production runtime or account
//! secrets. The child process, port probe, HTTP and clock are injected through `Platform`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::error::{RemoteError, Res};
use crate::feature::RemoteFeature;
use crate::files::{is_group_or_world_readable, read_desired, safe_status, DesiredRemote};
use crate::http::{HttpClient, HttpTransport};
use crate::platform::{ChildProcess, Clock, PortProbe, ProcessSpawner, SpawnSpec};
use crate::quick::QuickConnector;
use crate::status_map;

pub type TokenProvider = Arc<dyn Fn() -> Res<String> + Send + Sync>;

/// The environment a supervised `cloudflared` runs with (`PATH=os.defpath`, `HOME`).
pub fn child_env(home: &Path) -> Vec<(String, String)> {
    let path = if cfg!(windows) { "" } else { "/bin:/usr/bin" };
    vec![
        ("PATH".into(), path.into()),
        ("HOME".into(), home.to_string_lossy().into_owned()),
    ]
}

/// Terminate, wait 10 s, then kill and wait 5 s (`Connector.stop`/`QuickConnector.stop`).
pub fn stop_child(child: &mut Box<dyn ChildProcess>) {
    if matches!(child.try_wait(), Ok(None)) {
        child.terminate();
    }
    if !child.wait_timeout(10.0) {
        child.kill();
        child.wait_timeout(5.0);
    }
}

pub struct Connector {
    binary: PathBuf,
    control: PathBuf,
    status: PathBuf,
    metrics_port: u16,
    token_provider: Option<TokenProvider>,
    token_path: PathBuf,
    process: Option<Box<dyn ChildProcess>>,
    desired: DesiredRemote,
    retry_at: f64,
    delay: f64,
    clock: Arc<dyn Clock>,
    spawner: Arc<dyn ProcessSpawner>,
    ports: Arc<dyn PortProbe>,
    http: Arc<dyn HttpTransport>,
}

impl Connector {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        binary: &Path,
        control: &Path,
        status: &Path,
        metrics_port: u16,
        token_provider: Option<TokenProvider>,
        token_path: Option<PathBuf>,
        platform: &crate::platform::Platform,
    ) -> Self {
        Self {
            binary: binary.to_path_buf(),
            control: control.to_path_buf(),
            status: status.to_path_buf(),
            metrics_port,
            token_provider,
            token_path: token_path.unwrap_or_else(|| control.with_file_name("connector.token")),
            process: None,
            desired: DesiredRemote::default(),
            retry_at: 0.0,
            delay: 1.0,
            clock: platform.clock.clone(),
            spawner: platform.spawner.clone(),
            ports: platform.ports.clone(),
            http: platform.http.clone(),
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(|p| p.pid())
    }

    pub fn has_process(&self) -> bool {
        self.process.is_some()
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            stop_child(&mut child);
        }
        if self.token_provider.is_some() {
            let _ = std::fs::remove_file(&self.token_path);
        }
    }

    fn exited(&mut self) -> bool {
        self.process
            .as_mut()
            .is_some_and(|p| !matches!(p.try_wait(), Ok(None)))
    }

    fn token_arguments(&self) -> Res<Vec<String>> {
        if let Some(provider) = &self.token_provider {
            return Ok(vec!["--token".into(), provider()?]);
        }
        let token = &self.token_path;
        let private = std::fs::symlink_metadata(token)
            .ok()
            .filter(|m| m.is_file())
            .is_some_and(|_| {
                is_group_or_world_readable(token)
                    .map(|exposed| !exposed)
                    .unwrap_or(false)
            });
        if !private {
            return Err(RemoteError::io(
                "Installation token is missing or not private",
            ));
        }
        Ok(vec![
            "--token-file".into(),
            token.to_string_lossy().into_owned(),
        ])
    }

    fn start(&mut self) -> Res<()> {
        let token_arguments = self.token_arguments()?;
        // Do not mistake another local service's /ready for our connector.
        self.ports.check_free(self.metrics_port)?;
        let mut args = vec![
            "tunnel".to_owned(),
            "--no-autoupdate".into(),
            "--metrics".into(),
            format!("127.0.0.1:{}", self.metrics_port),
            "run".into(),
        ];
        args.extend(token_arguments);
        let home = self.control.parent().unwrap_or(Path::new("")).to_path_buf();
        let spec = SpawnSpec {
            program: self.binary.clone(),
            args,
            env: child_env(&home),
            capture_output: false,
        };
        self.process = Some(self.spawner.spawn(&spec)?);
        Ok(())
    }

    pub fn step(&mut self) -> Res<()> {
        let desired = read_desired(&self.control);
        if desired != self.desired {
            self.stop();
            self.desired = desired.clone();
            self.retry_at = 0.0;
            self.delay = 1.0;
        }
        if !desired.enabled {
            self.stop();
            safe_status(
                &self.status,
                status_map!("state" => "disabled", "checked_at" => self.clock.wall()),
            )?;
            return Ok(());
        }
        if self.exited() {
            self.process = None;
            self.retry_at = self.clock.monotonic() + self.delay;
            self.delay = (self.delay * 2.0).min(30.0);
        }
        if self.process.is_none() && self.clock.monotonic() >= self.retry_at {
            if let Err(error) = self.start() {
                match error {
                    RemoteError::Io(_) => {
                        if self.token_provider.is_some() {
                            let _ = std::fs::remove_file(&self.token_path);
                        }
                        self.retry_at = self.clock.monotonic() + self.delay;
                        self.delay = (self.delay * 2.0).min(30.0);
                    }
                    other => return Err(other),
                }
            }
        }
        let mut healthy = false;
        if self.process.is_some() {
            let client = HttpClient::new(
                &format!("http://127.0.0.1:{}", self.metrics_port),
                1.0,
                self.http.clone(),
            );
            healthy = client.get("/ready").is_ok_and(|r| r.status == 200);
        }
        healthy = healthy && self.process.is_some() && !self.exited();
        if healthy {
            self.delay = 1.0;
        }
        let pid = self.pid();
        safe_status(
            &self.status,
            status_map!(
                "state" => if healthy { "connected" } else { "reconnecting" },
                "checked_at" => self.clock.wall(),
                "pid" => pid,
                "generation" => desired.generation,
            ),
        )
    }
}

impl Drop for Connector {
    fn drop(&mut self) {
        if let Some(mut child) = self.process.take() {
            stop_child(&mut child);
        }
    }
}

/// One iteration of the stand-alone supervisor loop (`run()` body). Returns after one pass.
pub fn supervisor_pass(
    connector: &mut Connector,
    quick: &mut QuickConnector,
    feature: &RemoteFeature,
    binary: &Path,
    control_file: &Path,
    status_file: &Path,
    clock: &dyn Clock,
) {
    let result: Res<()> = (|| {
        let desired = read_desired(control_file);
        let intent = feature.intent();
        let named = !desired.public_origin.is_empty();
        if feature.intent_path.exists() && intent.enabled && !named {
            connector.stop();
            quick.step()?;
            safe_status(
                status_file,
                status_map!(
                    "state" => if !feature.intent().enabled { "disabled" } else { "reconnecting" },
                    "checked_at" => clock.wall(),
                ),
            )?;
        } else {
            quick.stop();
            connector.step()?;
            if feature.intent_path.exists() {
                let mut state = "off".to_owned();
                let mut url: Option<String> = None;
                if intent.enabled {
                    state = "enabling".to_owned();
                    if desired.enabled {
                        if let Some(current) = std::fs::read_to_string(status_file)
                            .ok()
                            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                        {
                            if current.get("generation").and_then(Value::as_str)
                                == Some(desired.generation.as_str())
                            {
                                if let Some(c @ ("connected" | "reconnecting")) =
                                    current.get("state").and_then(Value::as_str)
                                {
                                    state = c.to_owned();
                                }
                            }
                        }
                        if state == "connected" {
                            url = Some(desired.public_origin.clone());
                        }
                    }
                }
                safe_status(
                    &feature.status_path,
                    status_map!(
                        "state" => state,
                        "available" => binary.is_file(),
                        "checked_at" => clock.wall(),
                        "generation" => intent.generation,
                        "url" => url,
                        "temporary_url" => false,
                    ),
                )?;
            } else {
                safe_status(
                    &feature.status_path,
                    status_map!("state" => "off", "available" => binary.is_file(), "checked_at" => clock.wall()),
                )?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if error.is_io_or_value() {
            // Bad/unwritable state cannot keep public access alive.
            connector.stop();
            quick.stop();
        }
    }
}
