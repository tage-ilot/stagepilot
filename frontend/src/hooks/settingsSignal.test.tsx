import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { ApiError } from "../api";
import { LightingSetupPanel } from "../components/LightingSetupPanel";
import type { ApplicationState, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import { useStagePilot } from "./useStagePilot";

// The backend saves a recording by itself (no request from this window). The window must learn it from the
// `settings_revision` the backend publishes in the live state, and reload the saved settings once.
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
  getLightkeyStatus: vi.fn(() => Promise.reject(new Error("offline"))),
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

const stateAt = (revision: number, settingsRevision: number | undefined): ApplicationState => ({
  revision, updated_at: "2026-11-15T18:00:00Z", application_status: "running",
  plan: { id: "p", title: "Sunday", songs: [song, other] }, current_song: song, next_song: other, current_song_index: 0,
  playing_song: null, playing_song_index: null, up_next_song: null,
  planning_center_status: "connected", midi_status: "connected", propresenter_status: "connected", lights_status: "connected",
  service_load: { status: "idle", target_date: null, candidates: [], skipped_items: [], message: null, is_stale: false, last_attempt_at: null },
  timer: { status: "idle", duration_seconds: null, started_at: null, last_error: null },
  plugins: {}, recent_events: [], recent_errors: [], last_successful_plan_reload_at: null, last_action: null,
  ...(settingsRevision === undefined ? {} : { settings_revision: settingsRevision }),
}) as unknown as ApplicationState;

function Host() {
  const sp = useStagePilot();
  if (!sp.state) return <p>loading</p>;
  return (
    <LightingSetupPanel
      error={null} lights={sp.lights ?? lightsStatus} message={null} onRefresh={vi.fn()} onSaveCues={sp.saveLightingCues}
      onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={sp.pendingLightingOperation} settings={sp.settings} state={sp.state}
    />
  );
}

/** The backend pushes a snapshot over the live connection. */
const push = (state: ApplicationState) => act(async () => {
  socket().onmessage?.({ data: JSON.stringify({ type: "state.snapshot", data: state }) });
});
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
  vi.mocked(api.getState).mockResolvedValue(stateAt(1, 0));
  vi.mocked(api.getLightsStatus).mockResolvedValue(lightsStatus);
  vi.mocked(api.getMidiInputs).mockResolvedValue({ enabled: false, inputs: [] } as never);
  vi.mocked(api.getMidiMessages).mockResolvedValue({ messages: [] });
  vi.mocked(api.getProPresenterStatus).mockResolvedValue({ enabled: false } as never);
  vi.mocked(api.getPlanningCenterStatus).mockResolvedValue({ connection_status: "disconnected" } as never);
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

describe("the window learns that the backend saved a recording by itself (F-U1)", () => {
  it("reloads the settings when the backend's settings_revision moves, and the Songs strip says captured", async () => {
    await open();
    expect(heading()).toBe("Songs in this service - 0 of 2 fully captured");
    expect(chip("Elohim")).toHaveAccessibleName(/not captured/);

    // 18:07 UTC: the recording of Elohim finishes on the backend; no request came from this window.
    getSettings.mockResolvedValue(settingsAt(1, { "song-1": completeMap("song-1", "Elohim", [captured("a", 1), captured("b", 2)]) }, { "song-1": 1 }));
    await push(stateAt(2, 1));
    await settle(2_100);

    expect(getSettings).toHaveBeenCalledTimes(1);
    expect(heading()).toBe("Songs in this service - 1 of 2 fully captured");
    expect(chip("Elohim")).not.toHaveAccessibleName(/not captured/);
    expect(chip("Ever Be")).toHaveAccessibleName(/not captured/);
  });
});
