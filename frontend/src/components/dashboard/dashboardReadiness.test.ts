import { describe, expect, it } from "vitest";

import { playbackStatus } from "../../test/playbackFixtures";
import type { PlanRunMemory } from "./planProgress";
import type {
  ApplicationState,
  MidiInputsResponse,
  PlanningCenterStatusResponse,
  ProPresenterStatusResponse,
  SettingsResponse,
} from "../../types";
import {
  buildConnectionCardViews,
  buildReadinessChecks,
  readinessErrorCauses,
  readinessPassed,
  readinessWarnings,
} from "./dashboardReadiness";

const state = (overrides: Partial<ApplicationState> = {}): ApplicationState => ({
  revision: 1,
  updated_at: "2026-07-29T12:00:00Z",
  application_status: "running",
  plan: {
    id: "demo",
    title: "Sunday Worship — Demo",
    date: "2026-07-29",
    service_type: "Demo",
    service_type_id: null,
    service_times: [],
    duration_source: "demo",
    songs: [{
      id: "song-1",
      title: "Demo Song",
      duration_seconds: 240,
      order: 1,
      service_sequence: 1,
      is_generic: false,
      source_song_id: null,
    }],
  },
  current_song: null,
  next_song: null,
  current_song_index: null,
  playing_song: null, playing_song_index: null, up_next_song: null,
  planning_center_status: "disconnected",
  midi_status: "disconnected",
  propresenter_status: "disconnected",
  lights_status: "disconnected",
  service_load: {
    status: "loaded",
    target_date: "2026-07-29",
    candidates: [],
    skipped_items: [],
    message: "Demo service loaded.",
    is_stale: false,
    last_attempt_at: "2026-07-29T12:00:00Z",
  },
  timer: {
    status: "stopped",
    duration_seconds: 0,
    started_at: null,
    last_error: null,
  },
  plugins: {
    demo: {
      name: "demo",
      version: "0.1.0",
      status: "running",
      last_error: null,
      last_activity_at: "2026-07-29T12:00:00Z",
    },
  },
  recent_events: [],
  recent_errors: [],
  last_successful_plan_reload_at: null,
  last_action: null,
  ...overrides,
});

const settings = (modes: SettingsResponse["settings"]["integration_modes"]): SettingsResponse => ({
  settings: {
    schema_version: 1,
    onboarding: { general_completed: false },
    integration_modes: modes,
    timezone: "America/Los_Angeles",
    log_level: "INFO",
    server_port: 8765,
    lan_access: false,
    planning_center: {
      app_id: null,
      service_type_id: null,
      plan_title_preference: null,
      preferred_service_time: null,
      upcoming_lookahead_days: 7,
      request_timeout_seconds: 10,
    },
    midi: {
      enabled: false,
      input_name: null,
      channel: 1,
      note: 112,
      mappings: {},
      debounce_ms: 250,
    },
    lights: {
      enabled: false,
      output_name: null,
      channel: 1,
      pulse_ms: 100,
      cue_maps: {},
    },
    propresenter: {
      enabled: false,
      host: "127.0.0.1",
      port: 1025,
      timer_name: "Song Countdown",
      look_id: null,
      request_timeout_seconds: 3,
      reconnect_initial_seconds: 1,
      reconnect_max_seconds: 30,
      health_check_interval_seconds: 10,
    },
  },
  planning_center_secret_saved: false,
  warning: null,
  restart_required: false,
});

const emptyMidi: MidiInputsResponse = {
  enabled: false,
  channel: 1,
  note: 112,
  configured_input_name: null,
  selected_input_name: null,
  inputs: [],
  mappings: {},
};

describe("dashboard integration truthfulness", () => {
  it("keeps every external integration disconnected on a fresh demo installation", () => {
    const freshSettings = settings({
      service_source: "demo",
      midi_source: "simulated",
      timer_output: "simulated",
    });
    const views = buildConnectionCardViews({
      state: state({
        planning_center_status: "connected",
        midi_status: "connected",
        propresenter_status: "connected",
      }),
      settings: freshSettings,
      midi: emptyMidi,
      propresenter: null,
      lights: null,
    });

    expect(views.planningCenter).toMatchObject({
      status: "disconnected",
      mode: "demo",
      detail: "Demo service plan loaded — configure Services",
    });
    expect(views.midi.status).toBe("disconnected");
    expect(views.propresenter.status).toBe("disconnected");
    expect(views.lights.status).toBe("disconnected");

    const checks = buildReadinessChecks({
      state: state(),
      settings: freshSettings,
      propresenter: null,
      live: true,
      views,
    });
    expect(readinessPassed(checks)).toBe(false);
    expect(checks.find((check) => check.id === "service-plan")).toMatchObject({
      label: "Service plan not loaded",
      passed: false,
    });
  });

  it("shows Lighting OSC as ready, not connected, until a cue has gone out", () => {
    const base = settings({ service_source: "demo", midi_source: "simulated", timer_output: "simulated" });
    const oscSettings = { ...base, settings: { ...base.settings, lights: { ...base.settings.lights, enabled: true, method: "osc" as const } } } as SettingsResponse;
    const views = buildConnectionCardViews({
      state: state({ lights_status: "ready" }),
      settings: oscSettings,
      midi: emptyMidi,
      propresenter: null,
      lights: null,
    });
    expect(views.lights.status).toBe("ready");
    expect(views.lights.detail).toBe("Ready, capture not connected yet");
  });

  it("treats a connected OAuth account with a selected service type as configured", () => {
    const productionSettings = settings({
      service_source: "planning_center",
      midi_source: "real",
      timer_output: "propresenter",
    });
    productionSettings.settings.planning_center.service_type_id = "service";
    const planningCenterStatus: PlanningCenterStatusResponse = {
      connection_status: "connected",
      configured: true,
      app_id: null,
      service_type_id: "service",
      planning_center_secret_saved: false,
      connection_method: "oauth",
      oauth_connected: true,
      oauth_needs_reconnect: false,
      detail: null,
    };
    const connectedState = state({ planning_center_status: "connected", plugins: {} });
    const views = buildConnectionCardViews({
      state: connectedState,
      settings: productionSettings,
      planningCenterStatus,
      midi: emptyMidi,
      propresenter: null,
      lights: null,
    });
    const checks = buildReadinessChecks({
      state: connectedState,
      settings: productionSettings,
      propresenter: null,
      live: true,
      views,
    });

    expect(views.planningCenter).toMatchObject({
      status: "connected",
      configured: true,
      mode: "real",
    });
    expect(checks.find((check) => check.id === "planning-center")).toMatchObject({
      label: "Services connected",
      passed: true,
      status: "connected",
    });
  });

  it("does not trust a stale connected MIDI state without an open selected input", () => {
    const realSettings = settings({
      service_source: "planning_center",
      midi_source: "real",
      timer_output: "propresenter",
    });
    realSettings.settings.midi.enabled = true;
    const views = buildConnectionCardViews({
      state: state({ midi_status: "connected", plugins: {} }),
      settings: realSettings,
      midi: { ...emptyMidi, enabled: true },
      propresenter: null,
      lights: null,
    });

    expect(views.midi.status).toBe("disconnected");
    expect(views.midi.detail).toBe("No MIDI inputs found");
  });

  it("keeps ProPresenter API connectivity separate from timer readiness", () => {
    const realSettings = settings({
      service_source: "planning_center",
      midi_source: "real",
      timer_output: "propresenter",
    });
    realSettings.settings.propresenter.enabled = true;
    const propresenter = {
      enabled: true,
      connection_status: "connected",
      detail: "ProPresenter API connected.",
      timer_found: false,
    } as ProPresenterStatusResponse;
    const realState = state({ propresenter_status: "connected", plugins: {} });
    const views = buildConnectionCardViews({
      state: realState,
      settings: realSettings,
      midi: emptyMidi,
      propresenter,
      lights: null,
    });
    const checks = buildReadinessChecks({
      state: realState,
      settings: realSettings,
      propresenter,
      live: true,
      views,
    });

    expect(views.propresenter.status).toBe("connected");
    expect(checks.find((check) => check.id === "propresenter")).toMatchObject({ passed: true });
    expect(checks.find((check) => check.id === "propresenter-timer")).toMatchObject({
      label: "Presentation timer not found",
      passed: false,
      status: "error",
    });
  });

  it("does not list unconfigured Lighting and does not block readiness", () => {
    const productionSettings = settings({
      service_source: "planning_center",
      midi_source: "real",
      timer_output: "propresenter",
    });
    const readyState = state({
      plan: {
        ...state().plan!,
        date: "2026-07-29",
        duration_source: "Planning Center",
      },
      planning_center_status: "connected",
      midi_status: "connected",
      propresenter_status: "connected",
      plugins: {},
    });
    productionSettings.settings.planning_center.app_id = "app";
    productionSettings.settings.planning_center.service_type_id = "service";
    productionSettings.planning_center_secret_saved = true;
    productionSettings.settings.midi.enabled = true;
    productionSettings.settings.midi.input_name = "Playback";
    productionSettings.settings.propresenter.enabled = true;
    const views = buildConnectionCardViews({
      state: readyState,
      settings: productionSettings,
      midi: {
        ...emptyMidi,
        enabled: true,
        selected_input_name: "Playback",
        inputs: [{
          id: "playback",
          name: "Playback",
          ambiguous: false,
          selected: true,
          connected: true,
        }],
      },
      propresenter: {
        enabled: true,
        connection_status: "connected",
        timer_found: true,
      } as ProPresenterStatusResponse,
      lights: null,
    });
    const checks = buildReadinessChecks({
      state: readyState,
      settings: productionSettings,
      propresenter: {
        enabled: true,
        connection_status: "connected",
        timer_found: true,
      } as ProPresenterStatusResponse,
      live: true,
      views,
    });

    expect(checks.find((check) => check.id === "lights")).toBeUndefined();
    expect(readinessPassed(checks)).toBe(true);
  });

  it("never lists Lighting, and a Lighting output going away does not block readiness", () => {
    const configuredSettings = settings({ service_source: "demo", midi_source: "simulated", timer_output: "simulated" });
    configuredSettings.settings.lights.enabled = true;
    configuredSettings.settings.lights.output_name = "StagePilot to Lightkey";
    for (const lights_status of ["disconnected", "error", "connected"] as const) {
      const currentState = state({ lights_status });
      const views = buildConnectionCardViews({ state: currentState, settings: configuredSettings, midi: emptyMidi, propresenter: null, lights: null });
      const checks = buildReadinessChecks({ state: currentState, settings: configuredSettings, propresenter: null, live: true, views });
      expect(checks.some((check) => /lighting|lights/i.test(`${check.id} ${check.label}`))).toBe(false);
    }
  });

  describe("entries name the saved connection method", () => {
    const entry = (id: string, { midiSource, lightsMethod, lightsStatus, midiStatus }: {
      midiSource: SettingsResponse["settings"]["integration_modes"]["midi_source"];
      lightsMethod: "midi" | "osc";
      lightsStatus: "connected" | "disconnected";
      midiStatus: "connected" | "disconnected";
    }) => {
      const config = settings({ service_source: "planning_center", midi_source: midiSource, timer_output: "propresenter" });
      config.settings.lights.enabled = true;
      config.settings.lights.method = lightsMethod;
      config.settings.lights.output_name = lightsMethod === "midi" ? "Lightkey" : null;
      config.settings.midi.enabled = true;
      config.settings.midi.input_name = "Playback";
      const current = state({ midi_status: midiStatus, lights_status: lightsStatus, plugins: {} });
      const views = buildConnectionCardViews({
        state: current, settings: config, propresenter: null,
        midi: { ...emptyMidi, enabled: true, selected_input_name: "Playback", inputs: [{ id: "p", name: "Playback", ambiguous: false, selected: true, connected: midiStatus === "connected" }] },
        lights: { enabled: true, method: lightsMethod, output_name: config.settings.lights.output_name, detail: null } as never,
      });
      return buildReadinessChecks({ state: current, settings: config, propresenter: null, live: true, views }).find((check) => check.id === id)!;
    };

    it.each([
      ["playback_api", "connected", "Playback API connected"],
      ["playback_api", "disconnected", "Playback API disconnected"],
      ["real", "connected", "Playback MIDI connected"],
      ["real", "disconnected", "Playback MIDI disconnected"],
    ] as const)("Playback saved as %s and %s reads %s", (midiSource, midiStatus, label) => {
      expect(entry("midi", { midiSource, midiStatus, lightsMethod: "midi", lightsStatus: "disconnected" }).label).toBe(label);
    });
  });

  describe("Remote Access item", () => {
    const freshSettings = settings({ service_source: "demo", midi_source: "simulated", timer_output: "simulated" });
    const base = () => {
      const views = buildConnectionCardViews({ state: state(), settings: freshSettings, midi: emptyMidi, propresenter: null, lights: null });
      return { state: state(), settings: freshSettings, propresenter: null, live: true, views };
    };

    it("is not listed unless Remote Access has stayed offline", () => {
      expect(buildReadinessChecks(base()).some((check) => check.id === "remote-access")).toBe(false);
      expect(buildReadinessChecks({ ...base(), remoteOffline: false }).some((check) => check.id === "remote-access")).toBe(false);
    });

    it("is listed as an optional problem that neither blocks Ready nor turns the system red", () => {
      const checks = buildReadinessChecks({ ...base(), remoteOffline: true });
      const item = checks.find((check) => check.id === "remote-access");
      expect(item).toMatchObject({ label: "Remote access offline", required: false, passed: false, severity: "warning" });
      expect(readinessErrorCauses(checks)).toEqual([]);
      expect(readinessWarnings(checks)).toEqual([item]);
      const withoutIt = buildReadinessChecks(base());
      expect(readinessPassed(checks)).toBe(readinessPassed(withoutIt));
    });

    it("is not added to the live-status view Viewers get", () => {
      const checks = buildReadinessChecks({ ...base(), stateOnly: true, remoteOffline: true });
      expect(checks.some((check) => check.id === "remote-access")).toBe(false);
    });
  });

  describe("Settings problems (F-H7)", () => {
    const freshSettings = settings({ service_source: "demo", midi_source: "simulated", timer_output: "simulated" });
    const build = (patch: Partial<SettingsResponse>, extra: { stateOnly?: boolean; remoteOffline?: boolean } = {}) => {
      const withPatch = { ...freshSettings, ...patch };
      const views = buildConnectionCardViews({ state: state(), settings: withPatch, midi: emptyMidi, propresenter: null, lights: null });
      return buildReadinessChecks({ state: state(), settings: withPatch, propresenter: null, live: true, views, ...extra });
    };
    const withCaptureFor = (key: string): SettingsResponse["settings"] => ({
      ...freshSettings.settings,
      lights: { ...freshSettings.settings.lights, cue_maps: { [key]: { song_key: key, song_title: key, cues: [], capture: { complete: true, coverage_seconds: 100, recorded_at: "2026-11-10T09:00:00Z" } } } },
    });
    const ids = (checks: ReturnType<typeof build>) => checks.map((check) => check.id);

    it("lists nothing unless there is a problem", () => {
      for (const patch of [{}, { settings_degraded: false, settings_set_aside: [], settings_notice: null }, { settings_notice: "old text only" }] as Partial<SettingsResponse>[]) {
        expect(ids(build(patch)).filter((id) => id.startsWith("settings") || id.startsWith("set-aside"))).toEqual([]);
      }
    });

    it("an unreadable file is a required, blocking error that turns the pill red and leads", () => {
      const checks = build({ settings_degraded: true, settings_degraded_reason: "not valid JSON" });
      expect(checks[0]).toMatchObject({
        id: "settings-degraded", label: "Saved settings could not be read", required: true, severity: "blocking", status: "error", passed: false,
        detail: "It started with the default settings, so Planning Center, Lighting and the other saved settings are not loaded.",
      });
      expect(readinessErrorCauses(checks)[0]).toBe(checks[0]);
      expect(readinessPassed(checks)).toBe(false);
    });

    it("a newer file has its own label and sentence", () => {
      const item = build({ settings_degraded: true, settings_degraded_reason: "schema_version 3 is newer than 2" })[0]!;
      expect(item).toMatchObject({ label: "Settings are from a newer StagePilot", severity: "blocking", required: true, status: "error" });
      expect(item.detail).toContain("will not change the saved file");
    });

    it("a set-aside song is an optional warning, one row per song, in the order the server gave", () => {
      const checks = build({ settings_set_aside: [{ song_key: "a", song_title: "Ever Be" }, { song_key: "b", song_title: "Elohim" }] });
      const rows = checks.filter((check) => check.id.startsWith("set-aside:"));
      expect(rows.map((row) => row.label)).toEqual(["Record Ever Be again", "Record Elohim again"]);
      for (const row of rows) expect(row).toMatchObject({ required: false, severity: "warning", status: "error", passed: false });
      expect(rows[0]!.detail).toBe("The lighting data for Ever Be was damaged and set aside; record it again.");
      expect(readinessWarnings(checks)).toEqual(expect.arrayContaining(rows));
      expect(readinessErrorCauses(checks)).toEqual([]);
      expect(readinessPassed(checks)).toBe(readinessPassed(build({})));
    });

    it("a song with no title falls back to its key, and a repeated key is listed once", () => {
      const checks = build({ settings_set_aside: [{ song_key: "k9", song_title: "  " }, { song_key: "k9", song_title: "x" }] });
      expect(checks.filter((check) => check.id.startsWith("set-aside:")).map((row) => row.label)).toEqual(["Record k9 again"]);
    });

    it("a song drops out as soon as the saved data holds its lighting again", () => {
      const aside = [{ song_key: "a", song_title: "Ever Be" }, { song_key: "b", song_title: "Elohim" }];
      const recorded = { settings: withCaptureFor("a"), settings_set_aside: aside };
      expect(build(recorded).filter((check) => check.id.startsWith("set-aside:")).map((row) => row.label)).toEqual(["Record Elohim again"]);
      const typedCues = { settings: { ...freshSettings.settings, lights: { ...freshSettings.settings.lights, cue_maps: { b: { song_key: "b", song_title: "Elohim", cues: [{ id: "c", at_seconds: 1, label: "x", note: 1, velocity: 1 }] } } } }, settings_set_aside: aside } as unknown as Partial<SettingsResponse>;
      expect(build(typedCues).filter((check) => check.id.startsWith("set-aside:")).map((row) => row.label)).toEqual(["Record Ever Be again"]);
      // An empty map is no data: the song still needs recording.
      const empty = { settings: { ...freshSettings.settings, lights: { ...freshSettings.settings.lights, cue_maps: { a: { song_key: "a", song_title: "Ever Be", cues: [] } } } }, settings_set_aside: aside };
      expect(build(empty).filter((check) => check.id.startsWith("set-aside:"))).toHaveLength(2);
    });

    it("precedence: the settings error leads the error causes; a song warning never turns the system red; the first warning leads", () => {
      const both = build({ settings_degraded: true, settings_set_aside: [{ song_key: "a", song_title: "Ever Be" }] });
      expect(readinessErrorCauses(both).map((check) => check.id)).toEqual(["settings-degraded"]);
      expect(readinessWarnings(both).map((check) => check.id)).toEqual(["set-aside:a"]);
      // With Remote access also down, remote (listed first as before) leads the warnings and every song still has its row.
      const many = build({ settings_set_aside: [{ song_key: "a", song_title: "Ever Be" }, { song_key: "b", song_title: "Elohim" }] }, { remoteOffline: true });
      expect(readinessWarnings(many).map((check) => check.id)).toEqual(["remote-access", "set-aside:a", "set-aside:b"]);
    });

    it("is not added to the live-status view Viewers get", () => {
      const checks = build({ settings_degraded: true, settings_set_aside: [{ song_key: "a", song_title: "Ever Be" }] }, { stateOnly: true });
      expect(ids(checks).some((id) => id.startsWith("settings") || id.startsWith("set-aside"))).toBe(false);
    });
  });

  describe("Scan Playback again (F-S1)", () => {
    const production = settings({ service_source: "planning_center", midi_source: "playback_api", timer_output: "propresenter" });
    const SERVICE = Date.parse("2026-11-15T17:00:00Z"); // 09:00 in Los Angeles
    const planState = (overrides: Partial<ApplicationState> = {}) => {
      const base = state();
      return state({
        plan: { ...base.plan!, date: "2026-11-15", service_times: ["09:00"] },
        service_load: { ...base.service_load, target_date: "2026-11-15", status: "loaded", is_stale: false },
        ...overrides,
      });
    };
    const build = (minutesBefore: number, extra: { playback?: ReturnType<typeof playbackStatus> | null; state?: ApplicationState; stateOnly?: boolean; settings?: SettingsResponse; remoteOffline?: boolean } = {}) => {
      const appState = extra.state ?? planState();
      const config = extra.settings ?? production;
      const playback = extra.playback === undefined ? playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: "2026-11-14T20:00:00Z" }) : extra.playback;
      const views = buildConnectionCardViews({ state: appState, settings: config, midi: emptyMidi, playback, propresenter: null, lights: null, stateOnly: extra.stateOnly });
      return buildReadinessChecks({
        state: appState, settings: config, propresenter: null, live: true, views, playback, now: SERVICE - minutesBefore * 60_000,
        stateOnly: extra.stateOnly, remoteOffline: extra.remoteOffline,
      });
    };
    const reminder = (checks: ReturnType<typeof build>) => checks.find((check) => check.id === "scan-playback");

    it("is listed in the hour before the service, as a warning that never blocks", () => {
      const row = reminder(build(30))!;
      expect(row).toMatchObject({ label: "Scan Playback again", required: false, severity: "warning", status: "error", passed: false });
      expect(row.detail).toBe("The service starts in 30 minutes and Playback has not been scanned in the hour before it. Scan it so song order and lengths are right.");
      expect(readinessWarnings(build(30))[0]).toEqual(row);
      expect(readinessErrorCauses(build(30)).some((check) => check.id === "scan-playback")).toBe(false);
      expect(readinessPassed(build(30))).toBe(readinessPassed(build(90)));
    });

    it("is not listed outside the window, after the start, or while a problem of its own is shown", () => {
      expect(reminder(build(61))).toBeUndefined();
      expect(reminder(build(-5))).toBeUndefined();
      expect(reminder(build(30, { playback: null }))).toBeUndefined();
      expect(reminder(build(30, { playback: playbackStatus({ connected: false, discovery: "done", captured_at: null }) }))).toBeUndefined();
      expect(reminder(build(30, { state: planState({ timer: { status: "running", duration_seconds: 240, started_at: "2026-11-15T16:40:00Z", last_error: null } }) }))).toBeUndefined();
    });

    it("needs a loaded production plan: not the demo plan, not a stale or different-day load", () => {
      const demo = settings({ service_source: "demo", midi_source: "playback_api", timer_output: "simulated" });
      expect(reminder(build(30, { settings: demo }))).toBeUndefined();
      const base = planState();
      expect(reminder(build(30, { state: planState({ service_load: { ...base.service_load, is_stale: true } }) }))).toBeUndefined();
      expect(reminder(build(30, { state: planState({ service_load: { ...base.service_load, target_date: "2026-11-22" } }) }))).toBeUndefined();
      expect(reminder(build(30, { state: planState({ plan: null }) }))).toBeUndefined();
      expect(reminder(build(30, { state: planState({ plan: { ...base.plan!, service_times: [] } }) }))).toBeUndefined();
    });

    it("goes away when a scan completes inside the window, and stays out of the live-status view", () => {
      expect(reminder(build(30))).toBeDefined();
      expect(reminder(build(30, { playback: playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: new Date(SERVICE - 40 * 60_000).toISOString() }) }))).toBeUndefined();
      expect(reminder(build(30, { stateOnly: true }))).toBeUndefined();
    });

    it("stays out of the list while the plan is going and returns when the plan is over (F-S2)", () => {
      const base = planState();
      const plan = { ...base.plan!, service_times: ["09:00", "10:00"], songs: base.plan!.songs.slice(0, 1).map((song) => ({ ...song, duration_seconds: 300 })) };
      const going: PlanRunMemory = { planId: plan.id, firstSongMs: SERVICE + 5 * 60_000, endedMs: null };
      const over: PlanRunMemory = { planId: plan.id, firstSongMs: SERVICE + 5 * 60_000, endedMs: SERVICE + 10 * 60_000 };
      const buildAt = (minutesAfterFirst: number, planRun: PlanRunMemory | null) => {
        const appState = planState({ plan });
        const playback = playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: "2026-11-14T20:00:00Z" });
        const views = buildConnectionCardViews({ state: appState, settings: production, midi: emptyMidi, playback, propresenter: null, lights: null });
        return buildReadinessChecks({ state: appState, settings: production, propresenter: null, live: true, views, playback, now: SERVICE + minutesAfterFirst * 60_000, planRun });
      };
      expect(reminder(buildAt(8, going))).toBeUndefined(); // 09:08, the only song is playing or between songs
      expect(reminder(buildAt(12, over))).toMatchObject({ id: "scan-playback" }); // 09:12, the plan is over, 10:00 is 48 minutes away
      expect(reminder(buildAt(12, over))?.detail).toContain("starts in 48 minutes");
      expect(reminder(buildAt(60, over))).toBeUndefined(); // 10:00: the second service starts
    });

    it("a countdown that has run out is not a song playing (the presentation timer counts on past 0:00)", () => {
      const base = planState();
      const song = { ...base.plan!.songs[0]!, duration_seconds: 300 };
      const plan = { ...base.plan!, service_times: ["09:00", "10:00"], songs: [song] };
      const run = { planId: plan.id, firstSongMs: SERVICE + 5 * 60_000, endedMs: SERVICE + 10 * 60_000 };
      const appState = planState({ plan, playing_song: song, playing_song_index: 0, timer: { status: "running", duration_seconds: 300, started_at: new Date(SERVICE + 5 * 60_000).toISOString(), position_seconds: 0, last_error: null } });
      const playback = playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: "2026-11-14T20:00:00Z" });
      const views = buildConnectionCardViews({ state: appState, settings: production, midi: emptyMidi, playback, propresenter: null, lights: null });
      const at = (minutes: number) => buildReadinessChecks({ state: appState, settings: production, propresenter: null, live: true, views, playback, now: SERVICE + minutes * 60_000, planRun: run }).find((check) => check.id === "scan-playback");
      expect(at(8)).toBeUndefined(); // still counting down
      expect(at(12)).toBeDefined(); // ran out, the timer still says running
    });

    it("comes before the other warnings, so it leads the pill when everything required is fine", () => {
      const ids = readinessWarnings(build(30, { remoteOffline: true })).map((check) => check.id);
      expect(ids).toEqual(["scan-playback", "remote-access"]);
    });
  });

});
