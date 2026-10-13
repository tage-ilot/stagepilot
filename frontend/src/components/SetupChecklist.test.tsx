import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  ApplicationState,
  MidiInputsResponse,
  ProPresenterStatusResponse,
  SettingsResponse,
} from "../types";
import { SetupChecklist } from "./SetupChecklist";

const settings: SettingsResponse = {
  settings: {
    schema_version: 1,
    onboarding: { general_completed: true },
    integration_modes: {
      service_source: "planning_center",
      midi_source: "real",
      timer_output: "propresenter",
    },
    timezone: "America/Los_Angeles",
    log_level: "INFO",
    server_port: 8765,
    planning_center: {
      app_id: "app-id",
      service_type_id: "service-type",
      plan_title_preference: null,
      preferred_service_time: null,
      upcoming_lookahead_days: 30,
      request_timeout_seconds: 10,
    },
    midi: {
      enabled: true,
      input_name: "Playback Controller",
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
      enabled: true,
      host: "127.0.0.1",
      port: 1025,
      timer_name: "Song Countdown",
      request_timeout_seconds: 3,
      reconnect_initial_seconds: 1,
      reconnect_max_seconds: 30,
      health_check_interval_seconds: 10,
    },
  },
  planning_center_secret_saved: true,
  warning: null,
  restart_required: false,
};

const midi: MidiInputsResponse = {
  enabled: true,
  channel: 1,
  note: 112,
  configured_input_name: "Playback Controller",
  selected_input_name: "Playback Controller",
  inputs: [{
    id: "input-id",
    name: "Playback Controller",
    ambiguous: false,
    selected: true,
    connected: true,
  }],
  mappings: {},
};

const propresenter: ProPresenterStatusResponse = {
  enabled: true,
  host: "127.0.0.1",
  port: 1025,
  timer_name: "Song Countdown",
  look_id: null,
  request_timeout_seconds: 3,
  connection_status: "connected",
  detail: null,
  timers: [],
  selected_timer_id: "timer-id",
  timer_found: true,
  looks: [],
  current_look_id: null,
  look_found: true,
  last_checked_at: "2026-07-15T12:00:00Z",
};

const state: ApplicationState = {
  revision: 1,
  updated_at: "2026-07-15T12:00:00Z",
  application_status: "running",
  plan: {
    id: "plan-id",
    title: "Sunday Service",
    date: "2026-07-19",
    service_type: "Weekend Services",
    service_type_id: "service-type",
    service_times: ["09:00"],
    duration_source: "Planning Center scheduled item length",
    songs: [{
      id: "song-id",
      title: "Holy Forever",
      duration_seconds: 300,
      order: 1,
      is_generic: false,
      source_song_id: "source-song-id",
    }],
  },
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
    target_date: "2026-07-19",
    candidates: [],
    skipped_items: [],
    message: null,
    is_stale: false,
    last_attempt_at: "2026-07-15T12:00:00Z",
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
  last_successful_plan_reload_at: "2026-07-15T12:00:00Z",
  last_action: null,
};

const renderChecklist = (savedSettings: SettingsResponse | null = settings) => render(
  <SetupChecklist
    live
    midi={midi}
    onOpen={vi.fn()}
    propresenter={propresenter}
    settings={savedSettings}
    state={state}
  />,
);

afterEach(() => {
  vi.useRealTimers();
});

describe("SetupChecklist", () => {
  it("announces completed setup and closes automatically", () => {
    vi.useFakeTimers();
    renderChecklist();

    expect(screen.getByRole("heading", { name: "StagePilot configuration is complete" })).toBeInTheDocument();
    expect(screen.getByText(/Everything is connected and ready/)).toBeInTheDocument();
    expect(screen.getByText("6 of 6 complete")).toBeInTheDocument();

    act(() => vi.advanceTimersByTime(2500));

    expect(screen.queryByLabelText("StagePilot configuration progress")).not.toBeInTheDocument();
  });

  it("can be dismissed with its close button", async () => {
    const user = userEvent.setup();
    renderChecklist(null);

    await user.click(screen.getByRole("button", { name: "Close first-launch configuration" }));

    expect(screen.queryByLabelText("StagePilot configuration progress")).not.toBeInTheDocument();
  });

  it("uses the same dark translucent treatment as configuration panels", () => {
    renderChecklist(null);

    expect(screen.getByLabelText("StagePilot configuration progress")).toHaveClass(
      "bg-surface-950/70",
      "shadow-elevation-default",
    );
    expect(screen.getByText("Select a configuration step to open its connection panel.")).toHaveClass(
      "text-ink-body",
    );
    expect(screen.getByText("Review and save startup defaults")).toHaveClass("text-ink-muted");
  });
});
