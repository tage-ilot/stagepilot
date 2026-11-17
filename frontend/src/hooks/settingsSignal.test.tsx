import { act, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { ApiError } from "../api";
import { LightingSetupPanel } from "../components/LightingSetupPanel";
import type { ApplicationState, LightkeyStatus, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import { useStagePilot } from "./useStagePilot";

// The backend saves a recording by itself (no request from this window). The window must learn it from the
// `settings_revision` on the Lightkey status the strip polls, and reload the saved settings once.
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getAccess: vi.fn(),
  getHealth: vi.fn(),
  getState: vi.fn(),
  getSettings: vi.fn(),
  getLightsStatus: vi.fn(),
  getMidiInputs: vi.fn(),
  getMidiMessages: vi.fn(),
  getProPresenterStatus: vi.fn(),
  getPlanningCenterStatus: vi.fn(),
  getLightkeyStatus: vi.fn(),
  updateLightingCueMap: vi.fn(),
  websocketUrl: "ws://127.0.0.1:8765/ws",
}));

class MockWebSocket {
  static instances: MockWebSocket[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((message: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: ((event?: { code: number }) => void | Promise<void>) | null = null;
  constructor(readonly url: string) { MockWebSocket.instances.push(this); }
  close = vi.fn();
}
const socket = () => MockWebSocket.instances[MockWebSocket.instances.length - 1]!;

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Elohim", duration_seconds: 300, order: 1, is_generic: false };
const other: Song = { id: "item-2", source_song_id: "song-2", title: "Ever Be", duration_seconds: 240, order: 2, is_generic: false };
const base = { osc_host: "127.0.0.1", osc_port: 21600, enabled: true, output_name: null, channel: 1, pulse_ms: 100 };
const lightsStatus = { ...base, method: "osc", connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null } as LightsStatusResponse;

const captured = (id: string, at: number): OscLightingCue => ({
  id, at_seconds: at, address: "", args: [], label: "", source: "captured",
  messages: [{ address: "/fixture/MH1/overrides/dimmer", args: [{ type: "float", value: 1 }] }],
});
type CueMaps = Record<string, unknown>;
const completeMap = (key: string, title: string, rows: OscLightingCue[]) => ({
  song_key: key, song_title: title, cues: [], osc_cues: rows, capture: { complete: true },
});
/** The saved settings as the backend sends them: `settings_revision` counts the saves behind this copy. */
const settingsAt = (revision: number, cueMaps: CueMaps = {}, cueRevisions: Record<string, number> = {}) => ({
  planning_center_secret_saved: false, warning: null, restart_required: false, settings_revision: revision,
  settings: { lights: { ...base, method: "osc", pads: [], cue_maps: cueMaps, cue_revisions: cueRevisions } },
}) as unknown as SettingsResponse;

const stateAt = (revision: number): ApplicationState => ({
  revision, updated_at: "2026-11-15T18:00:00Z", application_status: "running",
  plan: { id: "p", title: "Sunday", songs: [song, other] }, current_song: song, next_song: other, current_song_index: 0,
  playing_song: null, playing_song_index: null, up_next_song: null,
  planning_center_status: "connected", midi_status: "connected", propresenter_status: "connected", lights_status: "connected",
  service_load: { status: "idle", target_date: null, candidates: [], skipped_items: [], message: null, is_stale: false, last_attempt_at: null },
  timer: { status: "idle", duration_seconds: null, started_at: null, last_error: null },
  plugins: {}, recent_events: [], recent_errors: [], last_successful_plan_reload_at: null, last_action: null,
}) as unknown as ApplicationState;

function Host() {
  const sp = useStagePilot();
  if (!sp.state) return <p>loading</p>;
  return (
    <LightingSetupPanel
      error={null} lights={sp.lights ?? lightsStatus} message={null} onRefresh={vi.fn()} onSaveCues={sp.saveLightingCues} onSettingsRevision={sp.noteSettingsRevision}
      onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={sp.pendingLightingOperation} settings={sp.settings} state={sp.state}
    />
  );
}

const lightkeyAt = (settingsRevision: number) => ({
  api_url: "http://10.30.0.211:8770", link: "connected", detail: null, active_count: 0, recording: false, recording_song: null,
  replay: true, replay_blocked: null, replaying_song: null, message: null, settings_revision: settingsRevision,
}) as LightkeyStatus;
/** The backend has made `settingsRevision` saves; the Lightkey strip's next poll of its status (every 1.5 s) says so. */
const backendSaves = async (settingsRevision: number, pollSeconds = 1.6) => {
  vi.mocked(api.getLightkeyStatus).mockResolvedValue(lightkeyAt(settingsRevision));
  await settle(pollSeconds * 1_000);
};
/** The same, for a window without the strip on screen: the hook is told directly. */
const told = async (hook: { result: { current: ReturnType<typeof useStagePilot> } }, settingsRevision: number) => {
  await act(async () => { hook.result.current.noteSettingsRevision(settingsRevision); });
};
const settle = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
const heading = () => screen.getByRole("heading", { name: /Songs in this service/ }).textContent;
const chip = (title: string) => within(screen.getByTestId("songs-strip")).getByRole("button", { name: new RegExp(`^${title}`) });

const getSettings = vi.mocked(api.getSettings);

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.clearAllMocks();
  MockWebSocket.instances = [];
  vi.stubGlobal("WebSocket", MockWebSocket);
  vi.spyOn(window, "confirm").mockReturnValue(true);
  vi.mocked(api.getHealth).mockResolvedValue({ status: "healthy", version: "0", application_status: "running", plugins: [] });
  vi.mocked(api.getState).mockResolvedValue(stateAt(1));
  vi.mocked(api.getLightsStatus).mockResolvedValue(lightsStatus);
  vi.mocked(api.getMidiInputs).mockResolvedValue({ enabled: false, inputs: [] } as never);
  vi.mocked(api.getMidiMessages).mockResolvedValue({ messages: [] });
  vi.mocked(api.getProPresenterStatus).mockResolvedValue({ enabled: false } as never);
  vi.mocked(api.getPlanningCenterStatus).mockResolvedValue({ connection_status: "disconnected" } as never);
  vi.mocked(api.getLightkeyStatus).mockResolvedValue(lightkeyAt(0));
  getSettings.mockResolvedValue(settingsAt(0));
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

const open = async () => {
  render(<Host />);
  await waitFor(() => expect(screen.queryByText("loading")).toBeNull());
  await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(1));
  await act(async () => { socket().onopen?.(); });
  await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(2)); // a (re)opened live connection reloads once
  getSettings.mockClear();
};
const openHook = async () => {
  const hook = renderHook(() => useStagePilot());
  await waitFor(() => expect(hook.result.current.settings).not.toBeNull());
  await act(async () => { socket().onopen?.(); });
  await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(2));
  getSettings.mockClear();
  return hook;
};

describe("the window learns that the backend saved a recording by itself (F-U1)", () => {
  it("reloads the settings when the backend's settings_revision moves, and the Songs strip says captured", async () => {
    await open();
    expect(heading()).toBe("Songs in this service - 0 of 2 fully captured");
    expect(chip("Elohim")).toHaveAccessibleName(/not captured/);

    // 18:07 UTC: the recording of Elohim finishes on the backend; no request came from this window.
    getSettings.mockResolvedValue(settingsAt(1, { "song-1": completeMap("song-1", "Elohim", [captured("a", 1), captured("b", 2)]) }, { "song-1": 1 }));
    await backendSaves(1);
    await settle(2_100);

    expect(getSettings).toHaveBeenCalledTimes(1);
    expect(heading()).toBe("Songs in this service - 1 of 2 fully captured");
    expect(chip("Elohim")).not.toHaveAccessibleName(/not captured/);
    expect(chip("Ever Be")).toHaveAccessibleName(/not captured/);
  });

  it("asks for nothing while the number is unchanged, however long the window stays open", async () => {
    await open();
    await backendSaves(0, 60);
    expect(api.getLightsStatus).toHaveBeenCalled();
    expect(getSettings).not.toHaveBeenCalled();
  });

  it("one reload for a burst of saves, and none once it has caught up", async () => {
    await open();
    // Four saves (a loudness take and a recording each) between two polls of the status: one number, one reload.
    getSettings.mockResolvedValue(settingsAt(4));
    for (const n of [1, 2, 3]) vi.mocked(api.getLightkeyStatus).mockResolvedValue(lightkeyAt(n));
    await backendSaves(4);
    await settle(2_100);
    expect(getSettings).toHaveBeenCalledTimes(1);
    // Caught up: the polls go on, the number is the same, nothing is asked.
    await settle(60_000);
    expect(getSettings).toHaveBeenCalledTimes(1);
    // Another save later: exactly one more.
    getSettings.mockResolvedValue(settingsAt(5));
    await backendSaves(5);
    await settle(2_100);
    expect(getSettings).toHaveBeenCalledTimes(2);
  });

  it("never starts a second reload while one is on its way", async () => {
    await open();
    let finish!: (value: SettingsResponse) => void;
    getSettings.mockImplementationOnce(() => new Promise<SettingsResponse>((resolve) => { finish = resolve; }));
    await backendSaves(1);
    await settle(2_100);
    expect(getSettings).toHaveBeenCalledTimes(1);
    // Another save, and more time than the delay, while the (large) first reload is still loading.
    await backendSaves(2);
    await settle(10_000);
    expect(getSettings).toHaveBeenCalledTimes(1);
    getSettings.mockResolvedValue(settingsAt(2));
    await act(async () => { finish(settingsAt(1)); });
    await settle(2_100);
    expect(getSettings).toHaveBeenCalledTimes(2); // behind again: exactly one more
    await settle(10_000);
    expect(getSettings).toHaveBeenCalledTimes(2);
  });

  it("a save of this window, which already reloads, is not followed by a second reload from the signal", async () => {
    const hook = await openHook();
    const { result } = hook;
    vi.mocked(api.updateLightingCueMap).mockResolvedValue({ accepted: true, message: "Saved.", lights: lightsStatus } as never);
    const save = deferredSettings();
    getSettings.mockImplementation(() => save.promise);

    let saving!: Promise<void>;
    act(() => { saving = result.current.saveLightingCues(song, [captured("a", 1)], "osc"); });
    await settle(50);
    // The backend counts the save and the next poll says so, before the (28 MB) reload of the save has finished.
    await told(hook, 1);
    await settle(2_500);
    await act(async () => { save.resolve(settingsAt(1)); await saving; });
    await settle(10_000);
    expect(getSettings).toHaveBeenCalledTimes(1);
  });

  it("keeps a row being typed when a recording of the same song finishes behind it", async () => {
    await open();
    const user = (await import("@testing-library/user-event")).default.setup({ advanceTimers: vi.advanceTimersByTime });
    await user.click(screen.getByRole("button", { name: "Add cue" }));
    const rows = () => screen.queryAllByLabelText("OSC address").length - 1; // minus the test-send field
    expect(rows()).toBe(1);
    await user.type(screen.getAllByLabelText("OSC address")[0]!, "/live/cue/");

    getSettings.mockResolvedValue(settingsAt(1, { "song-1": completeMap("song-1", "Elohim", [captured("a", 1), captured("b", 2)]) }, { "song-1": 1 }));
    await backendSaves(1);
    await settle(2_100);

    expect(getSettings).toHaveBeenCalledTimes(1);
    expect(screen.getAllByTestId("look-row")).toHaveLength(2); // the recording is on the list now
    expect(rows()).toBe(1); // and the row being typed is still there, with what was typed
    expect(screen.getAllByLabelText("OSC address")[0]).toHaveValue("/live/cue/");
  });

  it("leaves an unchanged song's saved rows as the very same objects when another song's recording finishes", async () => {
    const mapOne = completeMap("song-1", "Elohim", [captured("a", 1)]);
    getSettings.mockResolvedValue(settingsAt(0, { "song-1": mapOne }, { "song-1": 1 }));
    const hook = await openHook();
    const { result } = hook;
    const shown = result.current.settings!.settings.lights.cue_maps["song-1"];
    getSettings.mockResolvedValue(settingsAt(1, { "song-1": JSON.parse(JSON.stringify(mapOne)), "song-2": completeMap("song-2", "Ever Be", [captured("z", 4)]) }, { "song-1": 1, "song-2": 1 }));
    await told(hook, 1);
    await settle(2_100);
    await waitFor(() => expect(result.current.settings?.settings_revision).toBe(1));
    expect(Object.keys(result.current.settings!.settings.lights.cue_maps)).toEqual(["song-1", "song-2"]);
    // Nothing downstream (the rows on screen, a row being typed) sees a new value for the song that did not change.
    expect(result.current.settings!.settings.lights.cue_maps["song-1"]).toBe(shown);
  });

  it("a save from a screen that has not reloaded yet still carries its old revision, so the backend refuses it (409)", async () => {
    getSettings.mockResolvedValue(settingsAt(0, { "song-1": completeMap("song-1", "Elohim", [captured("a", 1)]) }, { "song-1": 3 }));
    const hook = await openHook();
    const { result } = hook;
    expect(result.current.settings?.settings.lights.cue_revisions?.["song-1"]).toBe(3);
    const put = vi.mocked(api.updateLightingCueMap);
    put.mockRejectedValueOnce(new ApiError("This song's lights changed since you opened them", 409));

    // The recording finished (revision 4 on the backend); this window has been told but has not reloaded yet.
    getSettings.mockResolvedValue(settingsAt(1, { "song-1": completeMap("song-1", "Elohim", [captured("n", 9)]) }, { "song-1": 4 }));
    await told(hook, 1);
    await act(async () => { await result.current.saveLightingCues(song, [captured("a", 1)], "osc"); });
    expect(put.mock.calls[0]![0].revision).toBe(3); // the revision the rows were edited from, not a fresh one
    expect(result.current.lightingError).toMatch(/changed since you opened/);
    // The refusal reloads what is saved now, and the next save is based on it.
    await waitFor(() => expect(result.current.settings?.settings.lights.cue_revisions?.["song-1"]).toBe(4));
    put.mockResolvedValue({ accepted: true, message: "Saved.", lights: lightsStatus } as never);
    await act(async () => { await result.current.saveLightingCues(song, [captured("n", 9)], "osc"); });
    expect(put.mock.calls[1]![0].revision).toBe(4);
  });

  it("Clear all songs sends each song's revision, so a recording the screen has not shown is refused, not erased", async () => {
    getSettings.mockResolvedValue(settingsAt(0, { "song-1": completeMap("song-1", "Elohim", [captured("a", 1)]) }, { "song-1": 3 }));
    const hook = await openHook();
    const { result } = hook;
    const put = vi.mocked(api.updateLightingCueMap);
    put.mockRejectedValueOnce(new ApiError("This song's lights changed since you opened them", 409));
    getSettings.mockResolvedValue(settingsAt(1, {}, { "song-1": 4 }));
    await act(async () => { await result.current.clearAllLightingCues([song, other], "osc"); });
    expect(put.mock.calls[0]![0]).toMatchObject({ song_key: "song-1", revision: 3 });
    expect(put).toHaveBeenCalledTimes(1); // stopped at the refusal
    expect(result.current.lightingError).toMatch(/changed since you opened|failed to clear/i);
    await waitFor(() => expect(result.current.settings?.settings.lights.cue_revisions?.["song-1"]).toBe(4));
  });
});

function deferredSettings() {
  let resolve!: (value: SettingsResponse) => void;
  const promise = new Promise<SettingsResponse>((r) => { resolve = r; });
  return { promise, resolve };
}
