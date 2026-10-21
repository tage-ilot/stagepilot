//! `remote_feature.py` + the parts of `remote_files.py` and `file_lock.py` it needs: the product
//! intent/status files for the independent Remote service. No account credentials, provider APIs
//! or lifecycle operations here; the supervisor alone publishes the trusted listener origin.
//!
//! The wider `remote_files` / file-lock module set belongs to the remote-modules crate (phase
//! 6b-B). This file carries only the small, private copies the routes need so the two cards do
//! not touch the same code: `atomic_write` (temp file, fsync, rename, directory fsync) and an
//! exclusive lock on `access.lock` (`std::fs::File::lock`, stable since Rust 1.89).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

/// `.stat().st_size > 4096` guards on the small private files.
const MAX_FILE_BYTES: u64 = 4096;
const DEFAULT_PORT: u32 = 8766;

#[derive(Debug)]
pub enum FeatureError {
    /// `ValueError`: the route answers 409.
    Value(String),
    /// `OSError`: the route answers 503 "Remote access storage is unavailable.".
    Io(std::io::Error),
}

impl From<std::io::Error> for FeatureError {
    fn from(error: std::io::Error) -> Self {
        FeatureError::Io(error)
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `remote_files.atomic_write`.
pub fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    make_private_dir(parent)?;
    let mut file = tempfile::Builder::new()
        .prefix(".remote-")
        .tempfile_in(parent)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    {
        // Windows cannot open a directory for fsync.
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    if dir.as_os_str().is_empty() || dir.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// `DesiredRemote` (extra fields forbidden, types strict).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesiredRemote {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub public_origin: String,
    #[serde(default = "default_port")]
    pub port: u32,
}

fn default_port() -> u32 {
    DEFAULT_PORT
}

impl Default for DesiredRemote {
    fn default() -> Self {
        Self {
            enabled: false,
            generation: String::new(),
            public_origin: String::new(),
            port: DEFAULT_PORT,
        }
    }
}

impl DesiredRemote {
    fn valid(&self) -> bool {
        if !(1024..=65535).contains(&self.port) {
            return false;
        }
        if self.enabled {
            if Uuid::parse_str(&self.generation).is_err() {
                return false;
            }
            // "an exact HTTPS origin": scheme and host (port allowed), nothing else.
            let Some(rest) = self.public_origin.strip_prefix("https://") else {
                return false;
            };
            if rest.is_empty()
                || rest.contains(['/', '?', '#', '@'])
                || Url::parse(&self.public_origin)
                    .map(|u| u.host_str().is_none())
                    .unwrap_or(true)
            {
                return false;
            }
        }
        true
    }
}

/// `read_desired`: a missing, oversized or invalid file means "nothing desired".
pub fn read_desired(path: &Path) -> DesiredRemote {
    let read = || -> Option<DesiredRemote> {
        if fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
            return None;
        }
        let parsed: DesiredRemote = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        parsed.valid().then_some(parsed)
    };
    read().unwrap_or_default()
}

/// `RemoteIntent` (strict, extra fields forbidden).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteIntent {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub requested_at: f64,
    #[serde(default = "default_port")]
    pub port: u32,
}

impl Default for RemoteIntent {
    fn default() -> Self {
        Self {
            enabled: false,
            generation: String::new(),
            requested_at: 0.0,
            port: DEFAULT_PORT,
        }
    }
}

pub struct RemoteFeature {
    pub control: PathBuf,
    pub intent_path: PathBuf,
    pub status_path: PathBuf,
    pub port: u32,
}

/// Exclusive lock on `access.lock`, released when dropped.
struct Lock(fs::File);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl RemoteFeature {
    pub fn new(control: impl Into<PathBuf>) -> Self {
        Self::with_port(control, DEFAULT_PORT)
    }

    pub fn with_port(control: impl Into<PathBuf>, port: u32) -> Self {
        let control: PathBuf = control.into();
        let sibling = |name: &str| control.with_file_name(name);
        Self {
            intent_path: sibling("access-request.json"),
            status_path: sibling("access-status.json"),
            control,
            port,
        }
    }

    fn locked(&self) -> std::io::Result<Lock> {
        make_private_dir(self.control.parent().unwrap_or_else(|| Path::new(".")))?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.control.with_file_name("access.lock"))?;
        file.lock()?;
        Ok(Lock(file))
    }

    pub fn intent(&self) -> RemoteIntent {
        let read = || -> Option<RemoteIntent> {
            if fs::metadata(&self.intent_path).ok()?.len() > MAX_FILE_BYTES {
                return None;
            }
            let intent: RemoteIntent =
                serde_json::from_str(&fs::read_to_string(&self.intent_path).ok()?).ok()?;
            (1024..=65535).contains(&intent.port).then_some(intent)
        };
        read().unwrap_or_default()
    }

    fn write_intent(&self, intent: &RemoteIntent) -> std::io::Result<()> {
        atomic_write(
            &self.intent_path,
            &serde_json::to_string(intent).unwrap_or_default(),
        )
    }

    fn write_marker(&self, existing: &DesiredRemote) -> std::io::Result<()> {
        // Preserve the non-secret stable-origin marker used by the external named-tunnel
        // control plane.
        let marker = if existing.public_origin.is_empty() {
            DesiredRemote::default()
        } else {
            DesiredRemote {
                public_origin: existing.public_origin.clone(),
                port: existing.port,
                ..DesiredRemote::default()
            }
        };
        atomic_write(
            &self.control,
            &serde_json::to_string(&marker).unwrap_or_default(),
        )
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), FeatureError> {
        let _lock = self.locked()?;
        let current = self.intent();
        if enabled && current.enabled {
            return Ok(()); // Lost-response retries do not create another tunnel/session epoch.
        }
        let existing = read_desired(&self.control);
        if enabled {
            // Do not adopt/replace externally managed named or manually enabled routes.
            if existing.enabled {
                return Err(FeatureError::Value(
                    "Remote is already managed elsewhere".into(),
                ));
            }
        } else {
            self.write_marker(&existing)?;
        }
        self.write_intent(&RemoteIntent {
            enabled,
            generation: Uuid::new_v4().to_string(),
            requested_at: now(),
            port: self.port,
        })?;
        Ok(())
    }

    /// Persist named-Remote intent using the control-plane generation.
    pub fn set_managed_enabled(&self, enabled: bool, generation: &str) -> Result<(), FeatureError> {
        if enabled && Uuid::parse_str(generation).is_err() {
            return Err(FeatureError::Value("badly formed generation".into()));
        }
        let _lock = self.locked()?;
        let existing = read_desired(&self.control);
        if !enabled {
            self.write_marker(&existing)?;
        }
        self.write_intent(&RemoteIntent {
            enabled,
            generation: if enabled {
                generation.to_string()
            } else {
                Uuid::new_v4().to_string()
            },
            requested_at: now(),
            port: existing.port,
        })?;
        Ok(())
    }

    /// The product status object. `needs_operator` and friends are added by the route.
    pub fn status(&self) -> Value {
        let intent = self.intent();
        let status: Value = (|| -> Option<Value> {
            if fs::metadata(&self.status_path).ok()?.len() > MAX_FILE_BYTES {
                return None;
            }
            serde_json::from_str(&fs::read_to_string(&self.status_path).ok()?).ok()
        })()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
        let checked_at = status.get("checked_at").and_then(Value::as_f64);
        let fresh = checked_at.is_some_and(|at| {
            let age = now() - at;
            (0.0..10.0).contains(&age)
        });
        let available = fresh && status.get("available") == Some(&Value::Bool(true));
        let mut state = "off".to_string();
        let mut url: Option<String> = None;
        let mut message: Option<&str> = None;
        if intent.enabled {
            state = "enabling".into();
            let same_generation = status.get("generation").and_then(Value::as_str)
                == Some(intent.generation.as_str());
            if !fresh || !same_generation {
                if now() - intent.requested_at > 15.0 {
                    state = "error".into();
                }
            } else {
                state = status
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("error")
                    .to_string();
                if !["enabling", "connected", "reconnecting", "error"].contains(&state.as_str()) {
                    state = "error".into();
                }
                if state == "connected" {
                    if let Some(candidate) = status.get("url").and_then(Value::as_str) {
                        // Validate syntax without exposing arbitrary text from service output.
                        if exact_https_origin(candidate) {
                            url = Some(candidate.to_string());
                        } else {
                            state = "error".into();
                        }
                    }
                    if url.is_none() {
                        state = "error".into();
                    }
                }
            }
            if state == "error" {
                message =
                    Some("Remote connection is unavailable. Check your connection or try again.");
            } else if state == "reconnecting" {
                message = Some("Reconnecting Remote Access. Local StagePilot is unaffected.");
            }
        } else if !available {
            message = Some(
                "Remote Access is unavailable on this installation. Local StagePilot is unaffected.",
            );
        }
        json!({
            "available": available,
            "enabled": intent.enabled,
            "state": state,
            "url": url,
            "message": message,
            "temporary_url": status.get("temporary_url") != Some(&Value::Bool(false)),
        })
    }
}

/// `urlsplit` check in `RemoteFeature.status`: https, a host, nothing else (no port either).
fn exact_https_origin(candidate: &str) -> bool {
    let Ok(parsed) = Url::parse(candidate) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.path() == "/"
        && !candidate.ends_with('/')
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && candidate.matches(':').count() == 1
}

/// `safe_status`: write the supervisor's status file (used by tests and the contract fake).
pub fn write_status(path: &Path, values: &Value) -> std::io::Result<()> {
    atomic_write(path, &values.to_string())
}
