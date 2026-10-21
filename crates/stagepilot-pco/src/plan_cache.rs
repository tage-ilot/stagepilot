//! Validated, non-secret last-known-good Planning Center service cache: port of
//! `core/plan_cache.py`. Same file name and JSON shape, so a cache written by Python is read here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stagepilot_core::state::ServicePlan;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The cached service plan could not be read or safely replaced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PlanCacheError(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedServicePlan {
    #[serde(default = "one")]
    pub schema_version: u8,
    pub plan: ServicePlan,
    pub last_successful_refresh: DateTime<Utc>,
}

fn one() -> u8 {
    1
}

pub trait PlanCacheStore: Send + Sync {
    fn load(&self) -> Result<Option<CachedServicePlan>, PlanCacheError>;
    fn save(&self, cached: &CachedServicePlan) -> Result<(), PlanCacheError>;
}

/// Atomically persist one validated cached service plan (temp file, fsync, rename).
pub struct FilePlanCacheStore {
    pub path: PathBuf,
}

impl FilePlanCacheStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl PlanCacheStore for FilePlanCacheStore {
    fn load(&self) -> Result<Option<CachedServicePlan>, PlanCacheError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let invalid =
            || PlanCacheError("The last-known-good service cache is corrupt or invalid.".into());
        let text = std::fs::read_to_string(&self.path).map_err(|_| invalid())?;
        let cached: CachedServicePlan = serde_json::from_str(&text).map_err(|_| invalid())?;
        if cached.schema_version != 1 {
            return Err(invalid());
        }
        Ok(Some(cached))
    }

    fn save(&self, cached: &CachedServicePlan) -> Result<(), PlanCacheError> {
        use std::io::Write;
        let failed = || {
            PlanCacheError(
                "The last-known-good service cache could not be saved atomically.".into(),
            )
        };
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|_| failed())?;
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        let write = || -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            let mut text = serde_json::to_string_pretty(cached).unwrap_or_default();
            text.push('\n');
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &self.path)
        };
        write().map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
            failed()
        })
    }
}

/// In-memory cache for explicitly configured application instances.
#[derive(Default)]
pub struct MemoryPlanCacheStore {
    cached: Mutex<Option<CachedServicePlan>>,
}

impl MemoryPlanCacheStore {
    pub fn new(cached: Option<CachedServicePlan>) -> Self {
        Self {
            cached: Mutex::new(cached),
        }
    }
}

impl PlanCacheStore for MemoryPlanCacheStore {
    fn load(&self) -> Result<Option<CachedServicePlan>, PlanCacheError> {
        Ok(self
            .cached
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone())
    }
    fn save(&self, cached: &CachedServicePlan) -> Result<(), PlanCacheError> {
        *self.cached.lock().unwrap_or_else(|p| p.into_inner()) = Some(cached.clone());
        Ok(())
    }
}

/// `STAGEPILOT_PLAN_CACHE_PATH`, else `last-known-good-service.json` beside the settings file.
pub fn default_plan_cache_path(environ: &BTreeMap<String, String>) -> PathBuf {
    if let Some(value) = environ
        .get("STAGEPILOT_PLAN_CACHE_PATH")
        .filter(|v| !v.is_empty())
    {
        return PathBuf::from(value);
    }
    stagepilot_core::settings::default_settings_path(environ)
        .with_file_name("last-known-good-service.json")
}
