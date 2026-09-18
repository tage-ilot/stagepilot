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
use tauri_plugin_shell::{
    process::{CommandChild, CommandEvent},
    ShellExt,
};
use tauri_plugin_updater::UpdaterExt;
use tauri_plugin_window_state::{StateFlags, WindowExt};

#[cfg(target_os = "windows")]
mod windows_jump_list;

const DEFAULT_PORT: u16 = 8765;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
const RECENT_BACKEND_LINES: usize = 32;
const BACKEND_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
const STAGEPILOT_GITHUB_URL: &str = "https://github.com/tage-ilot/stagepilot";
const STABLE_UPDATE_ENDPOINT: &str =
    "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json";
const BETA_UPDATE_ENDPOINT: &str =
    "https://github.com/tage-ilot/stagepilot-beta/releases/latest/download/latest.json";

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
    MacosCodeSigning,
    Timeout,
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
    child: Arc<Mutex<Option<CommandChild>>>,
    child_pid: Arc<Mutex<Option<u32>>>,
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
            child: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
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
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn signal_process(pid: u32, signal: &str) {
    let _ = std::process::Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status();
}

#[cfg(target_os = "macos")]
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
        let previous = path.with_extension("log.1");
        let _ = fs::remove_file(&previous);
        let _ = fs::rename(path, previous);
    }
}

fn recent_backend_text(lines: &VecDeque<String>) -> String {
    lines.iter().cloned().collect::<Vec<_>>().join("\n")
}

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
    let command = app
        .shell()
        .sidecar("stagepilot-backend")
        .map_err(|error| format!("Unable to locate the packaged backend sidecar: {error}"))?
        .env("STAGEPILOT_HOST", bind_host)
        .env("STAGEPILOT_PORT", port.to_string())
        .env("STAGEPILOT_SETTINGS_PATH", settings_path());
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
                        event_supervisor.fail(
                            &event_app,
                            kind,
                            message,
                            true,
                            event_log_path.clone(),
                        );
                    }
                    break;
                }
                _ => {}
            }
        }
    });
    wait_for_backend(app.clone(), supervisor, port, backend_log_path_text);
    Ok(())
}

#[tauri::command]
fn backend_supervisor_status(
    supervisor: tauri::State<'_, BackendSupervisor>,
) -> BackendSupervisorStatus {
    supervisor.snapshot()
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

/// Metadata describing an update found by [`check_for_update_on_channel`].
///
/// This mirrors the plugin's own (private) `Metadata` shape exactly, since
/// the frontend reconstructs the plugin's public `Update` class from it and
/// then drives the plugin's own `download`/`install` IPC commands by
/// resource id — the same signature-verified path the plugin's default
/// `check` command hands to JS.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelUpdateMetadata {
    rid: tauri::ResourceId,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    raw_json: serde_json::Value,
}

/// Checks for an update using a dynamically selected endpoint (stable or
/// beta) instead of the static `tauri.conf.json` endpoint list.
///
/// This still goes through `updater_builder()` — the same builder the
/// plugin's own default `check` IPC command uses internally — so signature
/// verification is identical to the default path; only the endpoint list
/// changes. Signature verification itself is never bypassed or reimplemented,
/// and the resulting `Update` resource is registered the same way the
/// plugin's own `check` command registers it, so downloads/installs continue
/// to go through the plugin's verified IPC commands.
#[tauri::command]
async fn check_for_update_on_channel(
    webview: tauri::Webview,
    beta_enabled: bool,
) -> Result<Option<ChannelUpdateMetadata>, String> {
    let endpoint = if beta_enabled {
        BETA_UPDATE_ENDPOINT
    } else {
        STABLE_UPDATE_ENDPOINT
    };
    let url = url::Url::parse(endpoint).map_err(|error| {
        format!("StagePilot could not parse the update endpoint {endpoint}: {error}")
    })?;

    let updater = webview
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|error| format!("StagePilot could not configure the update channel: {error}"))?
        .build()
        .map_err(|error| format!("StagePilot could not build the updater: {error}"))?;

    let update = updater
        .check()
        .await
        .map_err(|error| format!("StagePilot could not check for updates: {error}"))?;

    Ok(update.map(|update| {
        let metadata = ChannelUpdateMetadata {
            current_version: update.current_version.clone(),
            version: update.version.clone(),
            date: update.date.map(|date| date.to_string()),
            body: update.body.clone(),
            raw_json: update.raw_json.clone(),
            rid: 0,
        };
        let rid = webview.resources_table().add(update);
        ChannelUpdateMetadata { rid, ..metadata }
    }))
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
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            if let Some(action) = stagepilot_launch_action(&arguments) {
                perform_launch_action(app, action);
            } else {
                let _ = restore_main_window(app, false);
            }
        }))
        .plugin(tauri_plugin_shell::init())
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
            prepare_for_update,
            check_for_update_on_channel,
            hide_application_window
        ])
        .setup(move |app| {
            if initial_launch_action == Some(StagePilotLaunchAction::Quit) {
                app.handle().exit(0);
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            install_application_menu(app.handle()).map_err(std::io::Error::other)?;
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
        assert!(config["plugins"]["updater"]["pubkey"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
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
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert_eq!(probe_port(port), PortProbe::Available);
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
