//! Start-up pruning of the backend log generations (m9).
//!
//! `rotate_backend_log` only rotates when the active file reaches 5 MB, and it only knows two
//! generations. A Mac that ran the Python backend can carry `stagepilot-backend.log.1` (503 MB) and
//! `.log.2` (224 MB) that no rotation ever touches. This keeps the total of the active file and its
//! numbered generations under a cap by deleting the oldest generations first.
//!
//! Std only, so it can be tested without building the desktop shell.

use std::fs;
use std::path::{Path, PathBuf};

/// Total size the backend log and its generations may take up together.
pub const BACKEND_LOG_TOTAL_CAP: u64 = 100 * 1024 * 1024;

/// The numbered generations of `active` (`<name>.1`, `<name>.2`, ...) with their sizes, newest
/// (lowest number) first.
fn generations(active: &Path) -> Vec<(u32, PathBuf, u64)> {
    let (Some(directory), Some(name)) = (active.parent(), active.file_name()) else {
        return Vec::new();
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let mut found: Vec<(u32, PathBuf, u64)> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let number: u32 = file_name.strip_prefix(&prefix)?.parse().ok()?;
            let metadata = entry.metadata().ok()?;
            metadata
                .is_file()
                .then(|| (number, entry.path(), metadata.len()))
        })
        .collect();
    found.sort_by_key(|(number, _, _)| *number);
    found
}

/// Delete the oldest generations of `active` until the active file plus the remaining
/// generations fit in `cap` bytes. The active file is never deleted, even if it alone is over the
/// cap. Returns the deleted paths, oldest first.
pub fn prune_backend_logs(active: &Path, cap: u64) -> Vec<PathBuf> {
    let mut kept = generations(active);
    let mut total: u64 = fs::metadata(active).map_or(0, |m| m.len())
        + kept.iter().map(|(_, _, size)| *size).sum::<u64>();
    let mut deleted = Vec::new();
    while total > cap {
        let Some((_, path, size)) = kept.pop() else {
            break;
        };
        if fs::remove_file(&path).is_ok() {
            total -= size;
            deleted.push(path);
        }
    }
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "sp-log-prune-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Dir(path)
        }
        fn file(&self, name: &str, size: usize) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, vec![b'x'; size]).unwrap();
            path
        }
        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_python_era_logs_are_pruned_oldest_first() {
        // Proportions of the live Mac: 503 MB and 224 MB generations, scaled to 1 KB per MB.
        let dir = Dir::new("python-era");
        let active = dir.file("stagepilot-backend.log", 4);
        dir.file("stagepilot-backend.log.1", 503);
        dir.file("stagepilot-backend.log.2", 224);
        let deleted = prune_backend_logs(&active, 100);
        assert_eq!(
            deleted
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["stagepilot-backend.log.2", "stagepilot-backend.log.1"],
            "oldest generation (highest number) first"
        );
        assert_eq!(dir.names(), ["stagepilot-backend.log"]);
    }

    #[test]
    fn nothing_is_deleted_while_the_total_fits() {
        let dir = Dir::new("fits");
        let active = dir.file("stagepilot-backend.log", 10);
        dir.file("stagepilot-backend.log.1", 40);
        dir.file("stagepilot-backend.log.2", 50);
        assert!(prune_backend_logs(&active, 100).is_empty());
        assert_eq!(dir.names().len(), 3);
    }

    #[test]
    fn only_as_many_generations_as_needed_are_deleted_and_the_newest_survive() {
        let dir = Dir::new("partial");
        let active = dir.file("stagepilot-backend.log", 10);
        dir.file("stagepilot-backend.log.1", 40);
        dir.file("stagepilot-backend.log.2", 40);
        dir.file("stagepilot-backend.log.3", 40);
        let deleted = prune_backend_logs(&active, 100);
        assert_eq!(deleted.len(), 1);
        assert_eq!(
            dir.names(),
            [
                "stagepilot-backend.log",
                "stagepilot-backend.log.1",
                "stagepilot-backend.log.2"
            ]
        );
    }

    #[test]
    fn the_active_file_is_never_deleted_even_when_it_alone_is_over_the_cap() {
        let dir = Dir::new("active-big");
        let active = dir.file("stagepilot-backend.log", 500);
        dir.file("stagepilot-backend.log.1", 1);
        prune_backend_logs(&active, 100);
        assert_eq!(dir.names(), ["stagepilot-backend.log"]);
        assert_eq!(fs::metadata(&active).unwrap().len(), 500);
    }

    #[test]
    fn unrelated_files_and_a_missing_active_file_are_left_alone() {
        let dir = Dir::new("unrelated");
        let active = dir.0.join("stagepilot-backend.log"); // does not exist yet
        dir.file("stagepilot-backend.log.1", 300);
        dir.file("stagepilot-backend.log.bak", 300);
        dir.file("stagepilot-backend.logfile.2", 300);
        dir.file("other.log.1", 300);
        dir.file("crash-report.txt", 300);
        let deleted = prune_backend_logs(&active, 100);
        assert_eq!(deleted.len(), 1);
        assert_eq!(
            dir.names(),
            [
                "crash-report.txt",
                "other.log.1",
                "stagepilot-backend.log.bak",
                "stagepilot-backend.logfile.2"
            ]
        );
    }

    #[test]
    fn generations_sort_numerically_not_alphabetically() {
        let dir = Dir::new("numeric");
        let active = dir.file("stagepilot-backend.log", 1);
        dir.file("stagepilot-backend.log.2", 60);
        dir.file("stagepilot-backend.log.10", 60);
        let deleted = prune_backend_logs(&active, 100);
        assert_eq!(deleted[0].file_name().unwrap(), "stagepilot-backend.log.10");
        assert!(dir
            .names()
            .contains(&"stagepilot-backend.log.2".to_string()));
    }
}
