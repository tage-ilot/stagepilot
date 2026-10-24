//! OS seams: clock, file lock, child processes and port probing. Each has a real implementation
//! and an in-memory fake so every state machine is testable without a real `cloudflared`.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::http::{HttpTransport, ReqwestTransport};

// ---------------------------------------------------------------- clock

pub trait Clock: Send + Sync {
    /// `time.time()`
    fn wall(&self) -> f64;
    /// `time.monotonic()`
    fn monotonic(&self) -> f64;
}

pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn wall(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }

    fn monotonic(&self) -> f64 {
        // Offset so a fresh clock is already past `retry_at = 0.0`, as in Python.
        self.start.elapsed().as_secs_f64() + 1_000.0
    }
}

/// A clock tests advance by hand.
pub struct ManualClock {
    wall: Mutex<f64>,
    mono: Mutex<f64>,
}

impl ManualClock {
    pub fn new(wall: f64) -> Self {
        Self {
            wall: Mutex::new(wall),
            mono: Mutex::new(1_000.0),
        }
    }

    pub fn advance(&self, seconds: f64) {
        *self.wall.lock().unwrap() += seconds;
        *self.mono.lock().unwrap() += seconds;
    }
}

impl Clock for ManualClock {
    fn wall(&self) -> f64 {
        *self.wall.lock().unwrap()
    }

    fn monotonic(&self) -> f64 {
        *self.mono.lock().unwrap()
    }
}

// ---------------------------------------------------------------- file lock

/// A held lock; released on drop.
pub trait LockGuard: Send {}

pub trait FileLocker: Send + Sync {
    /// Block until the exclusive lock on `path` is held (creating the file, 0600 on Unix).
    fn acquire(&self, path: &Path) -> io::Result<Box<dyn LockGuard>>;
}

/// Cross-platform advisory lock through `std::fs::File::lock` (flock on Unix, LockFileEx on
/// Windows), the equivalent of `file_lock.exclusive_lock`.
pub struct StdFileLocker;

struct StdGuard(std::fs::File);

impl LockGuard for StdGuard {}

impl Drop for StdGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl FileLocker for StdFileLocker {
    fn acquire(&self, path: &Path) -> io::Result<Box<dyn LockGuard>> {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        file.lock()?;
        Ok(Box::new(StdGuard(file)))
    }
}

/// In-process lock keyed by path, for tests (and for platforms without file locking).
#[derive(Default)]
pub struct InMemoryLocker {
    held: Arc<(Mutex<HashSet<PathBuf>>, Condvar)>,
}

struct MemoryGuard {
    held: Arc<(Mutex<HashSet<PathBuf>>, Condvar)>,
    path: PathBuf,
}

impl LockGuard for MemoryGuard {}

impl Drop for MemoryGuard {
    fn drop(&mut self) {
        self.held.0.lock().unwrap().remove(&self.path);
        self.held.1.notify_all();
    }
}

impl FileLocker for InMemoryLocker {
    fn acquire(&self, path: &Path) -> io::Result<Box<dyn LockGuard>> {
        let (set, cv) = &*self.held;
        let mut guard = set.lock().unwrap();
        while guard.contains(path) {
            guard = cv.wait(guard).unwrap();
        }
        guard.insert(path.to_path_buf());
        Ok(Box::new(MemoryGuard {
            held: self.held.clone(),
            path: path.to_path_buf(),
        }))
    }
}

// ---------------------------------------------------------------- child processes

#[derive(Clone)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Pipe stdout and stderr (merged by the reader) instead of discarding them.
    pub capture_output: bool,
}

/// Environment values can hold secrets (the tunnel token), so Debug shows names only.
impl std::fmt::Debug for SpawnSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnSpec")
            .field("program", &self.program)
            .field("args", &self.args)
            .field(
                "env",
                &self.env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            )
            .field("capture_output", &self.capture_output)
            .finish()
    }
}

pub trait ChildProcess: Send {
    fn pid(&self) -> u32;
    /// `Popen.poll()`: `Some(code)` once exited.
    fn try_wait(&mut self) -> io::Result<Option<i32>>;
    /// SIGTERM on Unix, TerminateProcess on Windows.
    fn terminate(&mut self);
    fn kill(&mut self);
    /// Wait for exit; `false` on timeout.
    fn wait_timeout(&mut self, seconds: f64) -> bool;
    /// Captured output streams (stdout, stderr); empty unless `capture_output`.
    fn take_output(&mut self) -> Vec<Box<dyn Read + Send>>;
}

pub trait ProcessSpawner: Send + Sync {
    fn spawn(&self, spec: &SpawnSpec) -> io::Result<Box<dyn ChildProcess>>;
}

pub struct StdSpawner;

struct StdChild {
    child: std::process::Child,
}

impl ChildProcess for StdChild {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| status.code().unwrap_or(-1)))
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            if let Ok(pid) = i32::try_from(self.child.id()) {
                let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
    }

    fn wait_timeout(&mut self, seconds: f64) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
        loop {
            if matches!(self.child.try_wait(), Ok(Some(_)) | Err(_)) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn take_output(&mut self) -> Vec<Box<dyn Read + Send>> {
        let mut streams: Vec<Box<dyn Read + Send>> = Vec::new();
        if let Some(out) = self.child.stdout.take() {
            streams.push(Box::new(out));
        }
        if let Some(err) = self.child.stderr.take() {
            streams.push(Box::new(err));
        }
        streams
    }
}

impl Drop for StdChild {
    /// Orphan cleanup when the supervisor is dropped (tokio's `kill_on_drop` equivalent).
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl ProcessSpawner for StdSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> io::Result<Box<dyn ChildProcess>> {
        use std::process::{Command, Stdio};
        let mut command = Command::new(&spec.program);
        command.args(&spec.args).env_clear().stdin(Stdio::null());
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        if spec.capture_output {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        } else {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        Ok(Box::new(StdChild {
            child: command.spawn()?,
        }))
    }
}

/// Scripted fake: records every spawn; the test controls exit and output.
#[derive(Default)]
pub struct FakeSpawner {
    inner: Arc<Mutex<FakeSpawnerState>>,
}

#[derive(Default)]
struct FakeSpawnerState {
    specs: Vec<SpawnSpec>,
    children: Vec<Arc<FakeChildState>>,
    fail_next: Option<String>,
    output: Vec<u8>,
    next_pid: u32,
    ignore_terminate: bool,
}

#[derive(Default)]
pub struct FakeChildState {
    pub exit_code: Mutex<Option<i32>>,
    pub terminated: AtomicBool,
    pub killed: AtomicBool,
    pub pid: AtomicU64,
}

impl FakeChildState {
    pub fn exit(&self, code: i32) {
        *self.exit_code.lock().unwrap() = Some(code);
    }
}

impl FakeSpawner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn specs(&self) -> Vec<SpawnSpec> {
        self.inner.lock().unwrap().specs.clone()
    }

    pub fn children(&self) -> Vec<Arc<FakeChildState>> {
        self.inner.lock().unwrap().children.clone()
    }

    pub fn fail_next(&self, message: &str) {
        self.inner.lock().unwrap().fail_next = Some(message.to_owned());
    }

    /// Bytes every captured child will emit on its output stream.
    pub fn set_output(&self, bytes: &[u8]) {
        self.inner.lock().unwrap().output = bytes.to_vec();
    }

    /// Children that never exit on SIGTERM (exercises the kill path).
    pub fn set_ignore_terminate(&self, ignore: bool) {
        self.inner.lock().unwrap().ignore_terminate = ignore;
    }
}

struct FakeChild {
    state: Arc<FakeChildState>,
    output: Vec<u8>,
    ignore_terminate: bool,
}

impl ChildProcess for FakeChild {
    fn pid(&self) -> u32 {
        self.state.pid.load(Ordering::SeqCst) as u32
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        Ok(*self.state.exit_code.lock().unwrap())
    }

    fn terminate(&mut self) {
        self.state.terminated.store(true, Ordering::SeqCst);
        if !self.ignore_terminate {
            self.state.exit(-15);
        }
    }

    fn kill(&mut self) {
        self.state.killed.store(true, Ordering::SeqCst);
        self.state.exit(-9);
    }

    fn wait_timeout(&mut self, _seconds: f64) -> bool {
        self.state.exit_code.lock().unwrap().is_some()
    }

    fn take_output(&mut self) -> Vec<Box<dyn Read + Send>> {
        vec![Box::new(io::Cursor::new(std::mem::take(&mut self.output)))]
    }
}

impl ProcessSpawner for FakeSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> io::Result<Box<dyn ChildProcess>> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(message) = inner.fail_next.take() {
            return Err(io::Error::other(message));
        }
        inner.next_pid += 1;
        let state = Arc::new(FakeChildState::default());
        state
            .pid
            .store(4000 + u64::from(inner.next_pid), Ordering::SeqCst);
        inner.specs.push(spec.clone());
        inner.children.push(state.clone());
        let output = if spec.capture_output {
            inner.output.clone()
        } else {
            Vec::new()
        };
        Ok(Box::new(FakeChild {
            state,
            output,
            ignore_terminate: inner.ignore_terminate,
        }))
    }
}

// ---------------------------------------------------------------- ports

pub trait PortProbe: Send + Sync {
    /// Ok when 127.0.0.1:`port` can be bound (nothing else is listening there).
    fn check_free(&self, port: u16) -> io::Result<()>;
}

pub struct SystemPortProbe;

impl PortProbe for SystemPortProbe {
    fn check_free(&self, port: u16) -> io::Result<()> {
        TcpListener::bind(("127.0.0.1", port)).map(drop)
    }
}

/// Fake with an explicit set of busy ports.
#[derive(Default)]
pub struct FakePortProbe {
    busy: Mutex<HashMap<u16, ()>>,
}

impl FakePortProbe {
    pub fn set_busy(&self, port: u16, busy: bool) {
        let mut map = self.busy.lock().unwrap();
        if busy {
            map.insert(port, ());
        } else {
            map.remove(&port);
        }
    }
}

impl PortProbe for FakePortProbe {
    fn check_free(&self, port: u16) -> io::Result<()> {
        if self.busy.lock().unwrap().contains_key(&port) {
            Err(io::Error::new(io::ErrorKind::AddrInUse, "address in use"))
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- bundle

/// Every OS-dependent collaborator in one clonable bundle.
#[derive(Clone)]
pub struct Platform {
    pub inspector: Arc<dyn crate::orphan::ProcessInspector>,
    pub clock: Arc<dyn Clock>,
    pub locker: Arc<dyn FileLocker>,
    pub spawner: Arc<dyn ProcessSpawner>,
    pub ports: Arc<dyn PortProbe>,
    pub http: Arc<dyn HttpTransport>,
}

impl Platform {
    pub fn system() -> Self {
        Self {
            inspector: Arc::new(crate::orphan::SystemInspector),
            clock: Arc::new(SystemClock::new()),
            locker: Arc::new(StdFileLocker),
            spawner: Arc::new(StdSpawner),
            ports: Arc::new(SystemPortProbe),
            http: Arc::new(ReqwestTransport::new()),
        }
    }
}

// ---------------------------------------------------------------- stop flag

/// Cooperative stop shared by blocking threads and async tasks (`threading.Event` /
/// `asyncio.Event`).
#[derive(Default)]
pub struct StopFlag {
    flag: AtomicBool,
    pair: (Mutex<()>, Condvar),
    notify: tokio::sync::Notify,
}

impl StopFlag {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let _guard = self.pair.0.lock().unwrap();
        self.pair.1.notify_all();
        self.notify.notify_waiters();
    }

    pub fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Blocking `Event.wait(timeout)`.
    pub fn wait_blocking(&self, seconds: f64) {
        let deadline = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
        let mut guard = self.pair.0.lock().unwrap();
        while !self.is_set() {
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            guard = self.pair.1.wait_timeout(guard, deadline - now).unwrap().0;
        }
    }

    /// Async `wait_for(event.wait(), timeout)`; never errors.
    pub async fn wait(&self, seconds: f64) {
        let wait = async {
            loop {
                let notified = self.notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.is_set() {
                    return;
                }
                notified.await;
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs_f64(seconds.max(0.0)), wait).await;
    }
}
