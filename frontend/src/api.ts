import { isDesktopShell } from "./desktop";
import { accessGeneration, apiAccess, invalidateAccess } from "./access/accessState";
import type {
  ActionName,
  ActionResponse,
  ApplicationState,
  DashboardAuthStatus,
  DashboardAccess,
  HealthResponse,
  LengthPreview,
  LightsOperationResponse,
  LightsSettingsInput,
  LightsStatusResponse,
  MidiCueName,
  MidiCueSimulationResponse,
  MidiInputSelectionResponse,
  MidiInputsResponse,
  MidiMonitorResponse,
  PersistentSettings,
  PlaybackEventsResponse,
  PlaybackSettingsInput,
  PlaybackStatusResponse,
  PlanningCenterOAuthStartResponse,
  PlanningCenterOAuthStatusResponse,
  PlanningCenterServiceType,
  PlanningCenterSettingsInput,
  PlanningCenterStatusResponse,
  PlanningCenterTestInput,
  PlanningCenterTestResponse,
  PlanSelectionResponse,
  ProPresenterOperationResponse,
  ProPresenterSettingsInput,
  ProPresenterStatusResponse,
  SettingsResponse,
  SongLightingCueMap,
} from "./types";

const SERVER_PORT_KEY = "stagepilot.server-port";
const configuredOrigin = import.meta.env.VITE_STAGEPILOT_API_URL as string | undefined;

const savedServerPort = () => {
  try {
    const value = Number(window.localStorage.getItem(SERVER_PORT_KEY));
    return Number.isInteger(value) && value >= 1 && value <= 65535 ? value : 8765;
  } catch {
    return 8765;
  }
};

export const rememberServerPort = (port: number) => {
  try {
    window.localStorage.setItem(SERVER_PORT_KEY, String(port));
  } catch {
    // A restricted browser storage policy should not prevent settings persistence.
  }
};

export function resolveApiOrigin(
  location: Pick<Location, "protocol" | "origin" | "hostname" | "port">,
  desktop: boolean,
  development: boolean,
  configured: string | undefined,
  serverPort: number,
): string {
  const viteDevelopment = development && location.port === "5173"
    && ["localhost", "127.0.0.1", "[::1]"].includes(location.hostname);
  const hosted = !desktop && !viteDevelopment && ["http:", "https:"].includes(location.protocol);
  return (configured ?? (hosted ? location.origin : `http://127.0.0.1:${serverPort}`)).replace(/\/$/, "");
}

export const apiOrigin = resolveApiOrigin(
  window.location, isDesktopShell(), import.meta.env.DEV, configuredOrigin, savedServerPort(),
);
export const websocketUrl = `${apiOrigin.replace(/^http/, "ws")}/ws`;

export class ApiError extends Error {
  constructor(message: string, readonly status: number) { super(message); }
}

const accessEndpoints = new Set([
  "/api/v1/access", "/api/v1/dashboard-auth/status", "/api/v1/dashboard-auth/login",
  "/api/v1/remote-auth/login", "/api/v1/remote-auth/session",
]);
const ownMutations = new Set(["/api/v1/remote-auth/logout", "/api/v1/remote-auth/revoke-sessions"]);
const viewerReads = new Set([
  "/api/v1/state", "/api/v1/health", "/api/v1/health/live", "/api/v1/health/ready",
]);

async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const access = apiAccess();
  const generation = accessGeneration();
  const mutation = !["GET", "HEAD", "OPTIONS"].includes(init?.method ?? "GET");
  if (access && !accessEndpoints.has(path)) {
    if (!access.authenticated) throw new ApiError("Please sign in to continue.", 401);
    if ((!access.capabilities.canOperate && mutation && !ownMutations.has(path))
      || (!access.capabilities.canConfigure && !mutation && !viewerReads.has(path))) {
      throw new ApiError("This session has read-only access.", 403);
    }
  }
  const response = await fetch(`${apiOrigin}${path}`, {
    ...init,
    credentials: "include",
    headers: {
      Accept: "application/json",
      ...(mutation && access?.mode === "remote" && !path.startsWith("/api/v1/remote-")
        ? { "Idempotency-Key": crypto.randomUUID() } : {}),
      ...(mutation && access?.mode === "remote" && access.csrf_token
        ? { "X-CSRF-Token": access.csrf_token } : {}),
      ...init?.headers,
    },
  });
  if (!response.ok) {
    let detail: string | null = null;
    try {
      const payload = (await response.json()) as { detail?: unknown };
      if (typeof payload.detail === "string") detail = payload.detail;
    } catch {
      // Fall back to the status-only message when the server did not return JSON.
    }
    if ((response.status === 401 || response.status === 403) && !accessEndpoints.has(path)) {
      invalidateAccess(generation);
    }
    throw new ApiError(detail ?? `StagePilot API returned ${response.status}.`, response.status);
  }
  return response.status === 204 ? undefined as T : (await response.json()) as T;
}

export const getAccess = () => requestJson<DashboardAccess>("/api/v1/access", { cache: "no-store" });
export const loginRemote = (email: string, password: string) =>
  requestJson<{ authenticated: boolean; csrf_token: string }>("/api/v1/remote-auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-StagePilot-Remote": "1" },
    body: JSON.stringify({ email, password }),
  });
export const logoutRemote = () => requestJson<void>("/api/v1/remote-auth/logout", { method: "POST" });

export interface RemoteStatus {
  available: boolean;
  provisioned: boolean;
  credential_available: boolean;
  enabled: boolean;
  state: "off" | "enabling" | "connected" | "reconnecting" | "error";
  url: string | null;
  needs_operator: boolean;
  message: string | null;
  temporary_url: boolean;
  permanently_revoked: boolean;
}
export interface RemoteUser {
  id: string; email: string; role: "Viewer" | "Operator"; enabled: boolean;
}
export const getRemoteStatus = () => requestJson<RemoteStatus>("/api/v1/remote-access", {cache: "no-store"});
export const getRemoteUsers = () => requestJson<RemoteUser[]>("/api/v1/remote-auth/users", {cache: "no-store"});
const remoteMutation = <T>(path: string, method: string, body?: object) => requestJson<T>(path, {
  method, headers: {"Content-Type": "application/json", "X-StagePilot-Remote": "1"},
  ...(body ? {body: JSON.stringify(body)} : {}),
});
export const setRemoteEnabled = (enabled: boolean) => remoteMutation<RemoteStatus>(
  `/api/v1/remote-access/${enabled ? "enable" : "disable"}`, "POST", {});
export const regenerateRemote = () => remoteMutation<RemoteStatus>(
  "/api/v1/remote-access/regenerate", "POST", {});
export const resetRemoteIdentity = () => remoteMutation<RemoteStatus>(
  "/api/v1/remote-access/reset", "POST", {});
export const bootstrapRemote = (email: string, password: string) => remoteMutation<RemoteUser>(
  "/api/v1/remote-access/bootstrap", "POST", {email, password});
export const createRemoteUser = (email: string, password: string, role: RemoteUser["role"]) =>
  remoteMutation<RemoteUser>("/api/v1/remote-auth/users", "POST", {email, password, role});
export const updateRemoteUser = (id: string, body: {role?: RemoteUser["role"]; enabled?: boolean; password?: string}) =>
  remoteMutation<void>(`/api/v1/remote-auth/users/${encodeURIComponent(id)}`, "PATCH", body);
export const deleteRemoteUser = (id: string) =>
  remoteMutation<void>(`/api/v1/remote-auth/users/${encodeURIComponent(id)}`, "DELETE");

export const getDashboardAuthStatus = () =>
  requestJson<DashboardAuthStatus>("/api/v1/dashboard-auth/status");
export const loginDashboard = (pin: string) =>
  requestJson<DashboardAuthStatus>("/api/v1/dashboard-auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pin }),
  });
export const updateDashboardAccess = (enabled: boolean, pin?: string) =>
  requestJson<SettingsResponse>("/api/v1/dashboard-auth/settings", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled, ...(pin ? { pin } : {}) }),
  });

export const getHealth = () => requestJson<HealthResponse>("/api/v1/health");
export const getState = () => requestJson<ApplicationState>("/api/v1/state");
export const performAction = (action: ActionName) =>
  requestJson<ActionResponse>(`/api/v1/actions/${action}`, { method: "POST" });
export const selectPlanningCenterPlan = (planId: string) =>
  requestJson<PlanSelectionResponse>("/api/v1/planning-center/plans/select", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ plan_id: planId }),
  });
export const getSettings = () => requestJson<SettingsResponse>("/api/v1/settings");
export const updateSettings = (settings: PersistentSettings) =>
  requestJson<SettingsResponse>("/api/v1/settings", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });
export const getPlanningCenterStatus = () =>
  requestJson<PlanningCenterStatusResponse>("/api/v1/planning-center/status");
export const testPlanningCenter = (settings: PlanningCenterTestInput) =>
  requestJson<PlanningCenterTestResponse>("/api/v1/planning-center/test", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });
export const getPlanningCenterServiceTypes = () =>
  requestJson<PlanningCenterServiceType[]>("/api/v1/planning-center/service-types");
export const updatePlanningCenterSettings = (settings: PlanningCenterSettingsInput) =>
  requestJson<SettingsResponse>("/api/v1/planning-center/settings", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });

export const startPlanningCenterOAuth = () =>
  requestJson<PlanningCenterOAuthStartResponse>("/api/v1/planning-center/oauth/start", {
    method: "POST",
  });
export const completePlanningCenterOAuth = (
  state: string,
  code: string,
  redirectUri: string,
) =>
  requestJson<PlanningCenterOAuthStatusResponse>("/api/v1/planning-center/oauth/callback", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ state, code, redirect_uri: redirectUri }),
  });
export const disconnectPlanningCenterOAuth = () =>
  requestJson<PlanningCenterOAuthStatusResponse>("/api/v1/planning-center/oauth/disconnect", {
    method: "POST",
  });

export const getMidiInputs = () => requestJson<MidiInputsResponse>("/api/v1/midi/inputs");
export const getPlaybackStatus = () => requestJson<PlaybackStatusResponse>("/api/v1/playback-api/status");
export const getPlaybackEvents = () => requestJson<PlaybackEventsResponse>("/api/v1/playback-api/events");
export const findPlayback = (host?: string) =>
  requestJson<PlaybackStatusResponse>("/api/v1/playback-api/find", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(host ? { host } : {}),
  });
export const cancelPlaybackScan = () =>
  requestJson<PlaybackStatusResponse>("/api/v1/playback-api/find/cancel", { method: "POST" });
export const updatePlaybackSettings = (settings: PlaybackSettingsInput) =>
  requestJson<PlaybackStatusResponse>("/api/v1/playback-api/settings", {
    method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(settings),
  });
export const updatePlaybackArrangement = (skipped_song_ids: number[], skipped_plan_song_ids: string[]) =>
  requestJson<PlaybackStatusResponse>("/api/v1/playback-api/arrangement", {
    method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ skipped_song_ids, skipped_plan_song_ids }),
  });
export const discoverPlaybackSongOrder = () =>
  requestJson<PlaybackStatusResponse>("/api/v1/playback-api/discover-song-order", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ confirm: true }),
  });
const lengthPath = "/api/v1/playback-api/planning-center-lengths";
export const getLengthCategories = () => requestJson<PlanningCenterServiceType[]>(`${lengthPath}/categories`);
export const chooseLengthCategory = (service_type_id: string) => requestJson<PlaybackStatusResponse>(`${lengthPath}/category`, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ service_type_id }) });
export const previewPlanLengths = () => requestJson<LengthPreview>(`${lengthPath}/preview`, { method: "POST" });
export const confirmPlanLengths = (token: string) => requestJson<PlaybackStatusResponse>(`${lengthPath}/confirm`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ token, confirm: true }) });
export const restorePlanLengths = () => requestJson<PlaybackStatusResponse>(`${lengthPath}/restore`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ confirm: true }) });
export const getMidiMessages = () => requestJson<MidiMonitorResponse>("/api/v1/midi/messages");
export const refreshMidiInputs = () =>
  requestJson<MidiInputsResponse>("/api/v1/midi/inputs/refresh", { method: "POST" });
export const selectMidiInput = (inputId: string | null) =>
  requestJson<MidiInputSelectionResponse>("/api/v1/midi/input-selection", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ input_id: inputId }),
  });
export const simulateMidiCue = (cue: MidiCueName) =>
  requestJson<MidiCueSimulationResponse>("/api/v1/midi/cue-simulation", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ cue }),
  });

export const getProPresenterStatus = () =>
  requestJson<ProPresenterStatusResponse>("/api/v1/propresenter");
export const testProPresenter = () =>
  requestJson<ProPresenterOperationResponse>("/api/v1/propresenter/test", {
    method: "POST",
  });
export const refreshProPresenterTimers = () =>
  requestJson<ProPresenterOperationResponse>("/api/v1/propresenter/timers/refresh", {
    method: "POST",
  });
export const updateProPresenterSettings = (settings: ProPresenterSettingsInput) =>
  requestJson<ProPresenterOperationResponse>("/api/v1/propresenter/settings", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });

export const getLightsStatus = () =>
  requestJson<LightsStatusResponse>("/api/v1/lights");
export const refreshLightingOutputs = () =>
  requestJson<LightsStatusResponse>("/api/v1/lights/outputs/refresh", {
    method: "POST",
  });
export const updateLightsSettings = (settings: LightsSettingsInput) =>
  requestJson<LightsOperationResponse>("/api/v1/lights/settings", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });
export const updateLightingCueMap = (cueMap: SongLightingCueMap) =>
  requestJson<LightsOperationResponse>("/api/v1/lights/cue-map", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(cueMap),
  });
export const testLightingCue = (note: number, velocity: number) =>
  requestJson<LightsOperationResponse>("/api/v1/lights/test", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ note, velocity }),
  });
