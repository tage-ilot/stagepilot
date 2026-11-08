//! The raw recording of a song, kept on disk before it is fitted, so a song that cannot be
//! saved (or saved badly) can be looked at and fitted again offline
//! (`cargo run --example refit_recording`).
//!
//! The file sits next to the settings file in `lightkey-recordings/`, one per finished song:
//! `<song key>-<UTC time>.json`. The workspace has no compression crate, so the file is plain
//! JSON; the columns are compact (an address table and `[time, address index, [values]]` per
//! change; a float may read back one bit off). The newest [`KEEP_FILES`] files are kept and at most [`MAX_TOTAL_BYTES`] in total.
//! Writing is best effort: the caller logs a failure and carries on.

use crate::lightkey_fixtures::{FixtureArg, FixtureCommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Files kept in the folder (the newest).
pub const KEEP_FILES: usize = 5;
/// The folder never holds more than this many bytes.
pub const MAX_TOTAL_BYTES: u64 = 300 * 1024 * 1024;
/// One file is never larger than this; a bigger recording is not written (and says so).
pub const MAX_FILE_BYTES: u64 = 200 * 1024 * 1024;
/// Folder name next to the settings file.
pub const FOLDER: &str = "lightkey-recordings";
const VERSION: u32 = 1;

/// What a file holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawRecording {
    pub version: u32,
    pub song_key: String,
    pub duration_seconds: f64,
    /// Every address in the file, in the order the fit sees them.
    pub addresses: Vec<String>,
    /// The state when the song started: `[address index, [values]]`.
    pub baseline: Vec<(u32, Vec<FixtureArg>)>,
    /// The changes in time order: `[seconds, address index, [values]]`.
    pub changes: Vec<(f64, u32, Vec<FixtureArg>)>,
}

impl RawRecording {
    pub fn new(
        song_key: &str,
        duration_seconds: f64,
        baseline: &[FixtureCommand],
        changes: &[(f64, FixtureCommand)],
    ) -> Self {
        let mut addresses: Vec<String> = Vec::new();
        let mut table: HashMap<String, u32> = HashMap::new();
        let mut lookup = |address: &str, addresses: &mut Vec<String>| -> u32 {
            if let Some(&i) = table.get(address) {
                return i;
            }
            let i = u32::try_from(addresses.len()).unwrap_or(u32::MAX);
            addresses.push(address.to_string());
            table.insert(address.to_string(), i);
            i
        };
        let baseline = baseline
            .iter()
            .map(|c| (lookup(&c.a, &mut addresses), c.v.clone()))
            .collect();
        let changes = changes
            .iter()
            .filter(|(at, _)| at.is_finite())
            .map(|(at, c)| (*at, lookup(&c.a, &mut addresses), c.v.clone()))
            .collect();
        Self {
            version: VERSION,
            song_key: song_key.to_string(),
            duration_seconds,
            addresses,
            baseline,
            changes,
        }
    }

    /// The baseline and the changes, as the fit takes them.
    #[allow(clippy::type_complexity)]
    pub fn into_parts(self) -> (Vec<FixtureCommand>, Vec<(f64, FixtureCommand)>) {
        let name = |i: u32| {
            self.addresses
                .get(i as usize)
                .cloned()
                .unwrap_or_default()
        };
        let baseline = self
            .baseline
            .iter()
            .map(|(i, v)| FixtureCommand::new(name(*i), v.clone()))
            .collect();
        let changes = self
            .changes
            .iter()
            .map(|(at, i, v)| (*at, FixtureCommand::new(name(*i), v.clone())))
            .collect();
        (baseline, changes)
    }
}

/// Counts the bytes written and refuses to pass a limit.
struct Limited<W: Write> {
    inner: W,
    written: u64,
    limit: u64,
}

impl<W: Write> Write for Limited<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written + buf.len() as u64 > self.limit {
            return Err(std::io::Error::other("the recording is too large to keep"));
        }
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// A file-name-safe version of a song key.
fn safe_key(key: &str) -> String {
    let cleaned: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if cleaned.is_empty() {
        "song".into()
    } else {
        cleaned
    }
}

/// Write the recording into `dir` (created if missing) and prune the folder. `now` is the UTC
/// time in the name. Returns the file.
pub fn save(
    dir: &Path,
    recording: &RawRecording,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<PathBuf, String> {
    save_limited(dir, recording, now, MAX_FILE_BYTES)
}

fn save_limited(
    dir: &Path,
    recording: &RawRecording,
    now: chrono::DateTime<chrono::Utc>,
    limit: u64,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stem = format!(
        "{}-{}",
        safe_key(&recording.song_key),
        now.format("%Y%m%dT%H%M%SZ")
    );
    let mut path = dir.join(format!("{stem}.json"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.json"));
        n += 1;
    }
    let temporary = dir.join(format!(".{stem}.tmp"));
    let result = (|| -> std::io::Result<()> {
        let file = std::fs::File::create(&temporary)?;
        let mut writer = BufWriter::new(Limited {
            inner: file,
            written: 0,
            limit,
        });
        serde_json::to_writer(&mut writer, recording).map_err(std::io::Error::other)?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        file.inner.sync_all()?;
        std::fs::rename(&temporary, &path)
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("cannot write {}: {error}", path.display()));
    }
    prune(dir, KEEP_FILES, MAX_TOTAL_BYTES, Some(&path));
    Ok(path)
}

/// Read a file written by [`save`].
pub fn load(path: &Path) -> Result<RawRecording, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let recording: RawRecording = serde_json::from_reader(std::io::BufReader::new(file))
        .map_err(|e| format!("{} is not a recording: {e}", path.display()))?;
    if recording.version != VERSION {
        return Err(format!(
            "{} is version {}, this build reads version {VERSION}",
            path.display(),
            recording.version
        ));
    }
    Ok(recording)
}

/// Keep the newest `keep` recordings (`*.json`) of `dir` and at most `max_bytes` in total; delete
/// the older ones. `protect` (the file just written) is never deleted. Returns how many were
/// deleted. A file that cannot be deleted is left.
pub fn prune(dir: &Path, keep: usize, max_bytes: u64, protect: Option<&Path>) -> usize {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf, u64)> = read
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.ends_with(".json") && !name.starts_with('.')
        })
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file()
                .then(|| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), e.path(), meta.len()))
        })
        .collect();
    // newest first; the name breaks a tie (the time is in it)
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut total = 0u64;
    let mut deleted = 0;
    for (i, (_, path, size)) in files.iter().enumerate() {
        total += size;
        let over = i >= keep || total > max_bytes;
        if over && protect != Some(path.as_path()) && std::fs::remove_file(path).is_ok() {
            total -= size;
            deleted += 1;
        }
    }
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: usize) -> RawRecording {
        let baseline = vec![FixtureCommand::new(
            "/fixture/A/overrides/dimmer",
            vec![FixtureArg::Float(0.0)],
        )];
        let changes: Vec<(f64, FixtureCommand)> = (0..n)
            .map(|i| {
                (
                    i as f64 * 0.1,
                    FixtureCommand::new(
                        format!("/fixture/{}/overrides/dimmer", i % 3),
                        vec![FixtureArg::Float(i as f64 / 100.0)],
                    ),
                )
            })
            .collect();
        RawRecording::new("song-1", 30.0, &baseline, &changes)
    }

    #[test]
    fn a_recording_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let recording = sample(50);
        let path = save(dir.path(), &recording, chrono::Utc::now()).unwrap();
        assert!(path.extension().is_some_and(|e| e == "json"));
        // numbers may differ in the last bit (serde_json reads floats without the slower exact
        // mode); everything else is equal
        let back = load(&path).unwrap();
        assert_eq!(back.addresses, recording.addresses);
        assert_eq!(back.changes.len(), recording.changes.len());
        for (a, b) in back.changes.iter().zip(&recording.changes) {
            assert!((a.0 - b.0).abs() < 1e-12 && a.1 == b.1 && a.2.len() == b.2.len());
        }
        let (baseline, changes) = recording.clone().into_parts();
        assert_eq!(baseline.len(), 1);
        assert_eq!(changes.len(), 50);
    }

    #[test]
    fn the_folder_keeps_the_newest_five() {
        let dir = tempfile::tempdir().unwrap();
        let recording = sample(5);
        let t0 = chrono::Utc::now();
        for i in 0..8 {
            let path = save(
                dir.path(),
                &recording,
                t0 + chrono::Duration::seconds(i),
            )
            .unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i as u64))
                .unwrap();
        }
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), KEEP_FILES, "{names:?}");
    }

    #[test]
    fn the_folder_stays_inside_its_byte_budget() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..4u64 {
            let path = dir.path().join(format!("old-{i}.json"));
            std::fs::write(&path, vec![b' '; 1000]).unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + i))
                .unwrap();
        }
        // room for two files: the two newest stay
        let deleted = prune(dir.path(), 5, 2_500, None);
        assert_eq!(deleted, 2);
        assert!(dir.path().join("old-3.json").exists());
        assert!(dir.path().join("old-2.json").exists());
    }

    #[test]
    fn an_unwritable_folder_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, b"x").unwrap();
        // the folder would have to be created inside a file
        assert!(save(&blocker.join("inside"), &sample(3), chrono::Utc::now()).is_err());
    }

    #[test]
    fn a_recording_over_the_file_limit_is_not_written_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let result = save_limited(dir.path(), &sample(500), chrono::Utc::now(), 100);
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
