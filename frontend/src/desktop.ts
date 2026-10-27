import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";

export type BackendSupervisorStatus = {
  state: "starting" | "ready" | "external" | "failed" | "stopped";
  message: string;
  port: number;
  managed: boolean;
  failure_kind?:
    | "port_occupied"
    | "server_exited"
    | "timeout"
    | null;
  log_path?: string | null;
};

export const backendStartupTitle = (status: BackendSupervisorStatus | null): string => {
  if (!status) return "Connecting to the local backend";
  if (status.state === "ready" || status.state === "external") return "Backend connected";
  if (status.state === "starting") return "Starting the local backend";
  if (status.state === "stopped") return "Backend stopped";
  switch (status.failure_kind) {
    case "port_occupied":
      return "Backend port is occupied";
    case "server_exited":
      return "The StagePilot server stopped";
    case "timeout":
      return "Backend startup timed out";
    default:
      return "Backend startup failed";
  }
};

export const desktopBackendStatus = async (): Promise<BackendSupervisorStatus | null> => {
  if (!isTauri()) return null;
  return invoke<BackendSupervisorStatus>("backend_supervisor_status");
};

export const listenForDesktopBackend = async (
  onStatus: (status: BackendSupervisorStatus) => void,
): Promise<UnlistenFn | null> => {
  if (!isTauri()) return null;
  return listen<BackendSupervisorStatus>("stagepilot://backend-status", (event) => {
    onStatus(event.payload);
  });
};

export type BackendCrashLoopDetected = {
  failure_kind:
    | "port_occupied"
    | "server_exited"
    | "timeout"
    | null;
  message: string;
};

/// Mirrors `listenForDesktopBackend` but for the repeat-crash-loop alert.
/// Intentionally independent from the startup-gated backend-status
/// listener so callers can mount it at the app root, outside any
/// dashboardVisible gate, and still receive the event whether the user is
/// on the startup screen or the live dashboard.
export const listenForDesktopBackendCrashLoop = async (
  onCrashLoop: (payload: BackendCrashLoopDetected) => void,
): Promise<UnlistenFn | null> => {
  if (!isTauri()) return null;
  return listen<BackendCrashLoopDetected>(
    "stagepilot://backend-crash-loop-detected",
    (event) => {
      onCrashLoop(event.payload);
    },
  );
};

export const copyBackendLog = async (): Promise<string> => {
  if (!isTauri()) throw new Error("Backend log copy is only available in the desktop app.");
  return invoke<string>("copy_backend_log");
};

/// Gathers backend logs + app/version info (and, when supplied, the
/// frontend error that triggered the request) into a single JSON/text
/// blob suitable for `sendDiagnosticsBundle` in `./diagnostics`.
///
/// INTEGRATION ASSUMPTION: the Tauri command `collect_diagnostic_bundle`
/// is being built in a parallel task (t_40dd46f5). Assumed signature:
/// `invoke("collect_diagnostic_bundle", { frontendError?: string }) => Promise<string>`.
export const collectDiagnosticBundle = async (frontendError?: string): Promise<string> => {
  if (!isTauri()) throw new Error("Diagnostic collection is only available in the desktop app.");
  return invoke<string>("collect_diagnostic_bundle", { frontendError });
};


export const isDesktopShell = () => isTauri();

export const minimizeDesktopWindow = async () => {
  if (!isTauri()) return;
  await getCurrentWindow().minimize();
};

export const toggleMaximizeDesktopWindow = async () => {
  if (!isTauri()) return;
  await getCurrentWindow().toggleMaximize();
};

export const hideDesktopWindow = async () => {
  if (!isTauri()) return;
  await invoke("hide_application_window");
};

export const restartDesktopBackend = async (): Promise<boolean> => {
  if (!isTauri()) return false;
  await invoke<BackendSupervisorStatus>("restart_managed_backend");
  return true;
};

export const setRemoteAutostart = async (enabled: boolean): Promise<void> => {
  if (!isTauri()) return;
  await invoke("set_remote_autostart", {enabled});
};

export interface PlanningCenterSignInResult {
  code: string;
  redirect_uri: string;
}

/// Opens the system browser to Planning Center's consent page and waits
/// on the desktop's single-use loopback listener for the authorization
/// code. Only available in the desktop shell (Tauri); the OAuth flow has
/// no browser build equivalent because it needs the local loopback port.
export const signInWithPlanningCenter = async (
  authorizeUrlPrefix: string,
  state: string,
): Promise<PlanningCenterSignInResult> => {
  if (!isTauri()) {
    throw new Error("Signing in with Planning Center requires the StagePilot desktop app.");
  }
  return invoke<PlanningCenterSignInResult>("planning_center_sign_in", {
    authorizeUrlPrefix,
    state,
  });
};

export const openExternalUrl = async (url: string): Promise<void> => {
  if (isTauri()) {
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
};
