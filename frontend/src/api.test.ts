import { afterEach, describe, expect, it, vi } from "vitest";

import {
  apiOrigin,
  getLightsStatus,
  getMidiInputs,
  getMidiMessages,
  getPlanningCenterServiceTypes,
  rememberServerPort,
  refreshMidiInputs,
  selectMidiInput,
  selectPlanningCenterPlan,
  simulateMidiCue,
  testLightingCue,
  testPlanningCenter,
  updatePlanningCenterSettings,
  updateLightingCueMap,
  updateLightsSettings,
} from "./api";
import type {
  MidiCueSimulationResponse,
  MidiInputSelectionResponse,
  MidiInputsResponse,
  MidiMonitorResponse,
  PlanSelectionResponse,
} from "./types";

afterEach(() => {
  vi.unstubAllGlobals();
  window.localStorage.clear();
});

describe("dashboard server port", () => {
  it("remembers a validated saved port for the next dashboard launch", () => {
    rememberServerPort(9001);

    expect(window.localStorage.getItem("stagepilot.server-port")).toBe("9001");
  });
});

describe("Planning Center onboarding API", () => {
  it("tests temporary credentials and loads saved service types", async () => {
    const tested = {
      authenticated: true as const,
      message: "Planning Center authentication succeeded.",
      service_types: [{ id: "sunday", name: "Sunday Morning" }],
    };
    const fetchMock = vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue(tested),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue(tested.service_types),
      });
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      testPlanningCenter({ app_id: "app-id", secret: "private-secret" }),
    ).resolves.toEqual(tested);
    await expect(getPlanningCenterServiceTypes()).resolves.toEqual(tested.service_types);

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      `${apiOrigin}/api/v1/planning-center/test`,
      {
        method: "POST",
        credentials: "include",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify({ app_id: "app-id", secret: "private-secret" }),
      },
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      `${apiOrigin}/api/v1/planning-center/service-types`,
      { credentials: "include", headers: { Accept: "application/json" } },
    );
  });

  it("sends the credential only to the protected settings endpoint", async () => {
    const response = { planning_center_secret_saved: true };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: vi.fn().mockResolvedValue(response),
    });
    vi.stubGlobal("fetch", fetchMock);
    const input = {
      app_id: "app-id",
      service_type_id: "sunday",
      plan_title_preference: null,
      preferred_service_time: "09:00",
      upcoming_lookahead_days: 30,
      request_timeout_seconds: 10,
      secret: "private-secret",
    };

    await updatePlanningCenterSettings(input);

    expect(fetchMock).toHaveBeenCalledWith(
      `${apiOrigin}/api/v1/planning-center/settings`,
      {
        method: "POST",
        credentials: "include",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify(input),
      },
    );
  });
});

describe("selectPlanningCenterPlan", () => {
  it("posts the selected Planning Center plan ID as JSON", async () => {
    const response: PlanSelectionResponse = {
      accepted: true,
      message: "Plan loaded.",
      state: {
        revision: 1,
        updated_at: "2026-07-13T16:00:00Z",
        application_status: "running",
        plan: null,
        current_song: null,
        next_song: null,
        current_song_index: null,
        playing_song: null, playing_song_index: null, up_next_song: null,
        planning_center_status: "connected",
        midi_status: "connected",
        propresenter_status: "connected",
        lights_status: "disconnected",
        service_load: {
          status: "loaded",
          target_date: "2026-07-13",
          candidates: [],
          skipped_items: [],
          message: null,
          is_stale: false,
          last_attempt_at: "2026-07-13T16:00:00Z",
        },
        timer: {
          status: "stopped",
          duration_seconds: null,
          started_at: null,
          last_error: null,
        },
        plugins: {},
        recent_events: [],
        recent_errors: [],
        last_successful_plan_reload_at: "2026-07-13T16:00:00Z",
        last_action: null,
      },
    };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: vi.fn().mockResolvedValue(response),
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(selectPlanningCenterPlan("plan-evening")).resolves.toEqual(response);

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock).toHaveBeenCalledWith(
      `${apiOrigin}/api/v1/planning-center/plans/select`,
      {
        method: "POST",
        credentials: "include",
        headers: {
          Accept: "application/json",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ plan_id: "plan-evening" }),
      },
    );
  });
});

const midi: MidiInputsResponse = {
  enabled: true,
  channel: 1,
  note: 112,
  configured_input_name: null,
  selected_input_name: null,
  inputs: [],
  mappings: {
    start_next: 100,
    restart_current: 101,
    previous: 102,
    next: 103,
    reload_plan: 104,
    stop_timer: 105,
  },
};

describe("MIDI API", () => {
  it("gets and refreshes the available inputs", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: vi.fn().mockResolvedValue(midi),
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(getMidiInputs()).resolves.toEqual(midi);
    await expect(refreshMidiInputs()).resolves.toEqual(midi);

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      `${apiOrigin}/api/v1/midi/inputs`,
      { credentials: "include", headers: { Accept: "application/json" } },
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      `${apiOrigin}/api/v1/midi/inputs/refresh`,
      {
        method: "POST",
        credentials: "include",
        headers: { Accept: "application/json" },
      },
    );
  });

  it("gets recent MIDI monitor messages", async () => {
    const monitor: MidiMonitorResponse = { messages: [] };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: vi.fn().mockResolvedValue(monitor),
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(getMidiMessages()).resolves.toEqual(monitor);
    expect(fetchMock).toHaveBeenCalledWith(
      `${apiOrigin}/api/v1/midi/messages`,
      { credentials: "include", headers: { Accept: "application/json" } },
    );
  });

  it("posts input selections and cue simulations as JSON", async () => {
    const selection: MidiInputSelectionResponse = {
      accepted: true,
      message: "Input selected.",
      midi,
    };
    const simulation = {
      cue: "start_next",
      action: "start_next",
      accepted: true,
      message: "Cue accepted.",
      state: {
        revision: 2,
      },
    } as MidiCueSimulationResponse;
    const fetchMock = vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue(selection),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue(simulation),
      });
    vi.stubGlobal("fetch", fetchMock);
    const inputId = "a".repeat(64);

    await expect(selectMidiInput(inputId)).resolves.toEqual(selection);
    await expect(simulateMidiCue("start_next")).resolves.toEqual(simulation);

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      `${apiOrigin}/api/v1/midi/input-selection`,
      {
        method: "POST",
        credentials: "include",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify({ input_id: inputId }),
      },
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      `${apiOrigin}/api/v1/midi/cue-simulation`,
      {
        method: "POST",
        credentials: "include",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify({ cue: "start_next" }),
      },
    );
  });
});

describe("Lighting API", () => {
  it("configures output and sends cue maps and test pulses as JSON", async () => {
    const lights = {
      enabled: true,
      output_name: "StagePilot to Lightkey",
      channel: 3,
      pulse_ms: 100,
      connection_status: "connected" as const,
      detail: "Connected.",
      outputs: [],
      last_cue: null,
      last_cue_at: null,
    };
    const operation = { accepted: true, message: "Saved.", lights };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: vi.fn().mockResolvedValue(operation),
    });
    vi.stubGlobal("fetch", fetchMock);
    const outputSettings = {
      enabled: true,
      output_name: "StagePilot to Lightkey",
      channel: 3,
      pulse_ms: 100,
    };
    const cueMap = {
      song_key: "song-1",
      song_title: "Holy Forever",
      cues: [{
        id: "c17d19ab-1447-4e73-898e-468b2dfa87c7",
        at_seconds: 65,
        note: 72,
        velocity: 110,
        label: "First chorus",
      }],
    };

    await updateLightsSettings(outputSettings);
    await updateLightingCueMap(cueMap);
    await testLightingCue(72, 110);

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      `${apiOrigin}/api/v1/lights/settings`,
      {
        method: "POST",
        credentials: "include",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify(outputSettings),
      },
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      `${apiOrigin}/api/v1/lights/cue-map`,
      {
        method: "PUT",
        credentials: "include",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify(cueMap),
      },
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      `${apiOrigin}/api/v1/lights/test`,
      {
        method: "POST",
        credentials: "include",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify({ note: 72, velocity: 110 }),
      },
    );
  });

  it("loads the current lighting connection", async () => {
    const lights = {
      enabled: false,
      output_name: null,
      channel: 1,
      pulse_ms: 100,
      connection_status: "disconnected" as const,
      detail: null,
      outputs: [],
      last_cue: null,
      last_cue_at: null,
    };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: vi.fn().mockResolvedValue(lights),
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(getLightsStatus()).resolves.toEqual(lights);
    expect(fetchMock).toHaveBeenCalledWith(
      `${apiOrigin}/api/v1/lights`,
      { credentials: "include", headers: { Accept: "application/json" } },
    );
  });
});

// Remote sessions stay in HttpOnly cookies; only capabilities/CSRF live in memory.
import { getAccess, getState, loginRemote, logoutRemote, performAction, resolveApiOrigin, ApiError } from "./api";
import { NO_CAPABILITIES, LOCAL_CAPABILITIES, onAccessInvalidated, setApiAccess } from "./access/accessState";
import type { DashboardAccess } from "./types";

const operatorAccess: DashboardAccess = {
  mode: "remote", authentication: "password", authenticated: true,
  capabilities: { ...LOCAL_CAPABILITIES, canActivateServices: false },
  user: { email: "operator@example.com", role: "Operator" }, expires_at: null, csrf_token: "csrf-test-only",
};
afterEach(() => setApiAccess(null));

describe("remote access API", () => {
  it("uses the page origin for HTTPS and custom-port LAN, while preserving desktop/development ports", () => {
    const remote = { protocol: "https:", origin: "https://remote.example.com", hostname: "remote.example.com", port: "" };
    expect(resolveApiOrigin(remote, false, false, undefined, 8765)).toBe(remote.origin);
    const lan = { protocol: "http:", origin: "http://stagepilot.local:9001", hostname: "stagepilot.local", port: "9001" };
    expect(resolveApiOrigin(lan, false, false, undefined, 8765)).toBe(lan.origin);
    expect(resolveApiOrigin(remote, true, false, undefined, 9001)).toBe("http://127.0.0.1:9001");
    expect(resolveApiOrigin({ ...lan, origin: "http://localhost:5173", hostname: "localhost", port: "5173" }, false, true, undefined, 8765)).toBe("http://127.0.0.1:8765");
    expect(resolveApiOrigin(remote, false, false, "https://configured.example.com/", 8765)).toBe("https://configured.example.com");
  });

  it("gets backend capabilities without cache and logs in with the remote login header", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 200, json: async () => operatorAccess });
    vi.stubGlobal("fetch", fetchMock);
    await getAccess();
    expect(fetchMock).toHaveBeenLastCalledWith(`${apiOrigin}/api/v1/access`, expect.objectContaining({ cache: "no-store", credentials: "include" }));
    await loginRemote("operator@example.com", "test-password");
    expect(fetchMock).toHaveBeenLastCalledWith(`${apiOrigin}/api/v1/remote-auth/login`, expect.objectContaining({
      credentials: "include", method: "POST",
      headers: expect.objectContaining({ "X-StagePilot-Remote": "1" }),
      body: JSON.stringify({ email: "operator@example.com", password: "test-password" }),
    }));
  });

  it("attaches CSRF to Operator mutations and handles the 204 sign-out response", async () => {
    setApiAccess(operatorAccess);
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 204 });
    vi.stubGlobal("fetch", fetchMock);
    await performAction("start_next");
    expect(fetchMock).toHaveBeenLastCalledWith(`${apiOrigin}/api/v1/actions/start_next`, expect.objectContaining({
      credentials: "include", headers: { Accept: "application/json", "X-CSRF-Token": "csrf-test-only",
        "Idempotency-Key": expect.stringMatching(/^[a-f0-9-]{36}$/) },
    }));
    await expect(logoutRemote()).resolves.toBeUndefined();
  });

  it("blocks Viewer configuration requests and commands before fetch but permits live state", async () => {
    setApiAccess({ ...operatorAccess, capabilities: { ...NO_CAPABILITIES, canRead: true } });
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 200, json: async () => ({ revision: 9 }) });
    vi.stubGlobal("fetch", fetchMock);
    await expect(getMidiInputs()).rejects.toBeInstanceOf(ApiError);
    await expect(performAction("start_next")).rejects.toBeInstanceOf(ApiError);
    expect(fetchMock).not.toHaveBeenCalled();
    await expect(getState()).resolves.toEqual({ revision: 9 });
  });

  it.each([401, 403])("invalidates access on API rejection (%s), without treating login failures as session expiry", async (status) => {
    setApiAccess(operatorAccess);
    const invalidated = vi.fn();
    const unsubscribe = onAccessInvalidated(invalidated);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status, json: async () => ({ detail: "Access rejected" }) }));
    try {
      await expect(loginRemote("operator@example.com", "wrong-password")).rejects.toMatchObject({ status });
      expect(invalidated).not.toHaveBeenCalled();
      await expect(getState()).rejects.toMatchObject({ status });
      expect(invalidated).toHaveBeenCalledOnce();
    } finally { unsubscribe(); }
  });
});
