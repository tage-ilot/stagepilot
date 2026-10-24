//! Leftover `cloudflared` children (B2).
//!
//! A SIGKILL, crash or power cut of StagePilot leaves its `cloudflared` child running, still bound
//! to the metrics port. The next launch could neither start its own connector nor use the old one,
//! and stayed `reconnecting` until someone killed the orphan by hand.
//!
//! The fix has three parts, all here:
//! * a pidfile (pid, process start time, exe path, desired generation) written when a child is
//!   spawned and removed when it is stopped;
//! * at start, [`reap`] looks at the pidfile: a live process whose exe is the bundled
//!   `cloudflared` (and whose start time still matches) is either adopted by the connector or
//!   terminated; a dead process or a recycled pid belonging to anything else is left alone;
//! * [`ProcessInspector::exit_with_parent`] makes a new child exit when StagePilot dies.
//!
//! Inspection and signalling sit behind [`ProcessInspector`] so every case is testable with
//! [`FakeInspector`] and no real process.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::files::atomic_write;
use crate::platform::ChildProcess;

/// What the OS says about a running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    /// The executable the process runs, when the OS lets us read it.
    pub exe: Option<PathBuf>,
    /// An opaque, stable per-process start marker (equal for the same process, different for a
    /// process that later reuses the pid). `None` when the platform cannot tell.
    pub started_at: Option<u64>,
}

pub trait ProcessInspector: Send + Sync {
    /// `None` when no such process is running.
    fn info(&self, pid: u32) -> Option<ProcessInfo>;
    /// Ask the process to exit (SIGTERM / `taskkill`).
    fn terminate(&self, pid: u32);
    /// Force it to exit.
    fn kill(&self, pid: u32);
    /// Arrange for `child` to be terminated if the current process dies without cleaning up.
    /// Best effort; a platform without support does nothing.
    fn exit_with_parent(&self, child: u32);
}

// ---------------------------------------------------------------- pidfile

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PidRecord {
    pub pid: u32,
    #[serde(default)]
    pub started_at: Option<u64>,
    pub exe: String,
    /// The desired-Remote generation the child was started for.
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub metrics_port: u16,
}

pub fn read_pidfile(path: &Path) -> Option<PidRecord> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn write_pidfile(path: &Path, record: &PidRecord) {
    // Best effort: failing to write it only loses the orphan protection.
    if let Ok(text) = serde_json::to_string(record) {
        let _ = atomic_write(path, &text);
    }
}

pub fn remove_pidfile(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Record a freshly spawned child.
pub fn record_child(
    path: &Path,
    inspector: &dyn ProcessInspector,
    binary: &Path,
    pid: u32,
    generation: &str,
    metrics_port: u16,
) {
    let info = inspector.info(pid);
    write_pidfile(
        path,
        &PidRecord {
            pid,
            started_at: info.and_then(|i| i.started_at),
            exe: binary.to_string_lossy().into_owned(),
            generation: generation.to_owned(),
            metrics_port,
        },
    );
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Whether the process described by `info` is the bundled connector the pidfile `record` was
/// written for. Anything unreadable counts as "not ours": a stranger must never be signalled.
fn is_our_connector(record: &PidRecord, info: &ProcessInfo, binary: &Path) -> bool {
    let Some(exe) = &info.exe else {
        return false;
    };
    // A binary replaced by an update shows as "(deleted)" on Linux; the name still identifies it.
    let exe_text = exe.to_string_lossy();
    let exe_clean = Path::new(exe_text.trim_end_matches(" (deleted)")).to_path_buf();
    let exe_matches = same_file(&exe_clean, binary)
        || (cfg!(windows)
            && exe_clean.file_name().is_some()
            && exe_clean.file_name() == binary.file_name());
    if !exe_matches {
        return false;
    }
    // A recycled pid that happens to run the same binary (another StagePilot) has a different
    // start marker.
    match (record.started_at, info.started_at) {
        (Some(recorded), Some(current)) => recorded == current,
        _ => true,
    }
}

// ---------------------------------------------------------------- reaping

#[derive(Debug, PartialEq, Eq)]
pub enum Reaped {
    /// No pidfile, or its process is gone or belongs to something else (the pidfile is removed).
    Nothing,
    /// The recorded connector is still running and was left running for the caller to adopt.
    Alive(PidRecord),
    /// The recorded connector was running and has been terminated.
    Terminated(PidRecord),
}

/// Deal with the connector a previous run left behind. With `keep_if` returning true for a live
/// orphan it is left running (`Reaped::Alive`); otherwise it is terminated.
pub fn reap(
    pidfile: &Path,
    binary: &Path,
    inspector: &dyn ProcessInspector,
    keep_if: &dyn Fn(&PidRecord) -> bool,
) -> Reaped {
    let Some(record) = read_pidfile(pidfile) else {
        remove_pidfile(pidfile);
        return Reaped::Nothing;
    };
    let ours = inspector
        .info(record.pid)
        .is_some_and(|info| is_our_connector(&record, &info, binary));
    if !ours {
        remove_pidfile(pidfile);
        return Reaped::Nothing;
    }
    if keep_if(&record) {
        return Reaped::Alive(record);
    }
    terminate_and_wait(inspector, record.pid);
    remove_pidfile(pidfile);
    Reaped::Terminated(record)
}

/// SIGTERM, wait up to 10 s, then SIGKILL and wait up to 5 s (like `stop_child`).
fn terminate_and_wait(inspector: &dyn ProcessInspector, pid: u32) {
    let gone = |seconds: f64| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
        loop {
            if inspector.info(pid).is_none() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    inspector.terminate(pid);
    if !gone(10.0) {
        inspector.kill(pid);
        gone(5.0);
    }
}

// ---------------------------------------------------------------- an adopted child

/// A connector some earlier run started. It can be watched and stopped but has no output and no
/// exit code.
pub struct AdoptedChild {
    pid: u32,
    inspector: Arc<dyn ProcessInspector>,
}

impl AdoptedChild {
    pub fn new(pid: u32, inspector: Arc<dyn ProcessInspector>) -> Self {
        Self { pid, inspector }
    }
}

impl ChildProcess for AdoptedChild {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        Ok(self.inspector.info(self.pid).is_none().then_some(0))
    }

    fn terminate(&mut self) {
        self.inspector.terminate(self.pid);
    }

    fn kill(&mut self) {
        self.inspector.kill(self.pid);
    }

    fn wait_timeout(&mut self, seconds: f64) -> bool {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds.max(0.0));
        loop {
            if self.inspector.info(self.pid).is_none() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn take_output(&mut self) -> Vec<Box<dyn io::Read + Send>> {
        Vec::new()
    }
}

// ---------------------------------------------------------------- the real inspector

pub struct SystemInspector;

/// FNV-1a: a stable (cross-run) hash for start markers that are only available as text.
#[cfg(all(unix, not(target_os = "linux")))]
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(unix)]
impl SystemInspector {
    fn signal(pid: u32, signal: nix::sys::signal::Signal) {
        if let Ok(pid) = i32::try_from(pid) {
            if pid > 1 {
                let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal);
            }
        }
    }

    fn alive(pid: u32) -> bool {
        let Ok(raw) = i32::try_from(pid) else {
            return false;
        };
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(raw), None) {
            Ok(()) => true,
            // Exists, but belongs to another user.
            Err(nix::errno::Errno::EPERM) => true,
            Err(_) => false,
        }
    }

    #[cfg(target_os = "linux")]
    fn details(pid: u32) -> (Option<PathBuf>, Option<u64>) {
        let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
        // Field 22 of /proc/<pid>/stat is the start time in clock ticks since boot. The command
        // name (field 2) is parenthesised and may contain spaces, so count after the last ')'.
        let started_at = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                let rest = stat.rsplit_once(')')?.1;
                rest.split_whitespace().nth(19)?.parse::<u64>().ok()
            });
        (exe, started_at)
    }

    #[cfg(not(target_os = "linux"))]
    fn details(pid: u32) -> (Option<PathBuf>, Option<u64>) {
        let ps = |field: &str| -> Option<String> {
            let output = std::process::Command::new("/bin/ps")
                .args(["-p", &pid.to_string(), "-o", field])
                .env_clear()
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (output.status.success() && !text.is_empty()).then_some(text)
        };
        // On macOS `comm` is the executable's full path.
        let exe = ps("comm=").map(PathBuf::from);
        let started_at = ps("lstart=").map(|text| fnv1a(&text));
        (exe, started_at)
    }
}

#[cfg(unix)]
impl ProcessInspector for SystemInspector {
    fn info(&self, pid: u32) -> Option<ProcessInfo> {
        if !Self::alive(pid) {
            return None;
        }
        let (exe, started_at) = Self::details(pid);
        // A zombie has already exited; only its parent has yet to collect it.
        #[cfg(target_os = "linux")]
        if std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| stat.rsplit_once(')')?.1.trim().chars().next())
            == Some('Z')
        {
            return None;
        }
        Some(ProcessInfo { exe, started_at })
    }

    fn terminate(&self, pid: u32) {
        Self::signal(pid, nix::sys::signal::Signal::SIGTERM);
    }

    fn kill(&self, pid: u32) {
        Self::signal(pid, nix::sys::signal::Signal::SIGKILL);
    }

    fn exit_with_parent(&self, child: u32) {
        let parent = std::process::id();
        spawn_parent_watch(parent, child);
    }
}

/// A tiny detached `sh` that outlives a SIGKILLed StagePilot and then ends the child: it polls
/// both pids and, once the parent is gone while the child still runs, terminates the child. It
/// exits on its own as soon as either side is gone.
#[cfg(unix)]
pub fn spawn_parent_watch(parent: u32, child: u32) {
    use std::process::{Command, Stdio};
    const SCRIPT: &str = r#"
while kill -0 "$1" 2>/dev/null && kill -0 "$2" 2>/dev/null; do sleep 1; done
if ! kill -0 "$1" 2>/dev/null; then kill "$2" 2>/dev/null; sleep 5; kill -9 "$2" 2>/dev/null; fi
"#;
    let _ = Command::new("/bin/sh")
        .args(["-c", SCRIPT, "stagepilot-parent-watch"])
        .arg(parent.to_string())
        .arg(child.to_string())
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(windows)]
impl ProcessInspector for SystemInspector {
    fn info(&self, pid: u32) -> Option<ProcessInfo> {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let image = text.lines().find_map(|line| {
            let mut fields = line.split("\",\"");
            let name = fields.next()?.trim_start_matches('"');
            let found = fields.next()?.trim() == pid.to_string();
            found.then(|| name.to_owned())
        })?;
        // `tasklist` gives the image name only: enough to tell cloudflared from a stranger.
        Some(ProcessInfo {
            exe: Some(PathBuf::from(image)),
            started_at: None,
        })
    }

    fn terminate(&self, pid: u32) {
        self.kill(pid);
    }

    fn kill(&self, pid: u32) {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .creation_flags(0x0800_0000)
            .output();
    }

    fn exit_with_parent(&self, _child: u32) {
        // Not implemented on Windows (a job object needs Win32 calls this crate does not
        // link). The pidfile reaping at the next start still removes an orphan.
    }
}

// ---------------------------------------------------------------- the fake

/// Scripted processes for tests: nothing real is looked at or signalled.
#[derive(Default)]
pub struct FakeInspector {
    processes: Mutex<HashMap<u32, ProcessInfo>>,
    pub terminated: Mutex<Vec<u32>>,
    pub killed: Mutex<Vec<u32>>,
    pub bound_to_parent: Mutex<Vec<u32>>,
    /// Processes that ignore `terminate` and need `kill`.
    pub stubborn: Mutex<Vec<u32>>,
}

impl FakeInspector {
    pub fn add(&self, pid: u32, exe: Option<&Path>, started_at: Option<u64>) {
        self.processes.lock().unwrap().insert(
            pid,
            ProcessInfo {
                exe: exe.map(Path::to_path_buf),
                started_at,
            },
        );
    }

    pub fn is_running(&self, pid: u32) -> bool {
        self.processes.lock().unwrap().contains_key(&pid)
    }

    pub fn exit(&self, pid: u32) {
        self.processes.lock().unwrap().remove(&pid);
    }
}

impl ProcessInspector for FakeInspector {
    fn info(&self, pid: u32) -> Option<ProcessInfo> {
        self.processes.lock().unwrap().get(&pid).cloned()
    }

    fn terminate(&self, pid: u32) {
        self.terminated.lock().unwrap().push(pid);
        if !self.stubborn.lock().unwrap().contains(&pid) {
            self.exit(pid);
        }
    }

    fn kill(&self, pid: u32) {
        self.killed.lock().unwrap().push(pid);
        self.exit(pid);
    }

    fn exit_with_parent(&self, child: u32) {
        self.bound_to_parent.lock().unwrap().push(child);
    }
}
