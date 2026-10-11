import { describe, expect, it } from "vitest";

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
  readinessPassed,
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
      label: "ProPresenter timer not found",
      passed: false,
      status: "error",
    });
  });

  it("shows unconfigured Lights as optional without blocking readiness", () => {
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

    expect(checks.find((check) => check.id === "lights")).toMatchObject({
      label: "Lights MIDI output not connected",
      required: false,
      status: "disconnected",
    });
    expect(readinessPassed(checks)).toBe(true);
  });

  it("blocks readiness after a configured Lights output becomes unavailable", () => {
    const configuredSettings = settings({
      service_source: "demo",
      midi_source: "simulated",
      timer_output: "simulated",
    });
    configuredSettings.settings.lights.enabled = true;
    configuredSettings.settings.lights.output_name = "StagePilot to Lightkey";
    const currentState = state({ lights_status: "disconnected" });
    const views = buildConnectionCardViews({
      state: currentState,
      settings: configuredSettings,
      midi: emptyMidi,
      propresenter: null,
      lights: null,
    });
    const checks = buildReadinessChecks({
      state: currentState,
      settings: configuredSettings,
      propresenter: null,
      live: true,
      views,
    });

    expect(checks.find((check) => check.id === "lights")).toMatchObject({
      required: true,
      status: "disconnected",
    });
    expect(readinessPassed(checks)).toBe(false);
  });
});
