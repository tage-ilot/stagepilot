import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  ApplicationState,
  LightsStatusResponse,
  SettingsResponse,
  Song,
} from "../types";
import { LightsSetupPanel } from "./LightsSetupPanel";

afterEach(() => {
  vi.restoreAllMocks();
});

const song: Song = {
  id: "item-1",
  source_song_id: "song-1",
  title: "Holy Forever",
  duration_seconds: 336,
  order: 1,
  is_generic: false,
};

const lights: LightsStatusResponse = {
  enabled: true,
  output_name: "StagePilot to Lightkey",
  channel: 3,
  pulse_ms: 100,
  connection_status: "connected",
  detail: "Connected to lighting MIDI output StagePilot to Lightkey.",
  outputs: [{
    name: "StagePilot to Lightkey",
    ambiguous: false,
    selected: true,
    connected: true,
  }],
  last_cue: null,
  last_cue_at: null,
};

const settings: SettingsResponse = {
  planning_center_secret_saved: true,
  warning: null,
  restart_required: false,
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
      input_name: "Playback",
      channel: 1,
      note: 112,
      mappings: {},
      debounce_ms: 250,
    },
    lights: {
      enabled: true,
      output_name: "StagePilot to Lightkey",
      channel: 3,
      pulse_ms: 100,
      cue_maps: {
        "song-1": {
          song_key: "song-1",
          song_title: "Holy Forever",
          cues: [{
            id: "c17d19ab-1447-4e73-898e-468b2dfa87c7",
            at_seconds: 65,
            note: 72,
            velocity: 110,
            label: "First chorus",
          }],
        },
      },
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
};

const state: ApplicationState = {
  revision: 1,
  updated_at: "2026-07-15T19:00:00Z",
  application_status: "running",
  plan: {
    id: "plan-1",
    title: "Sunday",
    date: "2026-07-19",
    service_type: "Weekend",
    service_type_id: "service-type",
    service_times: ["09:00"],
    duration_source: "Planning Center",
    songs: [song],
  },
  current_song: song,
  next_song: null,
  current_song_index: 0,
  planning_center_status: "connected",
  midi_status: "connected",
  propresenter_status: "connected",
  lights_status: "connected",
  service_load: {
    status: "loaded",
    target_date: "2026-07-19",
    candidates: [],
    skipped_items: [],
    message: null,
    is_stale: false,
    last_attempt_at: "2026-07-15T19:00:00Z",
  },
  timer: { status: "running", duration_seconds: 336, started_at: "2026-07-15T19:00:00Z", last_error: null },
  plugins: {},
  recent_events: [],
  recent_errors: [],
  last_successful_plan_reload_at: "2026-07-15T19:00:00Z",
  last_action: "start_next",
};

describe("LightsSetupPanel", () => {
  it("uses the dashboard connection status while detail data is still loading", () => {
    render(
      <LightsSetupPanel
        connectionStatus="disconnected"
        error={null}
        lights={null}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={vi.fn()}
        onSaveSettings={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        settings={settings}
        state={{ ...state, lights_status: "disconnected" }}
      />,
    );

    expect(screen.getByText("Not connected")).toBeInTheDocument();
    expect(screen.queryByText("loading")).not.toBeInTheDocument();
  });

  it("loads and automatically saves a per-song elapsed-time cue map", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    render(
      <LightsSetupPanel
        error={null}
        lights={lights}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={onSaveCues}
        onSaveSettings={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        settings={settings}
        state={state}
      />,
    );

    expect(screen.getByDisplayValue("01:05")).toBeInTheDocument();
    expect(screen.getByDisplayValue("First chorus")).toBeInTheDocument();
    expect(screen.getAllByRole("option", { name: "C4" })).toHaveLength(2);

    const label = screen.getByLabelText("Lighting cue label");
    await user.clear(label);
    await user.type(label, "Chorus wash");
    await user.tab();

    expect(onSaveCues).toHaveBeenCalledOnce();
    expect(onSaveCues).toHaveBeenCalledWith(
      song,
      [expect.objectContaining({ at_seconds: 65, note: 72, velocity: 110, label: "Chorus wash" })],
    );
    expect(screen.queryByRole("button", { name: "Save song lighting cues" })).not.toBeInTheDocument();
  });

  it("toggles every note selector between music notation and numeric display", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    render(
      <LightsSetupPanel
        error={null}
        lights={lights}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={onSaveCues}
        onSaveSettings={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        settings={settings}
        state={state}
      />,
    );

    const displaySwitch = screen.getByRole("switch", {
      name: "MIDI note display: music notation",
    });
    const switchThumb = displaySwitch.querySelector("span");
    expect(displaySwitch).not.toBeChecked();
    expect(switchThumb).toHaveClass("left-0", "translate-x-1");
    expect(screen.getAllByRole("option", { name: "C4" })).toHaveLength(2);

    await user.click(displaySwitch);

    expect(
      screen.getByRole("switch", { name: "MIDI note display: numeric value" }),
    ).toBeChecked();
    expect(switchThumb).toHaveClass("left-0", "translate-x-7");
    expect(screen.getAllByRole("option", { name: "72" })).toHaveLength(2);
    expect(screen.getByLabelText("Lighting cue note")).toHaveValue("72");
    expect(onSaveCues).not.toHaveBeenCalled();
  });

  it("automatically saves added and individually removed cues", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    render(
      <LightsSetupPanel
        error={null}
        lights={lights}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={onSaveCues}
        onSaveSettings={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        settings={settings}
        state={state}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Add lighting cue" }));
    expect(onSaveCues).toHaveBeenLastCalledWith(song, [
      expect.objectContaining({ at_seconds: 0, note: 60, velocity: 127 }),
      expect.objectContaining({ at_seconds: 65, note: 72, velocity: 110 }),
    ]);

    await user.click(screen.getByRole("button", { name: "Remove cue at 01:05" }));
    expect(onSaveCues).toHaveBeenLastCalledWith(song, [
      expect.objectContaining({ at_seconds: 0, note: 60, velocity: 127 }),
    ]);
  });

  it("requires confirmation before clearing all cues", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(
      <LightsSetupPanel
        error={null}
        lights={lights}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={onSaveCues}
        onSaveSettings={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        settings={settings}
        state={state}
      />,
    );

    const clearAll = screen.getByRole("button", { name: "Clear all" });
    await user.click(clearAll);
    expect(confirm).toHaveBeenCalledWith(
      'Clear all lighting cues for "Holy Forever"? This cannot be undone.',
    );
    expect(onSaveCues).not.toHaveBeenCalled();

    await user.click(clearAll);
    expect(onSaveCues).toHaveBeenCalledWith(song, []);
  });

  it("sends an explicit Note On/Off compatibility test", async () => {
    const user = userEvent.setup();
    const onTest = vi.fn();
    render(
      <LightsSetupPanel
        error={null}
        lights={lights}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={vi.fn()}
        onSaveSettings={vi.fn()}
        onTest={onTest}
        pendingOperation={null}
        settings={settings}
        state={state}
      />,
    );

    await user.selectOptions(screen.getByLabelText("Test lighting note"), "76");
    await user.clear(screen.getByLabelText("Test lighting velocity"));
    await user.type(screen.getByLabelText("Test lighting velocity"), "115");
    await user.click(screen.getByRole("button", { name: "Send test cue" }));

    expect(onTest).toHaveBeenCalledWith(76, 115);
  });

  describe("Send test cue follows the saved output", () => {
    const reason = "Save the new output first. Test cue sends to the saved output.";
    const twoOutputs: LightsStatusResponse = {
      ...lights,
      outputs: [
        ...lights.outputs,
        { name: "Other MIDI port", ambiguous: false, selected: false, connected: false },
      ],
    };
    const withSaved = (output_name: string | null, channel: number): SettingsResponse => ({
      ...settings,
      settings: { ...settings.settings, lights: { ...settings.settings.lights, output_name, channel } },
    });
    const panel = (onTest: () => void, s: SettingsResponse, l: LightsStatusResponse = twoOutputs) => (
      <LightsSetupPanel
        error={null}
        lights={l}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={vi.fn()}
        onSaveSettings={vi.fn()}
        onTest={onTest}
        pendingOperation={null}
        settings={s}
        state={state}
      />
    );

    it("is enabled with no reason while the draft equals the saved output and channel", async () => {
      const onTest = vi.fn();
      const user = userEvent.setup();
      render(panel(onTest, settings));
      const button = screen.getByRole("button", { name: "Send test cue" });
      expect(button).toBeEnabled();
      expect(button).not.toHaveAttribute("aria-describedby");
      expect(screen.queryByText(reason)).not.toBeInTheDocument();
      await user.click(button);
      expect(onTest).toHaveBeenCalledTimes(1);
      expect(onTest).toHaveBeenCalledWith(60, 127);
    });

    it("is disabled with a described reason while a different output is selected", async () => {
      const onTest = vi.fn();
      const user = userEvent.setup();
      render(panel(onTest, settings));
      await user.selectOptions(screen.getByLabelText("MIDI output"), "Other MIDI port");
      const button = screen.getByRole("button", { name: "Send test cue" });
      expect(button).toBeDisabled();
      expect(button).toHaveAccessibleDescription(reason);
      expect(screen.getByText(reason)).toBeVisible();
      await user.click(button);
      expect(onTest).not.toHaveBeenCalled();
    });

    it("is disabled with a described reason while only the channel differs", async () => {
      const onTest = vi.fn();
      const user = userEvent.setup();
      render(panel(onTest, settings));
      await user.selectOptions(screen.getByLabelText("MIDI channel"), "5");
      const button = screen.getByRole("button", { name: "Send test cue" });
      expect(button).toBeDisabled();
      expect(button).toHaveAccessibleDescription(reason);
      await user.selectOptions(screen.getByLabelText("MIDI channel"), "3");
      expect(screen.getByRole("button", { name: "Send test cue" })).toBeEnabled();
      expect(screen.queryByText(reason)).not.toBeInTheDocument();
    });

    it("is disabled with the reason when no output is saved yet", () => {
      const noSaved = { ...twoOutputs, output_name: null };
      render(panel(vi.fn(), withSaved(null, 3), noSaved));
      const button = screen.getByRole("button", { name: "Send test cue" });
      expect(button).toBeDisabled();
      expect(button).toHaveAccessibleDescription(reason);
    });

    it("is enabled on the saved but unavailable output and disabled once another output is chosen", async () => {
      const user = userEvent.setup();
      const gone: LightsStatusResponse = { ...twoOutputs, outputs: twoOutputs.outputs.slice(1) };
      render(panel(vi.fn(), settings, gone));
      // saved output is missing from the list but still selected: draft equals saved
      expect(screen.getByRole("option", { name: /StagePilot to Lightkey \(currently unavailable\)/ })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Send test cue" })).toBeEnabled();
      await user.selectOptions(screen.getByLabelText("MIDI output"), "Other MIDI port");
      expect(screen.getByRole("button", { name: "Send test cue" })).toBeDisabled();
    });

    it("re-enables after the saved output updates to the draft and sends unchanged arguments", async () => {
      const onTest = vi.fn();
      const user = userEvent.setup();
      const { rerender } = render(panel(onTest, settings));
      await user.selectOptions(screen.getByLabelText("MIDI output"), "Other MIDI port");
      expect(screen.getByRole("button", { name: "Send test cue" })).toBeDisabled();
      rerender(panel(onTest, withSaved("Other MIDI port", 3)));
      const button = screen.getByRole("button", { name: "Send test cue" });
      expect(button).toBeEnabled();
      expect(button).not.toHaveAttribute("aria-describedby");
      await user.selectOptions(screen.getByLabelText("Test lighting note"), "76");
      await user.click(button);
      expect(onTest).toHaveBeenCalledWith(76, 127);
    });
  });

  it("shows the kicker, one sentence, a Basic heading and a visible label equal to each former aria-label-only control", () => {
    render(
      <LightsSetupPanel
        error={null}
        lights={lights}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={vi.fn()}
        onSaveSettings={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        settings={settings}
        state={state}
      />,
    );

    expect(screen.getByText("Connection configuration")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Lights" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Basic" })).toBeInTheDocument();
    for (const name of ["Test lighting note", "Test lighting velocity", "Cue elapsed time", "Lighting cue note", "Lighting cue velocity", "Lighting cue label"]) {
      const control = screen.getByLabelText(name);
      const visible = document.querySelector(`label[for="${control.id}"]`);
      expect(visible, name).not.toBeNull();
      expect(visible).toHaveTextContent(new RegExp(`^${name}$`));
      expect(control).toHaveAttribute("aria-label", name);
    }
  });
});
