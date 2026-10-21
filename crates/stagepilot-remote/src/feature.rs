//! Product intent/status for the independent Remote service (`remote_feature.py`). No account
//! credentials or provider APIs here; the supervisor alone publishes the trusted listener origin.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::error::{RemoteError, Res};
use crate::files::{
    atomic_write, create_private_dir, read_desired, urlsplit, DesiredRemote, MAX_STATE_BYTES,
};
use crate::health::number;
use crate::platform::{Clock, FileLocker, LockGuard};

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
    pub port: u16,
}

const fn default_port() -> u16 {
    8766
}

impl Default for RemoteIntent {
    fn default() -> Self {
        Self {
            enabled: false,
            generation: String::new(),
            requested_at: 0.0,
            port: 8766,
        }
    }
}

#[derive(Clone)]
pub struct RemoteFeature {
    pub control: PathBuf,
    pub intent_path: PathBuf,
    pub status_path: PathBuf,
    pub port: u16,
    locker: Arc<dyn FileLocker>,
    clock: Arc<dyn Clock>,
}

fn sibling(path: &Path, name: &str) -> PathBuf {
    path.with_file_name(name)
}

/// Validate a `https://host` URL without port, path, query, fragment or credentials.
fn is_plain_https_url(candidate: &str) -> bool {
    let p = urlsplit(candidate);
    p.scheme == "https"
        && p.hostname.is_some()
        && !p.username
        && !p.password
        && p.path.is_empty()
        && !p.query
        && !p.fragment
        && p.port.is_none()
        && !p.port_invalid
}

impl RemoteFeature {
    pub fn new(
        control: &Path,
        port: u16,
        locker: Arc<dyn FileLocker>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            control: control.to_path_buf(),
            intent_path: sibling(control, "access-request.json"),
            status_path: sibling(control, "access-status.json"),
            port,
            locker,
            clock,
        }
    }

    pub fn locked(&self) -> Res<Box<dyn LockGuard>> {
        if let Some(parent) = self.control.parent() {
            create_private_dir(parent)?;
        }
        Ok(self
            .locker
            .acquire(&sibling(&self.control, "access.lock"))?)
    }

    pub fn intent(&self) -> RemoteIntent {
        let read = || -> Res<RemoteIntent> {
            if std::fs::metadata(&self.intent_path)?.len() > MAX_STATE_BYTES {
                return Ok(RemoteIntent::default());
            }
            let intent: RemoteIntent =
                serde_json::from_str(&std::fs::read_to_string(&self.intent_path)?)?;
            if intent.port < 1024 {
                return Err(RemoteError::value("port out of range"));
            }
            Ok(intent)
        };
        read().unwrap_or_default()
    }

    fn write_intent(&self, intent: &RemoteIntent) -> Res<()> {
        atomic_write(&self.intent_path, &serde_json::to_string(intent)?)
    }

    fn disabled_marker(existing: &DesiredRemote) -> DesiredRemote {
        // Preserve the non-secret stable-origin marker used by the named-tunnel control plane.
        if existing.public_origin.is_empty() {
            DesiredRemote::default()
        } else {
            DesiredRemote::marker(&existing.public_origin, existing.port)
        }
    }

    pub fn set_enabled(&self, enabled: bool) -> Res<()> {
        let _guard = self.locked()?;
        let current = self.intent();
        if enabled && current.enabled {
            return Ok(()); // Lost-response retries do not create another tunnel/session epoch.
        }
        let existing = read_desired(&self.control);
        if enabled {
            // Do not adopt/replace externally managed named or manually enabled routes.
            if existing.enabled {
                return Err(RemoteError::value("Remote is already managed elsewhere"));
            }
        } else {
            atomic_write(&self.control, &Self::disabled_marker(&existing).to_json())?;
        }
        self.write_intent(&RemoteIntent {
            enabled,
            generation: Uuid::new_v4().to_string(),
            requested_at: self.clock.wall(),
            port: self.port,
        })
    }

    /// Persist named-Remote intent using the control-plane generation.
    pub fn set_managed_enabled(&self, enabled: bool, generation: &str) -> Res<()> {
        if enabled {
            Uuid::parse_str(generation).map_err(|e| RemoteError::value(e.to_string()))?;
        }
        let _guard = self.locked()?;
        let existing = read_desired(&self.control);
        if !enabled {
            atomic_write(&self.control, &Self::disabled_marker(&existing).to_json())?;
        }
        self.write_intent(&RemoteIntent {
            enabled,
            generation: if enabled {
                generation.to_owned()
            } else {
                Uuid::new_v4().to_string()
            },
            requested_at: self.clock.wall(),
            port: existing.port,
        })
    }

    pub fn status(&self) -> Map<String, Value> {
        let intent = self.intent();
        let now = self.clock.wall();
        // Any read/parse/float failure yields ({}, stale), as Python's broad except does.
        let read = || -> Option<(Value, bool)> {
            if std::fs::metadata(&self.status_path).ok()?.len() > MAX_STATE_BYTES {
                return None;
            }
            let value: Value =
                serde_json::from_str(&std::fs::read_to_string(&self.status_path).ok()?).ok()?;
            if !value.is_object() {
                return None;
            }
            let checked = number(Some(value.get("checked_at").unwrap_or(&json!(0))))?;
            let fresh = (0.0..10.0).contains(&(now - checked));
            Some((value, fresh))
        };
        let (status, fresh) = read().unwrap_or_else(|| (json!({}), false));
        let available = fresh && status.get("available") == Some(&Value::Bool(true));
        let mut state = "off".to_owned();
        let mut url: Option<String> = None;
        let mut message: Option<&str> = None;
        if intent.enabled {
            state = "enabling".to_owned();
            if !fresh
                || status.get("generation").and_then(Value::as_str)
                    != Some(intent.generation.as_str())
            {
                if now - intent.requested_at > 15.0 {
                    state = "error".to_owned();
                }
            } else {
                state = status
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("error")
                    .to_owned();
                if !["enabling", "connected", "reconnecting", "error"].contains(&state.as_str()) {
                    state = "error".to_owned();
                }
                if state == "connected" {
                    if let Some(candidate) = status.get("url").and_then(Value::as_str) {
                        // Validate syntax without exposing arbitrary text from service output.
                        if is_plain_https_url(candidate) {
                            url = Some(candidate.to_owned());
                        } else {
                            state = "error".to_owned();
                        }
                    }
                    if state == "connected" && url.is_none() {
                        state = "error".to_owned();
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
            message = Some("Remote Access is unavailable on this installation. Local StagePilot is unaffected.");
        }
        let temporary_url = status.get("temporary_url") != Some(&Value::Bool(false));
        let Value::Object(map) = json!({
            "available": available,
            "enabled": intent.enabled,
            "state": state,
            "url": url,
            "message": message,
            "temporary_url": temporary_url,
        }) else {
            unreachable!()
        };
        map
    }
}
