import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { useStagePilot } from "./useStagePilot";

// Counts API calls when one intent is triggered twice in the same tick (double click, Enter repeat)
// and again while the first call is still pending. Exactly one request must go out.
vi.mock("../api", async (original) => {
  const real = await original<typeof import("../api")>();
  const names = Object.keys(real).filter((key) => typeof (real as Record<string, unknown>)[key] === "function");
  return { ...real, ...Object.fromEntries(names.map((name) => [name, vi.fn()])) };
});

const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>> & Record<never, never>;
const m = (name: string) => mocked[name] as ReturnType<typeof vi.fn>;

class QuietSocket {
  onopen = null; onmessage = null; onerror = null; onclose = null;
  close() {}
  send() {}
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
}

const settingsResponse = {
  settings: {
    timezone: "UTC", log_level: "INFO", server_port: 8765,
    integration_modes: { timer_output: "propresenter", service_source: "planning_center" },
    propresenter: { enabled: true, host: "h", port: 1, timer_name: "t", request_timeout_seconds: 3 },
    lights: { enabled: false, output_name: null, channel: 1, pulse_ms: 100, cue_maps: {} },
    planning_center: { app_id: "a", service_type_id: "s" },
    midi: {},
  },
} as never;

// name | how to call it | the api function that must be hit once
const intents: Array<[string, (hook: ReturnType<typeof useStagePilot>) => Promise<unknown> | void, string]> = [
  ["dispatch (Reload service plan, Check System)", (h) => h.dispatch("reload_service_plan" as never), "performAction"],
  ["selectPlan", (h) => h.selectPlan("plan-1"), "selectPlanningCenterPlan"],
  ["saveGeneralSettings (Save configuration, PIN)", (h) => h.saveGeneralSettings({ timezone: "UTC", log_level: "INFO", server_port: 8765 } as never), "updateSettings"],
  ["testPlanningCenterConnection", (h) => h.testPlanningCenterConnection({}), "testPlanningCenter"],
  ["savePlanningCenter", (h) => h.savePlanningCenter({ app_id: "a", service_type_id: "s" } as never, "UTC"), "updatePlanningCenterSettings"],
  ["saveProPresenter (Save configuration)", (h) => h.saveProPresenter({ host: "h", port: 1, timer_name: "t", request_timeout_seconds: 3 } as never), "updateSettings"],
  ["saveLighting (Save configuration)", (h) => h.saveLighting({ enabled: true, output_name: "o", channel: 1, pulse_ms: 100 }), "updateLightsSettings"],
  ["simulateMidi", (h) => h.simulateMidi("next" as never), "simulateMidiCue"],
  ["refreshMidi", (h) => h.refreshMidi(), "refreshMidiInputs"],
];

describe("one API call per intent", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("WebSocket", QuietSocket);
    m("getHealth").mockResolvedValue({ status: "ok" });
    m("getState").mockResolvedValue(null);
    m("getSettings").mockResolvedValue(settingsResponse);
    m("getMidiInputs").mockResolvedValue({ inputs: [], selected: null, status: "disconnected", detail: null } as never);
    m("getMidiMessages").mockResolvedValue({ messages: [] });
    m("getLightsStatus").mockResolvedValue({ enabled: false, outputs: [] } as never);
    m("getPlanningCenterStatus").mockResolvedValue({ connection_method: "manual" } as never);
    m("getPlanningCenterServiceTypes").mockResolvedValue([]);
    m("getProPresenterStatus").mockResolvedValue({ enabled: false } as never);
    m("updateDashboardAccess").mockResolvedValue(settingsResponse);
    m("updatePlanningCenterSettings").mockResolvedValue(settingsResponse);
    m("updateProPresenterSettings").mockResolvedValue({ accepted: true, message: "ok", propresenter: { enabled: true } } as never);
    m("updateLightsSettings").mockResolvedValue({ accepted: true, message: "ok", lights: { enabled: true, outputs: [] } } as never);
  });
  afterEach(() => vi.unstubAllGlobals());

  it.each(intents)("%s: two calls in the same tick and one while pending hit the API once", async (_name, call, apiName) => {
    const gate = deferred<unknown>();
    const target = m(apiName);
    const before = target.mock.calls.length;
    target.mockImplementationOnce(() => gate.promise as never);
    const { result } = renderHook(() => useStagePilot());
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    const start = target.mock.calls.length - before;

    act(() => { void call(result.current); void call(result.current); });
    act(() => { void call(result.current); });
    expect(target.mock.calls.length - before - start).toBe(1);

    await act(async () => { gate.resolve(undefined); await Promise.resolve(); });
    // Released after it settles: the next intent is allowed to go out.
    await waitFor(() => expect(result.current.pendingAction).toBeNull());
  });

  it("a failed save releases the guard so Try again sends a new request", async () => {
    const { result } = renderHook(() => useStagePilot());
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    m("updateLightsSettings").mockRejectedValueOnce(new Error("offline"));
    const calls = () => m("updateLightsSettings").mock.calls.length;
    await act(async () => { await result.current.saveLighting({ enabled: true, output_name: "o", channel: 1, pulse_ms: 100 }); });
    expect(calls()).toBe(1);
    expect(result.current.lightingError).toBeTruthy();
    await act(async () => { await result.current.saveLighting({ enabled: true, output_name: "o", channel: 1, pulse_ms: 100 }); });
    expect(m("updateLightsSettings")).toHaveBeenCalledTimes(2);
  });
});
