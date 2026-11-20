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
  LibraryPlan,
  LibraryStatus,
  PlaybackHelperInstall,
  SongWaveform,
  LightingCaptureResponse,
  LightingPad,
  LightkeyActionResponse,
  LightkeyHelperInstall,
  LightkeyReadFixtures,
  LightkeyAudio,
  LightkeyMode,
  LightkeyStatus,
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
  UpcomingServicePlans,
  ProPresenterOperationResponse,
  ProPresenterSettingsInput,
  ProPresenterStatusResponse,
  SettingsResponse,
  SongLightingCueMapInput,
  OscArgument,
} from "./types";

import type {
  AddSongsJob,
  AddSongsPlan,
  AddSongsPreviewRequest,
  AddSongsStarted,
  LinkStatus,
  ProPresenterPlaylist,
} from "./propresenterLink";

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
  /** `code` is the server's short reason (`song_playing`, `plan_changed`...) when it sent one. Only the window's logic reads it; never shown. */
  constructor(message: string, readonly status: number, readonly code?: string) { super(message); }
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
    let code: string | undefined;
    try {
      const payload = (await response.json()) as { detail?: unknown; code?: unknown };
      if (typeof payload.detail === "string") detail = payload.detail;
      if (typeof payload.code === "string") code = payload.code;
    } catch {
      // Fall back to the status-only message when the server did not return JSON.
    }
    if ((response.status === 401 || response.status === 403) && !accessEndpoints.has(path)) {
      invalidateAccess(generation);
    }
    throw new ApiError(detail ?? `StagePilot API returned ${response.status}.`, response.status, code);
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
// The service date picker: the plans from today on, picking one other than the upcoming service, and going back.
export const getUpcomingServicePlans = () =>
  requestJson<UpcomingServicePlans>("/api/v1/planning-center/plans/upcoming", { cache: "no-store" });
export const pinServicePlan = (planId: string) =>
  requestJson<PlanSelectionResponse>("/api/v1/planning-center/plans/pin", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ plan_id: planId }),
  });
export const unpinServicePlan = () =>
  requestJson<PlanSelectionResponse>("/api/v1/planning-center/plans/pin", { method: "DELETE" });
export const getSettings = () => requestJson<SettingsResponse>("/api/v1/settings");
// Lighting (captures, cue maps, Lightkey options) is saved by its own routes and by the server
// itself, so the general settings save never sends it back: a page opened earlier would erase it.
export const updateSettings = (settings: PersistentSettings) => {
  const body: Partial<PersistentSettings> = { ...settings };
  delete body.lights;
  return requestJson<SettingsResponse>("/api/v1/settings", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
};
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
/** "Scan Playback Songs" while the Library Link works: reads the Library Link fresh; nothing is stepped in Playback. */
export const refreshPlaybackSongOrderFromLibrary = () =>
  requestJson<PlaybackStatusResponse>("/api/v1/playback-api/song-order/refresh-from-library", { method: "POST" });
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
export const updateLightingCueMap = (cueMap: SongLightingCueMapInput) =>
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
export const testOscLightingCue = (address: string, args: OscArgument[]) =>
  requestJson<LightsOperationResponse>("/api/v1/lights/test-osc", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ address, args }),
  });

export const updateLightingPads = (pads: LightingPad[]) =>
  requestJson<LightsOperationResponse>("/api/v1/lights/pads", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pads }),
  });

/** Fires the pad. With `capture`, the backend also adds the cue to the song at `at_seconds`. */
export const captureLightingPad = (
  padId: string,
  capture?: { song_key: string; song_title: string; at_seconds: number },
) =>
  requestJson<LightingCaptureResponse>("/api/v1/lights/capture", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pad_id: padId, ...capture }),
  });

export const undoLightingCapture = (songKey: string, cueId: string) =>
  requestJson<LightsOperationResponse>("/api/v1/lights/capture/undo", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ song_key: songKey, cue_id: cueId }),
  });

export const getLightkeyStatus = () => requestJson<LightkeyStatus>("/api/v1/lights/lightkey");
export const saveLightkeySettings = (settings: { api_url?: string; replay?: boolean; mode?: LightkeyMode; audio_device?: string }) =>
  requestJson<LightkeyActionResponse>("/api/v1/lights/lightkey/settings", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });
/** Asks the Lightkey helper to read the open project's fixtures again. Always answers 200 with {ok, message, ...}. */
export const readLightkeyFixtures = () =>
  requestJson<LightkeyReadFixtures>("/api/v1/lights/lightkey/read-fixtures", { method: "POST" });
/** "Search again": look for the Lighting computer now (only while nothing is connected). */
export const searchLightkeyAgain = () =>
  requestJson<LightkeyActionResponse>("/api/v1/lights/lightkey/discovery/scan", { method: "POST" });
/** "Use this one" / "Use <ip>": switch to a Lighting computer the last search found. 422 when the address was not found. */
export const switchToLightkeyComputer = (address: string) =>
  requestJson<LightkeyActionResponse>("/api/v1/lights/lightkey/discovery/use", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ address }),
  });
/** "Keep waiting": stop offering that address until StagePilot restarts or the chosen address changes. */
export const keepWaitingForLightkey = (address: string) =>
  requestJson<LightkeyActionResponse>("/api/v1/lights/lightkey/discovery/dismiss", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ address }),
  });
/** Opens (or extends) the 30-minute setup session and answers the command to paste on the Lighting Mac. `address` is this computer's address when StagePilot could not find it itself (409). */
export const startLightkeyHelperInstall = (address?: string) =>
  requestJson<LightkeyHelperInstall>("/api/v1/lights/lightkey/helper-install", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(address ? { address } : {}),
  });
/** Ends the setup session (the command stops working at once). */
export const endLightkeyHelperInstall = () =>
  requestJson<{ ended: boolean }>("/api/v1/lights/lightkey/helper-install", { method: "DELETE" });
export const releaseLightkeyLights = () =>
  requestJson<LightkeyActionResponse>("/api/v1/lights/lightkey/release", { method: "POST" });
/** Turns the input meter on (it switches itself off after a few seconds) or off; answers the picker's data. */
export const setLightkeyAudioMeter = (on: boolean) =>
  requestJson<LightkeyAudio>("/api/v1/lights/lightkey/audio/meter", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ on }),
  });
export const clearLightingLoudness = (songKey: string) =>
  requestJson<LightsOperationResponse>("/api/v1/lights/loudness/clear", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ song_key: songKey }),
  });
export const setLightkeyRecording = (recording: boolean) =>
  requestJson<LightkeyActionResponse>("/api/v1/lights/lightkey/recording", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ recording }),
  });

// The optional Playback library (read-only; the one PUT saves its own setting and nothing else).
const libraryAsk = (path: string, check: boolean) => `/api/v1/playback-library/${path}${check ? "?check=1" : ""}`;
/** Where the window gets a song's cover (the backend serves it; the window never calls the helper). `?v=` is the picture's etag, so a changed picture is fetched again. */
export const playbackLibraryArtworkUrl = (setlistSongId: number, etag: string) =>
  `${apiOrigin}/api/v1/playback-library/songs/${setlistSongId}/artwork?v=${encodeURIComponent(etag)}`;
/** A song's waveform (the backend asks the helper only when this is called; 404 when the song has none). */
export const getPlaybackLibraryWaveform = (setlistSongId: number) =>
  requestJson<SongWaveform>(`/api/v1/playback-library/songs/${setlistSongId}/waveform`);
export const getPlaybackLibraryStatus = (check = false) => requestJson<LibraryStatus>(libraryAsk("status", check));
export const getPlaybackLibraryPlan = (check = false) => requestJson<LibraryPlan>(libraryAsk("plan", check));
/** Chooses which Playback setlist the loaded plan is matched with (null: the one on the plan's date). Kept in memory by the backend, for this plan only; nothing is sent to Playback or Planning Center. */
export const choosePlaybackLibrarySetlist = (setlistId: number | null) =>
  requestJson<LibraryPlan>("/api/v1/playback-library/setlist", {
    method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ setlist_id: setlistId }),
  });
/** Saves the optional override of the helper's computer (empty: the saved Playback host). */
export const updatePlaybackLibrarySettings = (address: string) =>
  requestJson<LibraryStatus>("/api/v1/playback-library/settings", {
    method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ address }),
  });
/** Opens (or extends) the one-time setup session for the Playback computer; `address` is THIS computer's address there, when StagePilot cannot find it. */
export const startPlaybackHelperInstall = (address?: string) =>
  requestJson<PlaybackHelperInstall>("/api/v1/playback-library/helper-install", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(address ? { address } : {}),
  });
/** Ends the setup session (the command stops working at once). */
export const endPlaybackHelperInstall = () =>
  requestJson<{ ended: boolean }>("/api/v1/playback-library/helper-install", { method: "DELETE" });

// The ProPresenter Link (optional; nothing here is asked until a ProPresenter computer is saved). Writes only through apply.
const linkAsk = (path: string, check: boolean) => `/api/v1/propresenter-link/${path}${check ? "?check=1" : ""}`;
export const getProPresenterLinkStatus = (check = false) => requestJson<LinkStatus>(linkAsk("status", check));
/** Opens (or extends) the one-time setup session for the ProPresenter computer (another computer only; this one needs none). */
export const startProPresenterHelperInstall = (address?: string) =>
  requestJson<PlaybackHelperInstall>("/api/v1/propresenter-link/helper-install", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(address ? { address } : {}),
  });
/** Ends the setup session (the command stops working at once). */
export const endProPresenterHelperInstall = () =>
  requestJson<{ ended: boolean }>("/api/v1/propresenter-link/helper-install", { method: "DELETE" });
export const getProPresenterPlaylists = () =>
  requestJson<{ playlists: ProPresenterPlaylist[] }>("/api/v1/propresenter-link/playlists");
/** What would be done. Writes nothing anywhere. */
export const previewAddSongs = (request: AddSongsPreviewRequest) =>
  requestJson<AddSongsPlan>("/api/v1/propresenter-link/add-songs/preview", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(request),
  });
/** Does what the preview named (`skip`: songs the preview called stuck, left out by name). Answers at once with a job. */
export const applyAddSongs = (planId: string, skip: string[]) =>
  requestJson<AddSongsStarted>("/api/v1/propresenter-link/add-songs/apply", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ plan_id: planId, skip }),
  });
export const getAddSongsJob = (jobId: string) =>
  requestJson<AddSongsJob>(`/api/v1/propresenter-link/add-songs/jobs/${encodeURIComponent(jobId)}`);
