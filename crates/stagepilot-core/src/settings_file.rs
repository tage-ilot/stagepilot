//! The settings file: loading that never destroys data, and atomic saving (M4).
//!
//! In rc.4 any unknown key, newer schema or damaged file made the whole file fall back to the
//! built-in defaults with only an API warning; the next save then overwrote the file and nothing
//! was logged. The rules here:
//!
//! * **Unknown keys are kept.** They are removed before validation, remembered with their path,
//!   written back on every save, and listed once in a WARN (paths only, never values). The legacy
//!   `release_channel` is still dropped silently.
//! * **An unreadable or invalid file, or one from a newer StagePilot, is quarantined.** The
//!   original is copied to `settings.json.corrupt-<UTC timestamp>` (the last five are kept), an
//!   ERROR `settings_corrupt` names the path, the app starts on defaults and reports
//!   `settings_degraded`. A file from a newer schema is never overwritten.
//! * **Loading never writes.** A migrated file reaches the disk with the next save.
//! * The first overwrite is still copied once to `settings.json.pre-rust.bak`.

use crate::settings::{
    PersistentSettings, SettingsFileError, SettingsStore, BACKUP_SUFFIX, SETTINGS_SCHEMA_VERSION,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const CORRUPT: &str = "Saved settings are corrupt or invalid; built-in defaults were used.";
const NEWER: &str = "Saved settings were written by a newer version of StagePilot; built-in defaults are used and the file is left untouched.";
const PROTECTED: &str = "Settings were not saved: the settings file on disk was written by a newer version of StagePilot (or could not be preserved) and is not overwritten.";
/// How many `settings.json.corrupt-*` copies are kept.
pub const QUARANTINE_KEEP: usize = 5;

/// Why the saved settings are not in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsDegraded {
    /// A short, value-free reason (`not valid JSON`, `schema_version 3 is newer than 2`, ...).
    pub reason: String,
    /// The preserved copy of the original file, when one was made.
    pub quarantine: Option<PathBuf>,
    /// Saving is refused: the file on disk is newer than this build or could not be preserved.
    pub write_protected: bool,
}

#[derive(Default)]
struct Memory {
    /// Keys the typed settings do not know: `(path, value)`, written back on save.
    extras: Vec<(Vec<Value>, Value)>,
    degraded: Option<SettingsDegraded>,
}

pub struct SettingsFileStore {
    pub path: PathBuf,
    backed_up: Mutex<bool>,
    memory: Mutex<Memory>,
}

fn guard<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// `a.b.0.c` for logs.
pub fn dotted(path: &[Value]) -> String {
    path.iter()
        .map(|p| match p {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

impl SettingsFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            backed_up: Mutex::new(false),
            memory: Mutex::new(Memory::default()),
        }
    }

    pub fn backup_path(&self) -> PathBuf {
        self.sibling(&format!(".{BACKUP_SUFFIX}"))
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(suffix);
        self.path.with_file_name(name)
    }

    /// The quarantine copies next to the settings file, oldest first.
    pub fn quarantine_files(&self) -> Vec<PathBuf> {
        let prefix = self
            .sibling(".corrupt-")
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let dir = if dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            dir
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
            })
            .collect();
        files.sort();
        files
    }

    /// Copy the original aside (reusing an identical earlier copy) and keep the newest five.
    fn quarantine(&self, original: &[u8]) -> Option<PathBuf> {
        let existing = self.quarantine_files();
        if let Some(same) = existing
            .iter()
            .find(|p| std::fs::read(p).is_ok_and(|b| b == original))
        {
            return Some(same.clone());
        }
        // Names sort oldest-first, so a new copy must sort after every existing one even when the
        // clock repeats a timestamp or an older copy was pruned.
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string();
        let mut target = self.sibling(&format!(".corrupt-{stamp}"));
        if let Some(newest) = existing.last() {
            if target <= *newest {
                target = PathBuf::from(format!("{}-1", newest.display()));
            }
        }
        while target.exists() {
            target = PathBuf::from(format!("{}-1", target.display()));
        }
        if std::fs::write(&target, original).is_err() {
            return None;
        }
        let files = self.quarantine_files();
        for old in files
            .iter()
            .take(files.len().saturating_sub(QUARANTINE_KEEP))
        {
            let _ = std::fs::remove_file(old);
        }
        Some(target)
    }

    /// Refuse the file: preserve it, log, remember, and tell the caller to use defaults.
    fn reject(&self, original: &[u8], reason: &str, newer: bool) -> SettingsFileError {
        let quarantine = self.quarantine(original);
        tracing::error!(
            path = %self.path.display(),
            reason,
            quarantine = %quarantine
                .as_ref()
                .map_or_else(|| "none (copy failed)".to_string(), |p| p.display().to_string()),
            "settings_corrupt"
        );
        let mut memory = guard(&self.memory);
        memory.extras.clear();
        memory.degraded = Some(SettingsDegraded {
            reason: reason.to_string(),
            write_protected: newer || quarantine.is_none(),
            quarantine,
        });
        SettingsFileError(if newer { NEWER } else { CORRUPT }.into())
    }

    /// Copy the existing file aside once, before the first write of this process, unless a backup
    /// already exists (an older backup is never overwritten).
    fn backup_once(&self) -> Result<(), SettingsFileError> {
        let mut done = guard(&self.backed_up);
        if *done {
            return Ok(());
        }
        *done = true;
        let backup = self.backup_path();
        if self.path.is_file() && !backup.exists() {
            std::fs::copy(&self.path, &backup)
                .map_err(|_| SettingsFileError("Settings could not be saved atomically.".into()))?;
        }
        Ok(())
    }
}

/// Remove the keys the typed settings reject (`extra_forbidden`), one validation round at a time.
/// `None` means a different error: the file is invalid. Returns the settings and the migration.
fn strip_unknown(
    payload: &mut Value,
    extras: &mut Vec<(Vec<Value>, Value)>,
) -> Result<(PersistentSettings, Option<crate::settings::Migration>), String> {
    for _ in 0..10_000 {
        match PersistentSettings::from_value(payload.clone()) {
            Ok(parsed) => return Ok(parsed),
            Err(errors) => {
                let mut removed = false;
                for error in errors.iter().filter(|e| e.kind == "extra_forbidden") {
                    if let Some(value) = take_path(payload, &error.loc) {
                        extras.push((error.loc.clone(), value));
                        removed = true;
                    }
                }
                if !removed {
                    // Locations and error kinds only: the input values stay out of the log.
                    let first = errors.first();
                    return Err(format!(
                        "invalid value at {} ({})",
                        first.map_or(String::new(), |e| dotted(&e.loc)),
                        first.map_or("unknown", |e| e.kind.as_str())
                    ));
                }
            }
        }
    }
    Err("too many unknown keys".into())
}

fn take_path(root: &mut Value, path: &[Value]) -> Option<Value> {
    let (last, parents) = path.split_last()?;
    let mut node = root;
    for step in parents {
        node = step_into(node, step)?;
    }
    match (node, last) {
        (Value::Object(map), Value::String(key)) => map.remove(key),
        _ => None,
    }
}

fn step_into<'a>(node: &'a mut Value, step: &Value) -> Option<&'a mut Value> {
    match (node, step) {
        (Value::Object(map), Value::String(key)) => map.get_mut(key),
        (Value::Array(items), Value::Number(n)) => {
            items.get_mut(usize::try_from(n.as_u64()?).ok()?)
        }
        _ => None,
    }
}

/// Keys in `original` that `typed` (the same settings serialised back) no longer has: fields the
/// model silently ignores. The legacy top-level `release_channel` is dropped on purpose.
fn ignored_keys(
    original: &Value,
    typed: &Value,
    path: &mut Vec<Value>,
    out: &mut Vec<(Vec<Value>, Value)>,
) {
    match (original, typed) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                if path.is_empty() && key == "release_channel" {
                    continue;
                }
                path.push(Value::String(key.clone()));
                match b.get(key) {
                    None => out.push((path.clone(), value.clone())),
                    Some(t) => ignored_keys(value, t, path, out),
                }
                path.pop();
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                path.push(Value::from(index));
                ignored_keys(x, y, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn put_path(root: &mut Value, path: &[Value], value: &Value) {
    let Some((Value::String(key), parents)) = path.split_last() else {
        return;
    };
    let mut node = root;
    for step in parents {
        match step_into(node, step) {
            Some(next) => node = next,
            None => return,
        }
    }
    if let Value::Object(map) = node {
        map.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

impl SettingsStore for SettingsFileStore {
    fn path(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }

    fn degraded(&self) -> Option<SettingsDegraded> {
        guard(&self.memory).degraded.clone()
    }

    fn load(&self) -> Result<Option<PersistentSettings>, SettingsFileError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                // Cannot read it, so cannot copy it either: never overwrite what we cannot see.
                tracing::error!(
                    path = %self.path.display(),
                    reason = %error.kind(),
                    "settings_corrupt"
                );
                guard(&self.memory).degraded = Some(SettingsDegraded {
                    reason: format!("unreadable ({})", error.kind()),
                    quarantine: None,
                    write_protected: true,
                });
                return Err(SettingsFileError(CORRUPT.into()));
            }
        };
        // Windows editors add a UTF-8 byte-order mark; it is valid, just not part of the JSON.
        let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        let Ok(text) = std::str::from_utf8(body) else {
            return Err(self.reject(&bytes, "not valid UTF-8 text", false));
        };
        let mut payload: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => {
                return Err(self.reject(&bytes, "not valid JSON (empty or truncated?)", false))
            }
        };
        let Value::Object(map) = &mut payload else {
            return Err(self.reject(&bytes, "not a JSON object", false));
        };
        match map.get("schema_version") {
            None => {
                map.insert("schema_version".into(), serde_json::json!(1));
            }
            Some(version) => {
                if let Some(n) = version
                    .as_u64()
                    .filter(|n| *n > u64::from(SETTINGS_SCHEMA_VERSION))
                {
                    let reason =
                        format!("schema_version {n} is newer than {SETTINGS_SCHEMA_VERSION}");
                    return Err(self.reject(&bytes, &reason, true));
                }
            }
        }
        let mut extras = Vec::new();
        let (settings, migration) = match strip_unknown(&mut payload, &mut extras) {
            Ok(parsed) => parsed,
            Err(reason) => return Err(self.reject(&bytes, &reason, false)),
        };
        // Fields the model ignores without complaint are preserved too.
        ignored_keys(
            &payload,
            &settings.to_file_value(),
            &mut Vec::new(),
            &mut extras,
        );
        if !extras.is_empty() {
            let paths: Vec<String> = extras.iter().map(|(p, _)| dotted(p)).collect();
            tracing::warn!(
                path = %self.path.display(),
                keys = %paths.join(", "),
                "settings_unknown_keys kept and written back on save"
            );
        }
        if let Some(m) = migration {
            // In memory only: the file is rewritten by the next save (which first keeps the
            // original as settings.json.pre-rust.bak).
            tracing::warn!(
                "settings_migration schema={}->2 chose_midi={} midi_source={:?}",
                m.from_schema,
                m.chose_midi,
                m.midi_source
            );
        }
        let mut memory = guard(&self.memory);
        memory.extras = extras;
        memory.degraded = None;
        Ok(Some(settings))
    }

    fn save(&self, settings: &PersistentSettings) -> Result<(), SettingsFileError> {
        use std::io::Write;
        let failure = || SettingsFileError("Settings could not be saved atomically.".into());
        if guard(&self.memory)
            .degraded
            .as_ref()
            .is_some_and(|d| d.write_protected)
        {
            return Err(SettingsFileError(PROTECTED.into()));
        }
        self.backup_once()?;
        let parent = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "settings.json".into());
        let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        let mut value = settings.to_file_value();
        for (path, extra) in &guard(&self.memory).extras {
            put_path(&mut value, path, extra);
        }
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&parent)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            let mut text = serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?;
            text.push('\n');
            file.write_all(text.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(failure());
        }
        Ok(())
    }
}
