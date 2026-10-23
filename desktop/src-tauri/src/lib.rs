use std::{
    collections::VecDeque,
    env, fs,
    fs::OpenOptions,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{
    menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, RunEvent, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::ManagerExt;
#[cfg(not(feature = "in-process"))]
use tauri_plugin_shell::{
    process::{CommandChild, CommandEvent},
    ShellExt,
};
use tauri_plugin_updater::UpdaterExt;
use tauri_plugin_window_state::{StateFlags, WindowExt};

#[cfg(target_os = "windows")]
mod windows_jump_list;

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native_credentials;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use native_credentials::NativeCredentialBroker;

mod macos_install;

mod pco_oauth;

const DEFAULT_PORT: u16 = 8765;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
#[cfg_attr(feature = "in-process", allow(dead_code))]
const RECENT_BACKEND_LINES: usize = 32;
// A single rotation (keeping .log.1) is enough headroom for a rapid crash
// loop: at RECENT_BACKEND_LINES=32 lines per failure plus the structured
// failure entry, dozens of failures fit well inside BACKEND_LOG_MAX_BYTES
// before even one rotation, and the failing entry itself is always written
// to the *current* (active) file before any rotation check runs for the
// next backend start. We still bump to two backup generations (.log.1,
// .log.2) below purely as extra headroom for long-lived installs that
// accumulate many restarts across days without the app being restarted.
const BACKEND_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
const BACKEND_LOG_MAX_BACKUPS: u32 = 2;
/// Rolling window used for repeat-crash-loop detection.
const CRASH_LOOP_WINDOW: Duration = Duration::from_secs(120);
/// Number of failures inside `CRASH_LOOP_WINDOW` that qualifies as a loop.
const CRASH_LOOP_THRESHOLD: usize = 3;
/// Maximum bytes of log content returned by the "copy backend log" command.
const BACKEND_LOG_COPY_MAX_BYTES: u64 = 200 * 1024;
/// Maximum number of (newest) lines returned by "Copy Log" / included per
/// log generation in a "Send logs to developer" bundle. A backend under
/// load can log the same handful of lines (e.g. repeated HTTP request
/// entries) thousands of times in a short window; without a line cap, a
/// "Copy Log" click can return thousands of near-duplicate lines that are
/// unreadable to a human and, once JSON-escaped for the diagnostics
/// bundle, can inflate well past the upload size limit even though the
/// raw byte count looked fine (confirmed live: "Diagnostic bundle is too
/// large to send." from a log that was comfortably under the raw-byte
/// budget before escaping). Capping by line count fixes both the
/// readability complaint and the size-after-escaping bug at the source.
///
/// 100, not a rounder/larger number: sized against a real captured log
/// sample (2026-10-03) where a single repeating fetch-and-fail cycle is
/// ~8-10 lines every ~5-6 seconds -- 100 lines keeps 2-3 full cycles
/// visible (enough to see the repeating pattern) while staying far short
/// of unreadable. At ~150-250 bytes/line for these particular HTTP request
/// lines that is ~15-25KB, trivially inside the upload budget.
const MAX_LOG_LINES: usize = 100;
const STAGEPILOT_GITHUB_URL: &str = "https://github.com/tage-ilot/stagepilot";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StagePilotMenuAction {
    Restart,
    Minimize,
    ToggleFullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StagePilotLaunchAction {
    Restart,
    Quit,
}

fn stagepilot_launch_action<'a>(
    arguments: impl IntoIterator<Item = &'a String>,
) -> Option<StagePilotLaunchAction> {
    arguments
        .into_iter()
        .find_map(|argument| match argument.as_str() {
            "--stagepilot-restart" => Some(StagePilotLaunchAction::Restart),
            "--stagepilot-quit" => Some(StagePilotLaunchAction::Quit),
            _ => None,
        })
}

fn stagepilot_menu_action(id: &str) -> Option<StagePilotMenuAction> {
    match id {
        "restart-stagepilot" => Some(StagePilotMenuAction::Restart),
        "minimize-stagepilot" => Some(StagePilotMenuAction::Minimize),
        "toggle-fullscreen-stagepilot" => Some(StagePilotMenuAction::ToggleFullscreen),
        _ => None,
    }
}

fn perform_launch_action(app: &tauri::AppHandle, action: StagePilotLaunchAction) {
    app.state::<BackendSupervisor>().stop(app);
    match action {
        StagePilotLaunchAction::Restart => app.request_restart(),
        StagePilotLaunchAction::Quit => app.exit(0),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BackendState {
    Starting,
    Ready,
    External,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BackendFailureKind {
    PortOccupied,
    SidecarMissing,
    SidecarExited,
    /// The in-process server (cargo feature `in-process`) ended on its own. Only that build
    /// constructs it, so the default (sidecar) build would warn about dead code.
    #[cfg_attr(not(feature = "in-process"), allow(dead_code))]
    ServerExited,
    MacosCodeSigning,
    Timeout,
}

/// Payload for the `stagepilot://backend-crash-loop-detected` event, fired
/// when the backend has failed `CRASH_LOOP_THRESHOLD`+ times within
/// `CRASH_LOOP_WINDOW`.
#[derive(Clone, Debug, Serialize)]
struct CrashLoopDetected {
    failure_kind: BackendFailureKind,
    message: String,
}

fn timeout_may_replace(state: &BackendState) -> bool {
    matches!(state, BackendState::Starting)
}

fn managed_exit_should_fail(state: &BackendState) -> bool {
    matches!(state, BackendState::Starting | BackendState::Ready)
}

#[cfg(any(target_os = "windows", test))]
fn tasklist_has_stagepilot_backend(output: &[u8]) -> bool {
    String::from_utf8_lossy(output)
        .to_ascii_lowercase()
        .contains("stagepilot-backend.exe")
}

#[cfg(target_os = "windows")]
fn windows_backend_process_running() -> Result<bool, String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("tasklist.exe")
        .args([
            "/FI",
            "IMAGENAME eq stagepilot-backend.exe",
            "/FO",
            "CSV",
            "/NH",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| {
            format!("Unable to verify that the StagePilot backend stopped: {error}")
        })?;
    if !output.status.success() {
        return Err("Windows could not verify that the StagePilot backend stopped.".to_string());
    }
    Ok(tasklist_has_stagepilot_backend(&output.stdout))
}

#[derive(Clone, Debug, Serialize)]
struct BackendSupervisorStatus {
    state: BackendState,
    message: String,
    port: u16,
    managed: bool,
    failure_kind: Option<BackendFailureKind>,
    log_path: Option<String>,
}

#[derive(Clone)]
struct BackendSupervisor {
    status: Arc<Mutex<BackendSupervisorStatus>>,
    #[cfg(not(feature = "in-process"))]
    child: Arc<Mutex<Option<CommandChild>>>,
    #[cfg(not(feature = "in-process"))]
    child_pid: Arc<Mutex<Option<u32>>>,
    /// The in-process server (cargo feature `in-process`): the counterpart of `child`.
    #[cfg(feature = "in-process")]
    embedded: Arc<Mutex<Option<stagepilot_server::embedded::EmbeddedServer>>>,
    /// Timestamps of recent classified failures, used for crash-loop
    /// detection. In-memory only; cleared/pruned on a rolling window.
    failure_history: Arc<Mutex<VecDeque<Instant>>>,
    /// Set once a qualifying crash-loop window has been alerted on, so we
    /// don't re-emit on every subsequent failure in the same loop. Cleared
    /// once failures stop for `CRASH_LOOP_COOLDOWN`.
    crash_loop_armed: Arc<Mutex<bool>>,
}

impl BackendSupervisor {
    fn new(port: u16) -> Self {
        Self {
            status: Arc::new(Mutex::new(BackendSupervisorStatus {
                state: BackendState::Starting,
                message: format!("Starting the StagePilot backend on port {port}."),
                port,
                managed: true,
                failure_kind: None,
                log_path: None,
            })),
            #[cfg(not(feature = "in-process"))]
            child: Arc::new(Mutex::new(None)),
            #[cfg(not(feature = "in-process"))]
            child_pid: Arc::new(Mutex::new(None)),
            #[cfg(feature = "in-process")]
            embedded: Arc::new(Mutex::new(None)),
            failure_history: Arc::new(Mutex::new(VecDeque::new())),
            crash_loop_armed: Arc::new(Mutex::new(false)),
        }
    }

    /// Records a failure occurrence and reports whether this failure
    /// qualifies as the (re-)start of a crash loop that should alert the
    /// user. See `crash_loop_qualifies` / `prune_failure_history` for the
    /// pure decision logic (also covered directly by unit tests).
    fn note_failure_and_check_crash_loop(&self, now: Instant) -> bool {
        let mut history = self
            .failure_history
            .lock()
            .expect("backend failure-history lock poisoned");
        history.push_back(now);
        prune_failure_history(&mut history, now);
        let qualifies = crash_loop_qualifies(&history);
        let mut armed = self
            .crash_loop_armed
            .lock()
            .expect("backend crash-loop-armed lock poisoned");
        if qualifies {
            if *armed {
                false
            } else {
                *armed = true;
                true
            }
        } else {
            if history.len() < CRASH_LOOP_THRESHOLD {
                *armed = false;
            }
            false
        }
    }

    fn snapshot(&self) -> BackendSupervisorStatus {
        self.status
            .lock()
            .expect("backend status lock poisoned")
            .clone()
    }

    fn update(
        &self,
        app: &tauri::AppHandle,
        state: BackendState,
        message: impl Into<String>,
        managed: bool,
    ) {
        let mut status = self.status.lock().expect("backend status lock poisoned");
        status.state = state;
        status.message = message.into();
        status.managed = managed;
        status.failure_kind = None;
        let snapshot = status.clone();
        drop(status);
        let _ = app.emit("stagepilot://backend-status", snapshot);
    }

    fn fail(
        &self,
        app: &tauri::AppHandle,
        kind: BackendFailureKind,
        message: impl Into<String>,
        managed: bool,
        log_path: Option<String>,
    ) {
        let mut status = self.status.lock().expect("backend status lock poisoned");
        status.state = BackendState::Failed;
        status.message = message.into();
        status.managed = managed;
        status.failure_kind = Some(kind);
        status.log_path = log_path;
        let snapshot = status.clone();
        drop(status);
        let _ = app.emit("stagepilot://backend-status", snapshot);
    }

    fn fail_if_starting(
        &self,
        app: &tauri::AppHandle,
        kind: BackendFailureKind,
        message: impl Into<String>,
        log_path: Option<String>,
    ) -> bool {
        let mut status = self.status.lock().expect("backend status lock poisoned");
        if !timeout_may_replace(&status.state) {
            return false;
        }
        status.state = BackendState::Failed;
        status.message = message.into();
        status.managed = true;
        status.failure_kind = Some(kind);
        status.log_path = log_path;
        let snapshot = status.clone();
        drop(status);
        let _ = app.emit("stagepilot://backend-status", snapshot);
        true
    }

    fn stop(&self, app: &tauri::AppHandle) {
        self.update(
            app,
            BackendState::Stopped,
            "StagePilot backend stopped.",
            true,
        );
        let _ = self.terminate_child();
    }

    /// Stops the in-process server gracefully (axum drains, then Remote, plugins and the state
    /// service stop in reverse order); aborts it after `DEFAULT_STOP_GRACE`.
    #[cfg(feature = "in-process")]
    fn terminate_child(&self) -> Result<(), String> {
        let server = self
            .embedded
            .lock()
            .expect("backend embedded-server lock poisoned")
            .take();
        if let Some(server) = server {
            server.stop(stagepilot_server::embedded::DEFAULT_STOP_GRACE);
        }
        Ok(())
    }

    #[cfg(not(feature = "in-process"))]
    fn terminate_child(&self) -> Result<(), String> {
        let pid = self
            .child_pid
            .lock()
            .expect("backend child PID lock poisoned")
            .take();
        #[cfg(target_os = "windows")]
        if let Some(pid) = pid {
            use std::os::windows::process::CommandExt;

            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let _ = std::process::Command::new("taskkill.exe")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }
        #[cfg(target_os = "macos")]
        if let Some(pid) = pid {
            terminate_process_tree(pid);
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let _ = pid;
        if let Some(child) = self
            .child
            .lock()
            .expect("backend child lock poisoned")
            .take()
        {
            let _ = child.kill();
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[cfg_attr(feature = "in-process", allow(dead_code))]
fn direct_child_process_ids(parent_pid: u32) -> Vec<u32> {
    let Ok(output) = std::process::Command::new("/usr/bin/pgrep")
        .args(["-P", &parent_pid.to_string()])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|value| value.trim().parse().ok())
        .collect()
}

#[cfg(target_os = "macos")]
#[cfg_attr(feature = "in-process", allow(dead_code))]
fn descendant_process_ids(parent_pid: u32) -> Vec<u32> {
    fn collect(parent_pid: u32, descendants: &mut Vec<u32>) {
        for child_pid in direct_child_process_ids(parent_pid) {
            collect(child_pid, descendants);
            descendants.push(child_pid);
        }
    }

    let mut descendants = Vec::new();
    collect(parent_pid, &mut descendants);
    descendants
}

#[cfg(target_os = "macos")]
#[cfg_attr(feature = "in-process", allow(dead_code))]
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
#[cfg_attr(feature = "in-process", allow(dead_code))]
fn signal_process(pid: u32, signal: &str) {
    let _ = std::process::Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status();
}

#[cfg(target_os = "macos")]
#[cfg_attr(feature = "in-process", allow(dead_code))]
fn terminate_process_tree(root_pid: u32) {
    let mut process_ids = descendant_process_ids(root_pid);
    process_ids.push(root_pid);
    for pid in &process_ids {
        signal_process(*pid, "-TERM");
    }
    std::thread::sleep(Duration::from_millis(300));
    for pid in process_ids {
        if process_exists(pid) {
            signal_process(pid, "-KILL");
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PortProbe {
    Available,
    StagePilot,
    Occupied,
}

fn settings_path() -> PathBuf {
    if let Some(path) = env::var_os("STAGEPILOT_SETTINGS_PATH") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "windows")]
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("USERPROFILE").unwrap_or_default())
                .join("AppData")
                .join("Roaming")
        });
    #[cfg(not(target_os = "windows"))]
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("StagePilot").join("settings.json")
}

fn configured_port() -> u16 {
    if let Ok(value) = env::var("STAGEPILOT_PORT") {
        if let Ok(port) = value.parse::<u16>() {
            if port > 0 {
                return port;
            }
        }
    }
    fs::read_to_string(settings_path())
        .ok()
        .as_deref()
        .and_then(port_from_settings)
        .unwrap_or(DEFAULT_PORT)
}

fn restore_main_window(
    app: &tauri::AppHandle,
    restore_persisted_state: bool,
) -> Result<WebviewWindow, String> {
    let window = if let Some(window) = app.get_webview_window("main") {
        window
    } else {
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|config| config.label == "main")
            .ok_or_else(|| "The StagePilot main-window configuration is missing.".to_string())?;
        WebviewWindowBuilder::from_config(app, config)
            .map_err(|error| format!("Could not configure the StagePilot window: {error}"))?
            .build()
            .map_err(|error| format!("Could not create the StagePilot window: {error}"))?
    };
    if restore_persisted_state {
        window
            .restore_state(StateFlags::all())
            .map_err(|error| format!("Could not restore saved StagePilot window state: {error}"))?;
    }
    window
        .unminimize()
        .map_err(|error| format!("Could not restore the StagePilot window: {error}"))?;
    window
        .show()
        .map_err(|error| format!("Could not show the StagePilot window: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Could not focus the StagePilot window: {error}"))?;
    Ok(window)
}

fn port_from_settings(contents: &str) -> Option<u16> {
    serde_json::from_str::<serde_json::Value>(contents)
        .ok()
        .and_then(|settings| settings.get("server_port")?.as_u64())
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
}

fn lan_access_from_settings(contents: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(contents)
        .ok()
        .and_then(|settings| settings.get("lan_access")?.as_bool())
        .unwrap_or(false)
}

fn probe_port(port: u16) -> PortProbe {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(300)) else {
        return PortProbe::Available;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    if stream
        .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return PortProbe::Occupied;
    }
    let mut response = String::new();
    if stream.read_to_string(&mut response).is_ok()
        && response.starts_with("HTTP/1.1 200")
        && response.contains("\"application_status\"")
        && response.contains("\"version\"")
    {
        PortProbe::StagePilot
    } else {
        PortProbe::Occupied
    }
}

fn rotate_backend_log(path: &std::path::Path) {
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() >= BACKEND_LOG_MAX_BYTES) {
        // Shift .log.(N-1) -> .log.N, ..., .log.1 -> .log.2, then move the
        // active file into .log.1. Oldest generation beyond
        // BACKEND_LOG_MAX_BACKUPS is dropped.
        for generation in (1..BACKEND_LOG_MAX_BACKUPS).rev() {
            let from = path.with_extension(format!("log.{generation}"));
            let to = path.with_extension(format!("log.{}", generation + 1));
            if from.exists() {
                let _ = fs::remove_file(&to);
                let _ = fs::rename(&from, &to);
            }
        }
        let first_backup = path.with_extension("log.1");
        let _ = fs::remove_file(&first_backup);
        let _ = fs::rename(path, &first_backup);
    }
}

/// Removes any known secret-shaped substrings (Planning Center OAuth
/// tokens/credentials and generic bearer/api-key patterns) from text before
/// it is written to the on-disk crash log. The recent-output ring buffer is
/// raw backend stdout/stderr and must never be trusted to be secret-free.
fn redact_secrets(text: &str) -> String {
    let patterns: &[(&str, &str)] = &[
        ("Authorization: Bearer ", "Authorization: Bearer [REDACTED]"),
        ("planning_center_token", "planning_center_token=[REDACTED]"),
        ("access_token", "access_token=[REDACTED]"),
        ("refresh_token", "refresh_token=[REDACTED]"),
        ("client_secret", "client_secret=[REDACTED]"),
    ];
    let mut redacted = text.to_string();
    for line in redacted.clone().lines() {
        let lower = line.to_ascii_lowercase();
        for (needle, _) in patterns {
            if lower.contains(&needle.to_ascii_lowercase()) {
                redacted =
                    redacted.replace(line, "[REDACTED: line contained a credential-shaped token]");
                break;
            }
        }
    }
    redacted
}

/// Renders a UTC timestamp as an ISO-8601 string (e.g.
/// "2024-05-01T12:34:56Z") without pulling in a chrono/time dependency.
fn iso8601_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = now.as_secs();
    let days = total_seconds / 86_400;
    let time_of_day = total_seconds % 86_400;
    let (hour, minute, second) = (
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    );

    // Civil-from-days algorithm (Howard Hinnant's public-domain date
    // algorithms), converts a day count since the Unix epoch into a
    // proleptic-Gregorian (year, month, day) triple.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z",
        day = day,
    )
}

/// Structured crash entry appended to the backend log for every classified
/// failure, on top of the raw stdout/stderr already streamed there. Kept as
/// a plain struct (rather than inline formatting) so both production code
/// and tests share the exact same shape.
#[derive(Debug, Serialize)]
struct BackendCrashLogEntry<'a> {
    timestamp: String,
    app_version: &'a str,
    os: &'a str,
    failure_kind: BackendFailureKind,
    message: &'a str,
    recent_output: String,
}

impl<'a> BackendCrashLogEntry<'a> {
    fn new(
        app_version: &'a str,
        kind: BackendFailureKind,
        message: &'a str,
        recent_output: &str,
    ) -> Self {
        Self {
            timestamp: iso8601_now(),
            app_version,
            os: env::consts::OS,
            failure_kind: kind,
            message,
            recent_output: redact_secrets(recent_output),
        }
    }

    fn render(&self) -> String {
        format!(
            "---- STAGEPILOT BACKEND FAILURE ----\n\
             timestamp: {}\n\
             app_version: {}\n\
             os: {}\n\
             failure_kind: {:?}\n\
             message: {}\n\
             recent_output:\n{}\n\
             ---- END STAGEPILOT BACKEND FAILURE ----\n",
            self.timestamp,
            self.app_version,
            self.os,
            self.failure_kind,
            self.message,
            self.recent_output
        )
    }
}

/// Appends a structured failure entry to the backend log file, in addition
/// to the raw stdout/stderr lines already streamed there by the event
/// pump. Best-effort: failures to write are swallowed since the frontend
/// alert already carries the failure information via the emitted event.
fn write_crash_log_entry(
    log_path: Option<&str>,
    app_version: &str,
    kind: BackendFailureKind,
    message: &str,
    recent_output: &str,
) {
    let Some(path) = log_path else { return };
    let entry = BackendCrashLogEntry::new(app_version, kind, message, recent_output);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(entry.render().as_bytes());
        let _ = file.flush();
    }
}

/// Drops failure timestamps that are either outside the detection window
/// (older than `CRASH_LOOP_WINDOW`) or, when the history has gone quiet for
/// at least `CRASH_LOOP_COOLDOWN`, all history predating the quiet period —
/// this is what allows the loop-detected alert to re-arm after a cooldown.
fn prune_failure_history(history: &mut VecDeque<Instant>, now: Instant) {
    while let Some(oldest) = history.front() {
        if now.duration_since(*oldest) > CRASH_LOOP_WINDOW {
            history.pop_front();
        } else {
            break;
        }
    }
}

/// Pure decision: does this failure history qualify as an active crash
/// loop, i.e. does it contain at least `CRASH_LOOP_THRESHOLD` entries (all
/// of which are already guaranteed to be within `CRASH_LOOP_WINDOW` of
/// `now` by `prune_failure_history`)?
fn crash_loop_qualifies(history: &VecDeque<Instant>) -> bool {
    history.len() >= CRASH_LOOP_THRESHOLD
}

#[cfg_attr(feature = "in-process", allow(dead_code))]
fn recent_backend_text(lines: &VecDeque<String>) -> String {
    lines.iter().cloned().collect::<Vec<_>>().join("\n")
}

#[cfg_attr(feature = "in-process", allow(dead_code))]
fn backend_exit_failure(
    code: Option<i32>,
    recent_output: &str,
    log_path: Option<&str>,
) -> (BackendFailureKind, String) {
    let signing_rejection = [
        "Failed to load Python shared library",
        "mapped file has no Team ID",
        "not valid for use in process",
    ]
    .iter()
    .any(|needle| recent_output.contains(needle));
    let log_hint = log_path
        .map(|path| format!(" See the backend log at {path}."))
        .unwrap_or_default();
    if signing_rejection {
        return (
            BackendFailureKind::MacosCodeSigning,
            format!(
                "The packaged StagePilot backend was blocked by macOS code-signing policy. \
                 This application build is invalid.{log_hint}"
            ),
        );
    }
    (
        BackendFailureKind::SidecarExited,
        format!(
            "The packaged StagePilot backend exited before it became ready (exit code {}).{}",
            code.map_or_else(|| "unknown".to_string(), |value| value.to_string()),
            log_hint
        ),
    )
}

fn wait_for_backend(
    app: tauri::AppHandle,
    supervisor: BackendSupervisor,
    port: u16,
    log_path: Option<String>,
) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while Instant::now() < deadline {
            match probe_port(port) {
                PortProbe::StagePilot => {
                    supervisor.update(
                        &app,
                        BackendState::Ready,
                        format!("StagePilot backend is ready on port {port}."),
                        true,
                    );
                    return;
                }
                PortProbe::Available | PortProbe::Occupied => {
                    std::thread::sleep(PROBE_INTERVAL);
                }
            }
        }
        supervisor.fail_if_starting(
            &app,
            BackendFailureKind::Timeout,
            format!("The StagePilot backend did not become ready on port {port}."),
            log_path,
        );
    });
}

#[cfg(not(feature = "in-process"))]
fn spawn_backend(
    app: &tauri::AppHandle,
    supervisor: &BackendSupervisor,
    port: u16,
    bind_host: &str,
) -> Result<Option<String>, String> {
    let command = app
        .shell()
        .sidecar("stagepilot-backend")
        .map_err(|error| format!("Unable to locate the packaged backend sidecar: {error}"))?
        .env("STAGEPILOT_HOST", bind_host)
        .env("STAGEPILOT_PORT", port.to_string())
        .env("STAGEPILOT_SETTINGS_PATH", settings_path())
        .env(
            "STAGEPILOT_DESKTOP_REMOTE_ROOT",
            app.path()
                .app_data_dir()
                .map_err(|error| format!("Unable to resolve desktop Remote state: {error}"))?
                .join("remote"),
        )
        .env(
            "STAGEPILOT_CLOUDFLARED_BINARY",
            app.path()
                .resource_dir()
                .map_err(|error| format!("Unable to resolve bundled cloudflared: {error}"))?
                .join(if cfg!(target_os = "windows") {
                    "cloudflared.exe"
                } else {
                    "cloudflared"
                }),
        )
        .env(
            "STAGEPILOT_PCO_CLIENT_ID",
            // Baked in at compile time by build.rs from the
            // STAGEPILOT_PCO_CLIENT_ID build-time env var (release
            // workflows source it from the PLANNING_CENTER_CLIENT_ID repo
            // secret). Not a secret itself — see build.rs for rationale.
            env!("STAGEPILOT_PCO_CLIENT_ID"),
        );
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let command = {
        let broker = app.state::<NativeCredentialBroker>();
        command
            .env("STAGEPILOT_CREDENTIAL_BROKER_ORIGIN", &broker.origin)
            .env("STAGEPILOT_CREDENTIAL_BROKER_TOKEN", &broker.authorization)
    };
    let (mut events, child) = command
        .spawn()
        .map_err(|error| format!("Unable to start the packaged backend sidecar: {error}"))?;
    *supervisor
        .child_pid
        .lock()
        .expect("backend child PID lock poisoned") = Some(child.pid());
    *supervisor
        .child
        .lock()
        .expect("backend child lock poisoned") = Some(child);

    let backend_log_path = app
        .path()
        .app_log_dir()
        .ok()
        .map(|directory| directory.join("stagepilot-backend.log"));
    let backend_log_path_text = backend_log_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let backend_log = backend_log_path.as_ref().and_then(|path| {
        let directory = path.parent()?;
        fs::create_dir_all(directory).ok()?;
        rotate_backend_log(path);
        OpenOptions::new().create(true).append(true).open(path).ok()
    });
    let backend_log = Arc::new(Mutex::new(backend_log));
    let recent_output = Arc::new(Mutex::new(VecDeque::<String>::new()));

    let event_app = app.clone();
    let event_supervisor = supervisor.clone();
    let event_log_path = backend_log_path_text.clone();
    let event_app_version = app.package_info().version.to_string();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(line) | CommandEvent::Stderr(line) => {
                    let text = String::from_utf8_lossy(&line).trim().to_string();
                    if !text.is_empty() {
                        let mut recent = recent_output
                            .lock()
                            .expect("backend recent-output lock poisoned");
                        recent.push_back(text);
                        while recent.len() > RECENT_BACKEND_LINES {
                            recent.pop_front();
                        }
                    }
                    if let Some(file) = backend_log
                        .lock()
                        .expect("backend log lock poisoned")
                        .as_mut()
                    {
                        let _ = file.write_all(&line);
                        if !line.ends_with(b"\n") {
                            let _ = file.write_all(b"\n");
                        }
                        let _ = file.flush();
                    }
                }
                CommandEvent::Terminated(payload) => {
                    if managed_exit_should_fail(&event_supervisor.snapshot().state) {
                        let recent = recent_backend_text(
                            &recent_output
                                .lock()
                                .expect("backend recent-output lock poisoned"),
                        );
                        let (kind, message) =
                            backend_exit_failure(payload.code, &recent, event_log_path.as_deref());
                        record_backend_failure(
                            &event_app,
                            &event_supervisor,
                            &event_app_version,
                            event_log_path.clone(),
                            kind,
                            message,
                            &recent,
                        );
                    }
                    break;
                }
                _ => {}
            }
        }
    });
    Ok(backend_log_path_text)
}

/// The `in-process` counterpart of the sidecar spawn: the same environment contract, but the
/// server runs as tasks inside this process (`stagepilot-server`), so there is no child to
/// signal. Differences from the sidecar are documented in docs/rust-migration/phase6-remaining.md.
#[cfg(feature = "in-process")]
fn spawn_backend(
    app: &tauri::AppHandle,
    supervisor: &BackendSupervisor,
    port: u16,
    bind_host: &str,
) -> Result<Option<String>, String> {
    // The server reads its configuration from the environment, exactly as the sidecar does; in
    // process it is this process's environment.
    env::set_var("STAGEPILOT_HOST", bind_host);
    env::set_var("STAGEPILOT_PORT", port.to_string());
    env::set_var("STAGEPILOT_SETTINGS_PATH", settings_path());
    env::set_var(
        "STAGEPILOT_DESKTOP_REMOTE_ROOT",
        app.path()
            .app_data_dir()
            .map_err(|error| format!("Unable to resolve desktop Remote state: {error}"))?
            .join("remote"),
    );
    env::set_var(
        "STAGEPILOT_CLOUDFLARED_BINARY",
        app.path()
            .resource_dir()
            .map_err(|error| format!("Unable to resolve bundled cloudflared: {error}"))?
            .join(if cfg!(target_os = "windows") {
                "cloudflared.exe"
            } else {
                "cloudflared"
            }),
    );
    // The Rust release variant bundles the built frontend as the `stagepilot_web` resource (the
    // Python sidecar carries its own copy inside PyInstaller). An explicit override wins, and a
    // development run without the resource keeps the server's own fallbacks.
    if env::var_os("STAGEPILOT_WEB_ROOT").is_none() {
        if let Ok(resources) = app.path().resource_dir() {
            let web_root = resources.join("stagepilot_web");
            if web_root.join("index.html").is_file() {
                env::set_var("STAGEPILOT_WEB_ROOT", web_root);
            }
        }
    }
    env::set_var("STAGEPILOT_PCO_CLIENT_ID", env!("STAGEPILOT_PCO_CLIENT_ID"));
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let broker = app.state::<NativeCredentialBroker>();
        env::set_var("STAGEPILOT_CREDENTIAL_BROKER_ORIGIN", &broker.origin);
        env::set_var("STAGEPILOT_CREDENTIAL_BROKER_TOKEN", &broker.authorization);
    }

    let backend_log_path = app
        .path()
        .app_log_dir()
        .ok()
        .map(|directory| directory.join("stagepilot-backend.log"));
    let backend_log_path_text = backend_log_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    if let Some(path) = backend_log_path.as_ref() {
        if let Some(directory) = path.parent() {
            let _ = fs::create_dir_all(directory);
        }
        rotate_backend_log(path);
    }
    let (server, reports) = stagepilot_server::embedded::EmbeddedServer::start(
        bind_host.to_string(),
        port,
        backend_log_path,
    )
    .map_err(|error| format!("Unable to start the embedded StagePilot server: {error}"))?;
    *supervisor
        .embedded
        .lock()
        .expect("backend embedded-server lock poisoned") = Some(server);

    // The server ends on its own only when it crashes or fails to start (a requested stop sends
    // no report), so this is the in-process analogue of the sidecar's `Terminated` event.
    let event_app = app.clone();
    let event_supervisor = supervisor.clone();
    let event_log_path = backend_log_path_text.clone();
    let event_app_version = app.package_info().version.to_string();
    std::thread::spawn(move || {
        let Ok(report) = reports.recv() else { return };
        if managed_exit_should_fail(&event_supervisor.snapshot().state) {
            record_backend_failure(
                &event_app,
                &event_supervisor,
                &event_app_version,
                event_log_path,
                BackendFailureKind::ServerExited,
                report.message(),
                "",
            );
        }
    });
    Ok(backend_log_path_text)
}

/// Shared failure path for both variants: structured crash-log entry, `Failed` status event,
/// crash-loop detection.
fn record_backend_failure(
    app: &tauri::AppHandle,
    supervisor: &BackendSupervisor,
    app_version: &str,
    log_path: Option<String>,
    kind: BackendFailureKind,
    message: String,
    recent_output: &str,
) {
    write_crash_log_entry(
        log_path.as_deref(),
        app_version,
        kind.clone(),
        &message,
        recent_output,
    );
    supervisor.fail(app, kind.clone(), message.clone(), true, log_path);
    if supervisor.note_failure_and_check_crash_loop(Instant::now()) {
        let _ = app.emit(
            "stagepilot://backend-crash-loop-detected",
            CrashLoopDetected {
                failure_kind: kind,
                message,
            },
        );
    }
}

fn start_backend(app: &tauri::AppHandle, supervisor: BackendSupervisor) -> Result<(), String> {
    let port = supervisor.snapshot().port;
    match probe_port(port) {
        PortProbe::StagePilot => {
            supervisor.update(
                app,
                BackendState::External,
                format!("Connected to an existing StagePilot backend on port {port}."),
                false,
            );
            return Ok(());
        }
        PortProbe::Occupied => {
            supervisor.fail(
                app,
                BackendFailureKind::PortOccupied,
                format!("Port {port} is already in use by another application."),
                false,
                None,
            );
            return Ok(());
        }
        PortProbe::Available => {}
    }

    let lan_access_enabled = fs::read_to_string(settings_path())
        .ok()
        .is_some_and(|settings| lan_access_from_settings(&settings));
    let bind_host = if lan_access_enabled {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let backend_log_path_text = spawn_backend(app, &supervisor, port, bind_host)?;
    wait_for_backend(app.clone(), supervisor, port, backend_log_path_text);
    Ok(())
}

#[tauri::command]
fn backend_supervisor_status(
    supervisor: tauri::State<'_, BackendSupervisor>,
) -> BackendSupervisorStatus {
    supervisor.snapshot()
}

/// Reads the backend log file content for clipboard copy, primarily for
/// the "Copy Backend Log" button and the crash-loop alert dialog. Returns
/// at most the last `BACKEND_LOG_COPY_MAX_BYTES` bytes when the file is
/// larger, so a runaway log can't stall the UI or blow up the clipboard.
///
/// Uses `resolve_diagnostic_log_path` (see `collect_diagnostic_bundle`) for
/// the same reason: `supervisor.snapshot().log_path` is only ever populated
/// on a failure transition, so a healthy running backend previously made
/// this command always fail with "No backend log is available yet." --
/// which is why it only ever worked on the startup-crash screen and never
/// as a normal Settings-panel action.
#[tauri::command]
fn copy_backend_log(
    app: tauri::AppHandle,
    supervisor: tauri::State<'_, BackendSupervisor>,
) -> Result<String, String> {
    let computed_log_path = app.path().app_log_dir().ok().map(|directory| {
        directory
            .join("stagepilot-backend.log")
            .to_string_lossy()
            .into_owned()
    });
    let log_path = resolve_diagnostic_log_path(computed_log_path, supervisor.snapshot().log_path)
        .ok_or_else(|| "No backend log is available yet.".to_string())?;
    read_backend_log_tail(std::path::Path::new(&log_path))
}

fn read_backend_log_tail(path: &std::path::Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("Unable to open the backend log: {error}"))?;
    let size = file
        .metadata()
        .map_err(|error| format!("Unable to read the backend log metadata: {error}"))?
        .len();
    if size > BACKEND_LOG_COPY_MAX_BYTES {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(size - BACKEND_LOG_COPY_MAX_BYTES))
            .map_err(|error| format!("Unable to seek the backend log: {error}"))?;
    }
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|error| format!("Unable to read the backend log: {error}"))?;
    Ok(tail_lines(&content, MAX_LOG_LINES).0)
}

/// Keeps at most the last `max_lines` lines of `content`, discarding from
/// the start. Returns the (possibly unmodified) text plus whether anything
/// was dropped. Line-based (rather than byte-based) truncation is what
/// actually fixes the "thousands of near-identical repeated lines" case: a
/// backend logging the same handful of lines under load can stay well
/// under a byte budget while still being thousands of lines of noise.
fn tail_lines(content: &str, max_lines: usize) -> (String, bool) {
    let total_lines = content.lines().count();
    if total_lines <= max_lines {
        return (content.to_string(), false);
    }
    let kept: Vec<&str> = content.lines().skip(total_lines - max_lines).collect();
    (kept.join("\n"), true)
}

/// Maximum total size (bytes) of the diagnostic bundle returned by
/// `collect_diagnostic_bundle`. When the combined log content would exceed
/// this, the OLDEST log generation's content is truncated/dropped first so
/// the most recent (and most relevant) log content is always preserved.
const DIAGNOSTIC_BUNDLE_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Serialize)]
struct DiagnosticLogFile {
    /// Logical name, e.g. "stagepilot-backend.log", "stagepilot-backend.log.1".
    name: String,
    /// UTF-8 (lossy) content of this log generation, possibly truncated.
    content: String,
    /// Original size in bytes before any truncation.
    original_bytes: u64,
    /// True if this file's content was truncated to fit the bundle budget.
    truncated: bool,
}

#[derive(Serialize)]
struct DiagnosticAppInfo {
    app_version: String,
    os: String,
    arch: String,
}

#[derive(Serialize)]
struct DiagnosticBundle {
    app_info: DiagnosticAppInfo,
    /// Optional frontend-supplied crash message/stack, when triggered from a
    /// JS error boundary rather than the manual "send logs" button.
    frontend_crash: Option<String>,
    /// Log files ordered from most recent (active log) to oldest rotation.
    logs: Vec<DiagnosticLogFile>,
}

/// Picks the backend log path a diagnostic bundle should read from.
///
/// `computed_path` is the SAME deterministic path `start_backend()` derives
/// for every launch (`app_log_dir()/stagepilot-backend.log`) and is present
/// whenever the app's log directory is resolvable — i.e. essentially always,
/// regardless of whether the backend is currently healthy or has ever
/// failed. `snapshot_path` is `BackendSupervisor`'s own tracked path, which
/// is ONLY ever set on a failure transition (`fail()` / `fail_if_starting()`)
/// — the normal running-state transition (`update()`) never touches it.
///
/// Preferring `computed_path` fixes a real bug confirmed live in production
/// (2026-10-02): a healthy, successfully-running backend — the overwhelming
/// common case when a user clicks "send logs to developer" — always hit the
/// `None` snapshot path and produced a bundle with `logs: []`, useless for
/// debugging. `snapshot_path` is kept only as a fallback for the rare case
/// `app_log_dir()` itself is unresolvable.
fn resolve_diagnostic_log_path(
    computed_path: Option<String>,
    snapshot_path: Option<String>,
) -> Option<String> {
    computed_path.or(snapshot_path)
}

/// Collects the active backend log plus any rotated `.log.1`/`.log.2`
/// generations (see `rotate_backend_log` / `BACKEND_LOG_MAX_BACKUPS` above),
/// basic non-identifying app/platform info, and an optional frontend crash
/// message, into a single JSON blob the frontend can POST directly.
///
/// Deliberately does NOT include arbitrary user data, raw file listings, or
/// PCO/ProPresenter credentials/tokens — only the backend log text itself
/// and static app/OS/arch strings.
///
/// The combined bundle is capped at `DIAGNOSTIC_BUNDLE_MAX_BYTES`: if the
/// logs together exceed the budget, the OLDEST log generation is truncated
/// (from its start, keeping its tail) first, and dropped entirely before any
/// byte of the most recent log is cut.
#[tauri::command]
fn collect_diagnostic_bundle(
    app: tauri::AppHandle,
    supervisor: tauri::State<'_, BackendSupervisor>,
    frontend_crash: Option<String>,
) -> Result<String, String> {
    // BUG FIX: `supervisor.snapshot().log_path` is ONLY ever populated by
    // `fail()`/`fail_if_starting()` -- the normal running-state transition
    // (`update()`) never sets it. That meant a healthy, successfully-running
    // backend (the overwhelmingly common case a user clicks "send logs"
    // from) always produced an empty `logs: []` bundle, confirmed live in
    // production on 2026-10-02 (a 107-byte bundle with app_info populated
    // but zero log content). See `resolve_diagnostic_log_path` below.
    let computed_log_path = app.path().app_log_dir().ok().map(|directory| {
        directory
            .join("stagepilot-backend.log")
            .to_string_lossy()
            .into_owned()
    });
    let log_path = resolve_diagnostic_log_path(computed_log_path, supervisor.snapshot().log_path);

    let mut generations: Vec<(String, PathBuf)> = Vec::new();
    if let Some(path_text) = log_path.as_ref() {
        let active_path = std::path::Path::new(path_text).to_path_buf();
        let name = active_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "stagepilot-backend.log".to_string());
        generations.push((name, active_path.clone()));
        for generation in 1..=BACKEND_LOG_MAX_BACKUPS {
            let backup_path = active_path.with_extension(format!("log.{generation}"));
            let backup_name = backup_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("stagepilot-backend.log.{generation}"));
            generations.push((backup_name, backup_path));
        }
    }

    let mut logs: Vec<DiagnosticLogFile> = Vec::new();
    for (name, path) in &generations {
        if let Ok(metadata) = fs::metadata(path) {
            let original_bytes = metadata.len();
            let content = fs::read_to_string(path).unwrap_or_else(|_| {
                // Fall back to a lossy read for logs with non-UTF-8 bytes
                // rather than dropping the generation entirely.
                fs::read(path)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .unwrap_or_default()
            });
            // Cap by line count BEFORE the byte-budget truncation below: a
            // backend logging the same handful of lines thousands of times
            // under load can stay well under DIAGNOSTIC_BUNDLE_MAX_BYTES in
            // raw bytes while still being thousands of repeated lines --
            // and once JSON-escaped, can exceed the upload size limit even
            // though the raw read looked fine (confirmed live, 2026-10-03:
            // "Diagnostic bundle is too large to send." from a log under
            // the raw budget before escaping).
            let (content, line_truncated) = tail_lines(&content, MAX_LOG_LINES);
            logs.push(DiagnosticLogFile {
                name: name.clone(),
                content,
                original_bytes,
                truncated: line_truncated,
            });
        }
    }

    truncate_logs_to_budget(
        &mut logs,
        DIAGNOSTIC_BUNDLE_MAX_BYTES,
        frontend_crash.as_deref(),
    );

    let app_info = DiagnosticAppInfo {
        app_version: app.package_info().version.to_string(),
        os: env::consts::OS.to_string(),
        arch: env::consts::ARCH.to_string(),
    };

    let bundle = DiagnosticBundle {
        app_info,
        frontend_crash,
        logs,
    };

    serde_json::to_string(&bundle)
        .map_err(|error| format!("Unable to serialize the diagnostic bundle: {error}"))
}

/// Trims `logs` (ordered newest-first, as built by `collect_diagnostic_bundle`)
/// so their combined content, plus a fixed-size allowance for `frontend_crash`
/// and the app-info fields, fits within `budget_bytes`. The OLDEST entries
/// (the tail of the vec) are truncated first — from the start of their
/// content, keeping the end — and dropped entirely before any byte is cut
/// from a newer (earlier-in-the-vec) log.
fn truncate_logs_to_budget(
    logs: &mut [DiagnosticLogFile],
    budget_bytes: usize,
    frontend_crash: Option<&str>,
) {
    // Reserve a small allowance for the non-log fields (app info, crash
    // text, JSON structure/field-name overhead) so the final serialized
    // bundle stays within budget, not just the raw log bytes.
    let reserved = frontend_crash.map(str::len).unwrap_or(0) + 1024;
    let mut remaining = budget_bytes.saturating_sub(reserved);

    // First pass: logs are already ordered newest-first; walk them in that
    // order, keeping each in full while budget allows.
    let mut kept_full = vec![false; logs.len()];
    for (index, log) in logs.iter().enumerate() {
        let size = log.content.len();
        if size <= remaining {
            remaining -= size;
            kept_full[index] = true;
        } else {
            break;
        }
    }

    for (index, log) in logs.iter_mut().enumerate() {
        if kept_full[index] {
            continue;
        }
        if remaining == 0 {
            // No budget left at all for this (older) generation: drop it.
            log.content.clear();
            log.truncated = true;
            continue;
        }
        // Keep the TAIL (most recent portion) of this generation's content
        // and truncate from the start, on a UTF-8 char boundary.
        let content = std::mem::take(&mut log.content);
        let total_len = content.len();
        if total_len <= remaining {
            log.content = content;
            continue;
        }
        let mut start = total_len - remaining;
        while start < total_len && !content.is_char_boundary(start) {
            start += 1;
        }
        log.content = content[start..].to_string();
        log.truncated = true;
        remaining = 0;
    }
}

#[tauri::command]
async fn restart_managed_backend(
    app: tauri::AppHandle,
    supervisor: tauri::State<'_, BackendSupervisor>,
) -> Result<BackendSupervisorStatus, String> {
    if !supervisor.snapshot().managed {
        return Err(
            "StagePilot is connected to an older external backend. Fully quit StagePilot once, then reopen it."
                .to_string(),
        );
    }
    supervisor.update(
        &app,
        BackendState::Stopped,
        "Restarting the StagePilot backend.",
        true,
    );
    supervisor.terminate_child()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && probe_port(supervisor.snapshot().port) != PortProbe::Available
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if probe_port(supervisor.snapshot().port) != PortProbe::Available {
        return Err("The previous StagePilot backend did not stop in time.".to_string());
    }

    supervisor.update(
        &app,
        BackendState::Starting,
        format!(
            "Starting the StagePilot backend on port {}.",
            supervisor.snapshot().port
        ),
        true,
    );
    start_backend(&app, supervisor.inner().clone())?;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if probe_port(supervisor.snapshot().port) == PortProbe::StagePilot {
            return Ok(supervisor.snapshot());
        }
        tokio::time::sleep(PROBE_INTERVAL).await;
    }
    Err("The restarted StagePilot backend did not become ready.".to_string())
}

#[tauri::command]
async fn prepare_for_update(
    app: tauri::AppHandle,
    supervisor: tauri::State<'_, BackendSupervisor>,
) -> Result<(), String> {
    // Never stop the backend for an install that cannot possibly land: under
    // macOS App Translocation the running bundle lives on a read-only image, so
    // the in-place replacement fails *after* the backend is already down.
    let environment = macos_install::describe();
    if environment.updates_blocked {
        return Err(environment
            .guidance
            .unwrap_or_else(|| macos_install::TRANSLOCATION_GUIDANCE.to_string()));
    }
    let managed = supervisor.snapshot().managed;
    supervisor.update(
        &app,
        BackendState::Stopped,
        "Stopping the StagePilot backend before installing the update.",
        managed,
    );
    if !managed {
        return Ok(());
    }
    supervisor.terminate_child()?;

    #[cfg(target_os = "windows")]
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let port_available = probe_port(supervisor.snapshot().port) == PortProbe::Available;
            let process_stopped = !windows_backend_process_running()?;
            if port_available && process_stopped {
                // Windows can retain the executable mapping briefly after the process
                // disappears. Give the loader time to release the sidecar before NSIS
                // attempts to replace it.
                tokio::time::sleep(Duration::from_millis(500)).await;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(
            "The StagePilot backend did not release its executable before update installation."
                .to_string(),
        )
    }

    #[cfg(not(target_os = "windows"))]
    Ok(())
}

/// Metadata describing an update found by [`check_for_update`].
///
/// This mirrors the plugin's own (private) `Metadata` shape closely enough
/// for the frontend to reconstruct the plugin's public `Update` class from
/// it and then drive the plugin's own `download`/`install` IPC commands by
/// resource id.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMetadata {
    rid: tauri::ResourceId,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    raw_json: serde_json::Value,
}

/// Checks the endpoint configured in `tauri.conf.json` (the GitHub releases feed).
///
/// This goes through `updater_builder()` -- the same builder the plugin's own
/// default `check` IPC command uses internally -- so signature verification is
/// identical to the default path. "No update available" surfaces as `Ok(None)`,
/// never an error, exactly like the plugin's default check.
#[tauri::command]
async fn check_for_update(webview: tauri::Webview) -> Result<Option<UpdateMetadata>, String> {
    let updater = webview
        .updater_builder()
        .build()
        .map_err(|error| format!("StagePilot could not build the updater: {error}"))?;

    let update = updater
        .check()
        .await
        .map_err(|error| format!("StagePilot could not check for updates: {error}"))?;

    Ok(update.map(|update| {
        let metadata = UpdateMetadata {
            current_version: update.current_version.clone(),
            version: update.version.clone(),
            date: update.date.map(|date| date.to_string()),
            body: update.body.clone(),
            raw_json: update.raw_json.clone(),
            rid: 0,
        };
        let mut resources = webview.resources_table();
        let rid = resources.add(update);
        UpdateMetadata { rid, ..metadata }
    }))
}

/// Reports where StagePilot is actually running from so the frontend can block
/// in-app updates (and explain why) when macOS has translocated the bundle.
#[tauri::command]
fn install_environment() -> macos_install::InstallEnvironment {
    macos_install::describe()
}

#[tauri::command]
fn set_remote_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|error| format!("Could not update Remote restart recovery: {error}"))
}

#[tauri::command]
fn hide_application_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "The StagePilot window is unavailable.".to_string())?;
    window
        .hide()
        .map_err(|error| format!("StagePilot could not hide its window: {error}"))
}

/// Opens Planning Center's consent page in the user's real system browser
/// (never an embedded webview) and waits on the single-use loopback
/// listener for the authorization code.
///
/// The backend owns PKCE/state generation and the token exchange; this
/// command only performs the two steps that must happen on the desktop:
/// binding one of the four pre-registered loopback redirect ports and
/// opening the browser. The listener is bound BEFORE the browser opens so
/// a very fast redirect can never arrive at a closed port.
#[tauri::command]
async fn planning_center_sign_in(
    authorize_url_prefix: String,
    state: String,
) -> Result<PlanningCenterSignIn, String> {
    if state.is_empty() {
        return Err("Planning Center sign-in could not be started.".to_string());
    }
    let server = pco_oauth::CallbackServer::start()?;
    let redirect_uri = server.redirect_uri();
    let authorize_url = format!(
        "{authorize_url_prefix}&redirect_uri={}",
        url::form_urlencoded::byte_serialize(redirect_uri.as_bytes()).collect::<String>()
    );
    if !pco_oauth::is_allowed_authorize_url(&authorize_url) {
        return Err("Planning Center sign-in could not be started.".to_string());
    }
    tauri_plugin_opener::open_url(&authorize_url, None::<&str>)
        .map_err(|_| "StagePilot could not open your browser to sign in.".to_string())?;
    let timeout = pco_oauth::default_timeout();
    let code = tauri::async_runtime::spawn_blocking(move || server.wait(&state, timeout))
        .await
        .map_err(|_| "Planning Center sign-in did not complete. Try again.".to_string())??;
    Ok(PlanningCenterSignIn { code, redirect_uri })
}

/// The redirect URI is returned alongside the code because Planning Center
/// requires the exact same value on the token exchange, and only the
/// desktop side knows which of the four registered ports was free.
#[derive(Clone, serde::Serialize)]
struct PlanningCenterSignIn {
    code: String,
    redirect_uri: String,
}
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn install_application_menu(app: &tauri::AppHandle) -> Result<(), String> {
    let package = app.package_info();
    let config = app.config();
    let about = AboutMetadata {
        name: Some(package.name.clone()),
        version: Some(package.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        credits: Some(format!("StagePilot on GitHub\n{STAGEPILOT_GITHUB_URL}")),
        icon: app.default_window_icon().cloned(),
        ..Default::default()
    };
    let restart = MenuItem::with_id(
        app,
        "restart-stagepilot",
        "Restart StagePilot",
        true,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let application_menu = Submenu::with_items(
        app,
        package.name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, None, Some(about))
                .map_err(|error| error.to_string())?,
            &PredefinedMenuItem::separator(app).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::services(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::separator(app).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::hide(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::hide_others(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::show_all(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::separator(app).map_err(|error| error.to_string())?,
            &restart,
            &PredefinedMenuItem::quit(app, None).map_err(|error| error.to_string())?,
        ],
    )
    .map_err(|error| error.to_string())?;
    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::redo(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::separator(app).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::cut(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::copy(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::paste(app, None).map_err(|error| error.to_string())?,
            &PredefinedMenuItem::select_all(app, None).map_err(|error| error.to_string())?,
        ],
    )
    .map_err(|error| error.to_string())?;
    let minimize = MenuItem::with_id(
        app,
        "minimize-stagepilot",
        "Minimize",
        true,
        Some("Command+M"),
    )
    .map_err(|error| error.to_string())?;
    let toggle_fullscreen = MenuItem::with_id(
        app,
        "toggle-fullscreen-stagepilot",
        "Toggle Full Screen",
        true,
        Some("Control+Command+F"),
    )
    .map_err(|error| error.to_string())?;
    let window_menu = Submenu::with_id_and_items(
        app,
        "stagepilot-window-menu",
        "Window",
        true,
        &[&minimize, &toggle_fullscreen],
    )
    .map_err(|error| error.to_string())?;
    let help_menu = Submenu::with_id_and_items(app, "stagepilot-help-menu", "Help", true, &[])
        .map_err(|error| error.to_string())?;
    let menu = Menu::with_items(
        app,
        &[&application_menu, &edit_menu, &window_menu, &help_menu],
    )
    .map_err(|error| error.to_string())?;
    app.set_menu(menu).map_err(|error| error.to_string())?;
    Ok(())
}

fn install_tray(app: &tauri::App) -> Result<(), String> {
    let show = MenuItem::with_id(
        app,
        "show-stagepilot",
        "Show StagePilot",
        true,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let restart = MenuItem::with_id(
        app,
        "restart-stagepilot",
        "Restart StagePilot",
        true,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let quit = MenuItem::with_id(
        app,
        "quit-stagepilot",
        "Quit StagePilot",
        true,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let menu =
        Menu::with_items(app, &[&show, &restart, &quit]).map_err(|error| error.to_string())?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("StagePilot")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show-stagepilot" => {
                let _ = restore_main_window(app, false);
            }
            "restart-stagepilot" => {
                perform_launch_action(app, StagePilotLaunchAction::Restart);
            }
            "quit-stagepilot" => {
                perform_launch_action(app, StagePilotLaunchAction::Quit);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = restore_main_window(tray.app_handle(), false);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app).map_err(|error| error.to_string())?;
    Ok(())
}

/// Starts the StagePilot native shell and supervises its packaged backend.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let launch_arguments = env::args().collect::<Vec<_>>();
    let initial_launch_action = stagepilot_launch_action(&launch_arguments);
    let port = configured_port();
    let supervisor = BackendSupervisor::new(port);
    let shutdown_supervisor = supervisor.clone();
    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let builder = {
        let credential_broker =
            NativeCredentialBroker::start().expect("failed to start the native credential broker");
        builder.manage(credential_broker)
    };
    let app = builder
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            if let Some(action) = stagepilot_launch_action(&arguments) {
                perform_launch_action(app, action);
            } else {
                let _ = restore_main_window(app, false);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .on_menu_event(
            |app, event| match stagepilot_menu_action(event.id().as_ref()) {
                Some(StagePilotMenuAction::Restart) => {
                    perform_launch_action(app, StagePilotLaunchAction::Restart);
                }
                Some(StagePilotMenuAction::Minimize) => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.minimize();
                    }
                }
                Some(StagePilotMenuAction::ToggleFullscreen) => {
                    if let Some(window) = app.get_webview_window("main") {
                        if let Ok(fullscreen) = window.is_fullscreen() {
                            let _ = window.set_fullscreen(!fullscreen);
                        }
                    }
                }
                None => {}
            },
        )
        .manage(supervisor.clone())
        .invoke_handler(tauri::generate_handler![
            backend_supervisor_status,
            restart_managed_backend,
            copy_backend_log,
            collect_diagnostic_bundle,
            prepare_for_update,
            check_for_update,
            install_environment,
            set_remote_autostart,
            hide_application_window,
            planning_center_sign_in
        ])
        .setup(move |app| {
            if initial_launch_action == Some(StagePilotLaunchAction::Quit) {
                app.handle().exit(0);
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            install_application_menu(app.handle()).map_err(std::io::Error::other)?;
            // Strip our own Gatekeeper quarantine flag (when present and
            // writable) so macOS cannot translocate the next launch.
            macos_install::clear_own_quarantine_if_possible();
            #[cfg(target_os = "windows")]
            if let Err(message) = windows_jump_list::install() {
                eprintln!("StagePilot could not install its Windows taskbar actions: {message}");
            }
            restore_main_window(app.handle(), true).map_err(std::io::Error::other)?;
            install_tray(app).map_err(std::io::Error::other)?;
            if let Err(message) = start_backend(app.handle(), supervisor.clone()) {
                let kind = if message.contains("locate the packaged backend sidecar") {
                    BackendFailureKind::SidecarMissing
                } else {
                    BackendFailureKind::SidecarExited
                };
                supervisor.fail(app.handle(), kind, message, true, None);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the StagePilot desktop shell");

    app.run(move |handle, event| match event {
        RunEvent::Exit | RunEvent::ExitRequested { .. } => shutdown_supervisor.stop(handle),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            let _ = restore_main_window(handle, false);
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_log(name: &str, content: &str) -> DiagnosticLogFile {
        DiagnosticLogFile {
            name: name.to_string(),
            content: content.to_string(),
            original_bytes: content.len() as u64,
            truncated: false,
        }
    }

    #[test]
    fn resolve_diagnostic_log_path_prefers_computed_path_when_healthy() {
        // Regression test for the production bug (2026-10-02): a healthy
        // backend has `snapshot_path == None` (only ever set on failure),
        // but `computed_path` is always available. The bundle must use it,
        // not silently fall back to an empty log list.
        let resolved =
            resolve_diagnostic_log_path(Some("/data/stagepilot-backend.log".to_string()), None);
        assert_eq!(resolved, Some("/data/stagepilot-backend.log".to_string()));
    }

    #[test]
    fn resolve_diagnostic_log_path_falls_back_to_snapshot_when_uncomputable() {
        let resolved =
            resolve_diagnostic_log_path(None, Some("/tmp/stagepilot-backend.log".to_string()));
        assert_eq!(resolved, Some("/tmp/stagepilot-backend.log".to_string()));
    }

    #[test]
    fn resolve_diagnostic_log_path_none_when_both_unavailable() {
        assert_eq!(resolve_diagnostic_log_path(None, None), None);
    }

    #[test]
    fn tail_lines_keeps_everything_under_the_limit() {
        let content = "line1\nline2\nline3";
        let (result, truncated) = tail_lines(content, 500);
        assert_eq!(result, content);
        assert!(!truncated);
    }

    #[test]
    fn tail_lines_keeps_only_the_newest_lines_over_the_limit() {
        // Regression test for the production bug (2026-10-03): a backend
        // logging the same handful of lines (e.g. repeated HTTP request
        // entries) thousands of times stayed under the raw-byte budget but
        // produced an unreadable "Copy Log" result and, once JSON-escaped,
        // a diagnostics bundle that exceeded the server's upload size
        // limit ("Diagnostic bundle is too large to send.").
        let content = (1..=1000)
            .map(|index| format!("line{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (result, truncated) = tail_lines(&content, 500);
        assert!(truncated);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 500);
        assert_eq!(lines.first(), Some(&"line501"));
        assert_eq!(lines.last(), Some(&"line1000"));
    }

    #[test]
    fn tail_lines_handles_exactly_the_limit() {
        let content = (1..=500)
            .map(|index| format!("line{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (result, truncated) = tail_lines(&content, 500);
        assert!(!truncated);
        assert_eq!(result.lines().count(), 500);
    }

    #[test]
    fn max_log_lines_is_100() {
        // Pins the real constant used by copy_backend_log and
        // collect_diagnostic_bundle, sized against a real captured log
        // sample (2026-10-03): ~8-10 lines per repeating fetch-and-fail
        // cycle every ~5-6 seconds, so 100 lines keeps 2-3 full cycles
        // visible without being unreadable.
        assert_eq!(MAX_LOG_LINES, 100);
    }

    #[test]
    fn keeps_all_logs_unmodified_when_under_budget() {
        let mut logs = vec![
            make_log("stagepilot-backend.log", "recent content"),
            make_log("stagepilot-backend.log.1", "older content"),
        ];
        truncate_logs_to_budget(&mut logs, DIAGNOSTIC_BUNDLE_MAX_BYTES, None);
        assert_eq!(logs[0].content, "recent content");
        assert_eq!(logs[1].content, "older content");
        assert!(!logs[0].truncated);
        assert!(!logs[1].truncated);
    }

    #[test]
    fn truncates_oldest_log_first_when_over_budget() {
        // Budget only large enough for the newest log plus a sliver of the
        // older one.
        let newest = "x".repeat(100);
        let oldest = "y".repeat(200);
        let mut logs = vec![
            make_log("stagepilot-backend.log", &newest),
            make_log("stagepilot-backend.log.1", &oldest),
        ];
        let budget = 1024 + 150; // reserved (1024) + room for newest + 50 of oldest
        truncate_logs_to_budget(&mut logs, budget, None);

        // Newest log is preserved in full and untouched.
        assert_eq!(logs[0].content, newest);
        assert!(!logs[0].truncated);

        // Oldest log is truncated from the start, keeping its tail.
        assert!(logs[1].truncated);
        assert!(logs[1].content.len() < oldest.len());
        assert!(oldest.ends_with(&logs[1].content));
    }

    #[test]
    fn drops_oldest_log_entirely_before_touching_newest() {
        let newest = "keep-me".repeat(50);
        let oldest = "drop-me".repeat(500);
        let mut logs = vec![
            make_log("stagepilot-backend.log", &newest),
            make_log("stagepilot-backend.log.1", &oldest),
        ];
        // Budget just barely fits the newest log and nothing else.
        let budget = 1024 + newest.len();
        truncate_logs_to_budget(&mut logs, budget, None);

        assert_eq!(logs[0].content, newest);
        assert!(!logs[0].truncated);
        assert!(logs[1].content.is_empty());
        assert!(logs[1].truncated);
    }

    #[test]
    fn never_exceeds_the_total_byte_budget() {
        let a = "a".repeat(3000);
        let b = "b".repeat(3000);
        let c = "c".repeat(3000);
        let mut logs = vec![
            make_log("stagepilot-backend.log", &a),
            make_log("stagepilot-backend.log.1", &b),
            make_log("stagepilot-backend.log.2", &c),
        ];
        let budget = 4000;
        truncate_logs_to_budget(&mut logs, budget, None);
        let total: usize = logs.iter().map(|log| log.content.len()).sum();
        assert!(total <= budget);
    }

    #[test]
    fn reserves_room_for_frontend_crash_text() {
        let content = "z".repeat(5000);
        let mut logs = vec![make_log("stagepilot-backend.log", &content)];
        let crash = "x".repeat(2000);
        truncate_logs_to_budget(&mut logs, 4096, Some(&crash));
        // Reserved budget (crash text + 1024 overhead) leaves less than
        // 4096 for the log, so it must be truncated.
        assert!(logs[0].truncated);
        assert!(logs[0].content.len() < content.len());
    }

    #[test]
    fn detects_stagepilot_backend_in_windows_task_list_output() {
        assert!(tasklist_has_stagepilot_backend(
            br#""stagepilot-backend.exe","8420","Console","1","72,000 K""#,
        ));
        assert!(!tasklist_has_stagepilot_backend(
            b"INFO: No tasks are running which match the specified criteria.",
        ));
    }

    #[test]
    fn saved_server_port_is_validated() {
        assert_eq!(port_from_settings(r#"{"server_port": 9123}"#), Some(9123));
        assert_eq!(port_from_settings(r#"{"server_port": 0}"#), None);
        assert_eq!(port_from_settings(r#"{"server_port": 70000}"#), None);
        assert_eq!(port_from_settings("not-json"), None);
    }

    #[test]
    fn lan_access_requires_an_explicit_saved_setting() {
        assert!(lan_access_from_settings(r#"{"lan_access": true}"#));
        assert!(!lan_access_from_settings(r#"{"lan_access": false}"#));
        assert!(!lan_access_from_settings(r#"{"server_port": 8765}"#));
        assert!(!lan_access_from_settings("not-json"));
    }

    #[test]
    fn updater_configuration_preserves_stable_desktop_identity() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["productName"], "StagePilot");
        assert_eq!(config["identifier"], "org.stagepilot.desktop");
        assert_eq!(config["app"]["windows"][0]["label"], "main");
        assert_eq!(config["bundle"]["createUpdaterArtifacts"], true);
        assert_eq!(config["bundle"]["macOS"]["signingIdentity"], "-");
        let macos: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.macos.conf.json")).unwrap();
        assert_eq!(macos["bundle"]["macOS"]["hardenedRuntime"], false);
        assert_eq!(macos["bundle"]["macOS"]["minimumSystemVersion"], "12.0");
        assert_eq!(
            config["plugins"]["updater"]["endpoints"][0],
            "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json"
        );
        let windows: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.release.conf.json")).unwrap();
        let direct_github_endpoint =
            "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json";
        assert_eq!(
            windows["plugins"]["updater"]["endpoints"][0],
            direct_github_endpoint
        );
        assert_eq!(
            macos["plugins"]["updater"]["endpoints"][0],
            direct_github_endpoint
        );
        assert!(config["plugins"]["updater"]["pubkey"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
    }

    /// Packaged release configs (Windows/macOS overlays) must never point the
    /// updater at the dead control-plane release broker: `releaseAsset()` in
    /// `control-plane/src/index.ts` returns 503 whenever
    /// `GITHUB_RELEASE_TOKEN` is unset, which is the case today.
    /// Every packaged/release-config updater endpoint must be the direct,
    /// anonymous GitHub releases URL instead.
    #[test]
    fn packaged_release_configs_never_point_at_the_dead_control_plane_broker() {
        let dead_broker_endpoint =
            "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases/latest.json";
        let direct_github_endpoint =
            "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json";

        for (name, contents) in [
            ("tauri.conf.json", include_str!("../tauri.conf.json")),
            (
                "tauri.release.conf.json",
                include_str!("../tauri.release.conf.json"),
            ),
            (
                "tauri.macos.conf.json",
                include_str!("../tauri.macos.conf.json"),
            ),
        ] {
            let config: serde_json::Value = serde_json::from_str(contents).unwrap();
            let Some(endpoints) = config["plugins"]["updater"]["endpoints"].as_array() else {
                continue;
            };
            for endpoint in endpoints {
                let endpoint = endpoint.as_str().unwrap_or_default();
                assert_ne!(
                    endpoint, dead_broker_endpoint,
                    "{name} still points the updater at the dead control-plane broker"
                );
                assert_eq!(
                    endpoint, direct_github_endpoint,
                    "{name} updater endpoint should be the direct GitHub releases URL"
                );
            }
        }
    }

    #[test]
    fn timeout_only_replaces_a_starting_state() {
        assert!(timeout_may_replace(&BackendState::Starting));
        assert!(!timeout_may_replace(&BackendState::Failed));
        assert!(!timeout_may_replace(&BackendState::Ready));
        assert!(!timeout_may_replace(&BackendState::Stopped));
    }

    #[test]
    fn managed_exit_is_reported_before_or_after_readiness() {
        assert!(managed_exit_should_fail(&BackendState::Starting));
        assert!(managed_exit_should_fail(&BackendState::Ready));
        assert!(!managed_exit_should_fail(&BackendState::Failed));
        assert!(!managed_exit_should_fail(&BackendState::Stopped));
        assert!(!managed_exit_should_fail(&BackendState::External));
    }

    #[test]
    fn macos_library_validation_failure_is_actionable() {
        let (kind, message) = backend_exit_failure(
            Some(255),
            "Failed to load Python shared library: mapped file has no Team ID and is not valid for use in process",
            Some("/tmp/stagepilot-backend.log"),
        );
        assert_eq!(kind, BackendFailureKind::MacosCodeSigning);
        assert!(message.contains("blocked by macOS code-signing policy"));
        assert!(message.contains("/tmp/stagepilot-backend.log"));
    }

    #[test]
    fn ordinary_early_exit_includes_code_and_log() {
        let (kind, message) =
            backend_exit_failure(Some(7), "ordinary error", Some("/tmp/backend.log"));
        assert_eq!(kind, BackendFailureKind::SidecarExited);
        assert!(message.contains("exit code 7"));
        assert!(message.contains("/tmp/backend.log"));
    }

    #[test]
    fn crash_log_entry_contains_structured_fields_and_redacts_secrets() {
        let recent = "starting up\nAuthorization: Bearer super-secret-token\nboom";
        let entry = BackendCrashLogEntry::new(
            "1.2.3",
            BackendFailureKind::SidecarExited,
            "The packaged StagePilot backend exited before it became ready (exit code 1).",
            recent,
        );
        let rendered = entry.render();
        assert!(rendered.contains("app_version: 1.2.3"));
        assert!(rendered.contains(&format!("os: {}", env::consts::OS)));
        assert!(rendered.contains("failure_kind: SidecarExited"));
        assert!(rendered.contains("exit code 1"));
        assert!(rendered.contains("boom"));
        assert!(!rendered.contains("super-secret-token"));
        assert!(rendered.contains("REDACTED"));
        // ISO-8601 timestamp: YYYY-MM-DDTHH:MM:SSZ
        assert!(rendered.contains("timestamp: 2"));
        assert!(rendered.contains('T'));
        assert!(rendered.contains('Z'));
    }

    #[test]
    fn write_crash_log_entry_persists_structured_failure_to_disk() {
        let dir = std::env::temp_dir().join(format!(
            "stagepilot-crash-log-test-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("stagepilot-backend.log");
        let log_path_text = log_path.to_string_lossy().into_owned();

        write_crash_log_entry(
            Some(&log_path_text),
            "9.9.9",
            BackendFailureKind::MacosCodeSigning,
            "macOS blocked the backend.",
            "access_token=abc123\nharmless line",
        );

        let contents = fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("app_version: 9.9.9"));
        assert!(contents.contains("failure_kind: MacosCodeSigning"));
        assert!(contents.contains("macOS blocked the backend."));
        assert!(contents.contains("harmless line"));
        assert!(!contents.contains("abc123"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn secrets_are_redacted_from_crash_log_output() {
        let text = "line one\naccess_token=super-secret\nclient_secret: also-secret\nfine line";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("super-secret"));
        assert!(!redacted.contains("also-secret"));
        assert!(redacted.contains("fine line"));
        assert!(redacted.contains("line one"));
    }

    #[test]
    fn three_failures_within_two_minutes_trigger_crash_loop_detection() {
        let mut history = VecDeque::new();
        let base = Instant::now();
        let first = base;
        let second = base + Duration::from_secs(30);
        let third = base + Duration::from_secs(60);

        history.push_back(first);
        prune_failure_history(&mut history, first);
        assert!(!crash_loop_qualifies(&history));

        history.push_back(second);
        prune_failure_history(&mut history, second);
        assert!(!crash_loop_qualifies(&history));

        history.push_back(third);
        prune_failure_history(&mut history, third);
        assert!(
            crash_loop_qualifies(&history),
            "3 failures within 2 minutes should qualify as a crash loop"
        );
    }

    #[test]
    fn two_failures_more_than_two_minutes_apart_do_not_trigger_crash_loop() {
        let mut history = VecDeque::new();
        let base = Instant::now();
        let first = base;
        let second = base + Duration::from_secs(150); // > CRASH_LOOP_WINDOW (120s)

        history.push_back(first);
        prune_failure_history(&mut history, first);
        assert!(!crash_loop_qualifies(&history));

        history.push_back(second);
        // Pruning at `second` should drop `first` since it's now more than
        // CRASH_LOOP_WINDOW old, leaving only the single recent failure.
        prune_failure_history(&mut history, second);
        assert_eq!(history.len(), 1);
        assert!(!crash_loop_qualifies(&history));
    }

    #[test]
    fn crash_loop_alert_only_fires_once_per_qualifying_window_and_rearms_later() {
        let supervisor = BackendSupervisor::new(8765);
        let base = Instant::now();

        // First two failures: below threshold, no alert.
        assert!(!supervisor.note_failure_and_check_crash_loop(base));
        assert!(!supervisor.note_failure_and_check_crash_loop(base + Duration::from_secs(10)));
        // Third failure within the window: crosses the threshold, alert fires.
        assert!(supervisor.note_failure_and_check_crash_loop(base + Duration::from_secs(20)));
        // A 4th failure still inside/related to the same window must not
        // re-emit the alert.
        assert!(!supervisor.note_failure_and_check_crash_loop(base + Duration::from_secs(25)));

        // After the original failures age out of the rolling window and
        // failures stop occurring for a while, a fresh 3rd-in-2-minutes
        // pattern should be able to alert again.
        let far_future = base + Duration::from_secs(400);
        assert!(!supervisor.note_failure_and_check_crash_loop(far_future));
        assert!(!supervisor.note_failure_and_check_crash_loop(far_future + Duration::from_secs(10)));
        assert!(supervisor.note_failure_and_check_crash_loop(far_future + Duration::from_secs(20)));
    }

    #[test]
    fn desktop_capability_has_only_required_update_permissions() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let permissions = capability["permissions"].as_array().unwrap();
        let has = |permission: &str| permissions.iter().any(|value| value == permission);
        assert!(has("updater:default"));
        assert!(has("process:allow-restart"));
        assert!(has("window-state:default"));
        assert!(!has("shell:allow-execute"));
        assert!(!has("fs:default"));
    }

    #[test]
    fn native_menu_actions_are_mapped_without_capturing_system_items() {
        assert_eq!(
            stagepilot_menu_action("restart-stagepilot"),
            Some(StagePilotMenuAction::Restart)
        );
        assert_eq!(
            stagepilot_menu_action("minimize-stagepilot"),
            Some(StagePilotMenuAction::Minimize)
        );
        assert_eq!(
            stagepilot_menu_action("toggle-fullscreen-stagepilot"),
            Some(StagePilotMenuAction::ToggleFullscreen)
        );
        assert_eq!(stagepilot_menu_action("quit-stagepilot"), None);
    }

    #[test]
    fn taskbar_launch_arguments_map_only_stagepilot_lifecycle_actions() {
        let restart = vec![
            "stagepilot.exe".to_string(),
            "--stagepilot-restart".to_string(),
        ];
        let quit = vec![
            "stagepilot.exe".to_string(),
            "--stagepilot-quit".to_string(),
        ];
        let ordinary = vec!["stagepilot.exe".to_string(), "--unrelated".to_string()];
        assert_eq!(
            stagepilot_launch_action(&restart),
            Some(StagePilotLaunchAction::Restart)
        );
        assert_eq!(
            stagepilot_launch_action(&quit),
            Some(StagePilotLaunchAction::Quit)
        );
        assert_eq!(stagepilot_launch_action(&ordinary), None);
    }

    #[test]
    fn unused_local_port_is_available() {
        // Binding an ephemeral port, dropping the listener, and then probing
        // that exact port is inherently racy: the OS (or another process on
        // the machine) can reclaim the port in the window between drop and
        // probe, especially under CI load. Try several independent
        // candidate ports and require at least one to probe `Available`
        // rather than pinning the assertion to a single port number.
        const ATTEMPTS: usize = 20;
        let mut observed_occupied = 0;
        for _ in 0..ATTEMPTS {
            let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            match probe_port(port) {
                PortProbe::Available => return,
                PortProbe::Occupied | PortProbe::StagePilot => {
                    observed_occupied += 1;
                }
            }
        }
        panic!(
            "expected at least one of {ATTEMPTS} freshly-dropped ephemeral ports to probe \
             Available, but all {observed_occupied} attempts probed Occupied"
        );
    }

    #[test]
    fn stagepilot_health_response_is_identified() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request);
            let body = r#"{"version":"0.1.0","application_status":"running"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        assert_eq!(probe_port(port), PortProbe::StagePilot);
        server.join().unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn nested_backend_process_tree_is_terminated() {
        let mut root = std::process::Command::new("/bin/sh")
            .args(["-c", "/bin/sh -c '/bin/sleep 30 & wait' & wait"])
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let descendants = loop {
            let descendants = descendant_process_ids(root.id());
            if descendants.len() >= 2 || Instant::now() >= deadline {
                break descendants;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(
            descendants.len() >= 2,
            "expected a nested child process tree"
        );

        terminate_process_tree(root.id());
        let _ = root.wait();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && (process_exists(root.id()) || descendants.iter().any(|pid| process_exists(*pid)))
        {
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(!process_exists(root.id()));
        assert!(descendants.iter().all(|pid| !process_exists(*pid)));
    }

    #[test]
    fn unrelated_listener_is_reported_as_occupied() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .unwrap();
        });
        assert_eq!(probe_port(port), PortProbe::Occupied);
        server.join().unwrap();
    }
}

/// The installers are built with `--features in-process` on Windows and macOS. If MIDI hardware
/// support were off there, the server would refuse to start on any settings that send MIDI
/// (v2.42.0-rc.3 shipped like that). Unit tests of other crates cannot see this: it is decided by
/// how the desktop crate depends on the server.
#[cfg(all(test, feature = "in-process", any(windows, target_os = "macos")))]
mod shipped_build_tests {
    #[test]
    // The value is a compile-time constant on purpose: that is exactly what this test pins.
    #[allow(clippy::assertions_on_constants)]
    fn midi_hardware_support_is_compiled_into_the_shipped_build() {
        assert!(
            stagepilot_midi::HARDWARE_SUPPORT,
            "the in-process desktop build must enable stagepilot-server/midir-backend"
        );
    }
}
