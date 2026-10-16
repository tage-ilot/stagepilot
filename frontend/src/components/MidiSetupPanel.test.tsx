import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { MidiInputsResponse, MidiMonitorMessage, SettingsResponse } from "../types";
import { MidiSetupPanel } from "./MidiSetupPanel";

const midi: MidiInputsResponse = {
  enabled: true,
  channel: 3,
  note: 112,
  configured_input_name: "Startup Controller",
  selected_input_name: null,
  inputs: [
    {
      id: "a".repeat(64),
      name: "Playback Controller",
      ambiguous: false,
      selected: false,
      connected: false,
    },
  ],
  mappings: {
    start_next: 100,
    restart_current: 101,
    previous: 102,
    next: 103,
    reload_plan: 104,
    stop_timer: 105,
  },
};

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
      input_name: "Startup Controller",
      channel: 3,
      note: 112,
      mappings: midi.mappings,
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

function renderPanel({
  value = midi,
  onRefresh = vi.fn(),
  onSelect = vi.fn(),
  onSaveSettings = vi.fn(),
  messages = [],
  songs = [],
}: {
  value?: MidiInputsResponse;
  onRefresh?: () => void;
  onSelect?: (inputId: string | null) => void;
  onSaveSettings?: (value: SettingsResponse["settings"]["midi"]) => void;
  messages?: MidiMonitorMessage[];
  songs?: Parameters<typeof MidiSetupPanel>[0]["songs"];
} = {}) {
  render(
    <MidiSetupPanel
      error={null}
      message={null}
      midi={value}
      messages={messages}
      onRefresh={onRefresh}
      onSaveSettings={onSaveSettings}
      onSelect={onSelect}
      pendingOperation={null}
      settings={settings}
      songs={songs}
    />,
  );
}

describe("MidiSetupPanel", () => {
  it("is a setup Panel; the connection type and MIDI fields are reachable by label inside Fields", () => {
    renderPanel({});
    expect(document.getElementById("midi-configuration")).toHaveAttribute("data-variant", "setup");
    expect(document.getElementById("midi-configuration")!.className).not.toContain("setup-panel");
    expect(screen.getByRole("radiogroup", { name: "Connection method" })).toBeInTheDocument();
    for (const name of ["MIDI channel", "Fixed note", "Debounce (ms)"]) {
      expect(screen.getByLabelText(name).closest("[data-field]")).not.toBeNull();
    }
    expect(screen.getByRole("button", { name: "Save configuration" })).toHaveAttribute("data-variant", "primary");
  });
  it("shows the loaded service plan song-to-velocity map", () => {
    renderPanel({
      songs: [
        {
          id: "song-a",
          title: "Alpha",
          duration_seconds: 240,
          order: 1,
          is_generic: false,
          source_song_id: null,
        },
        {
          id: "song-b",
          title: "Beta",
          duration_seconds: 220,
          order: 2,
          is_generic: false,
          source_song_id: null,
        },
      ],
    });

    expect(screen.getByText("Playback song velocities")).toBeInTheDocument();
    expect(screen.getByText("Alpha").previousElementSibling).toHaveTextContent("1");
    expect(screen.getByText("Beta").previousElementSibling).toHaveTextContent("2");
  });

  it("shows persistent configuration and connects an available input", async () => {
    const onRefresh = vi.fn();
    const onSelect = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onRefresh, onSelect });

    expect(screen.getByText(/Receives song and action cues from Playback to control StagePilot/i)).toBeInTheDocument();
    expect(screen.getByText("Startup default: Startup Controller")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    await user.selectOptions(screen.getByLabelText("Available input"), "a".repeat(64));
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await user.click(screen.getByRole("button", { name: "Refresh inputs" }));

    expect(onSelect).toHaveBeenCalledWith("a".repeat(64));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("disables the input list when the MIDI plugin is disabled", () => {
    renderPanel({ value: { ...midi, enabled: false } });

    expect(screen.queryByText(/Save the MIDI settings above/i)).not.toBeInTheDocument();
    expect(document.body.textContent ?? "").not.toMatch(/restart stagepilot/i);
    expect(screen.getByLabelText("Available input")).toBeDisabled();
  });

  it("shows Connecting, then Connected, with no restart wording in any state", () => {
    const selected = { ...midi, selected_input_name: midi.inputs[0]!.name, inputs: midi.inputs.map((input, index) => ({ ...input, selected: index === 0, connected: false })) };
    cleanup();
    renderPanel({ value: selected });
    expect(screen.getByText(/Connecting to/)).toBeInTheDocument();
    expect(document.body.textContent ?? "").not.toMatch(/restart stagepilot/i);
    cleanup();
    renderPanel({ value: { ...selected, inputs: selected.inputs.map((input, index) => ({ ...input, connected: index === 0 })) } });
    expect(screen.getByText(/Connected:/)).toBeInTheDocument();
    expect(screen.queryByText(/Connecting to/)).not.toBeInTheDocument();
    expect(document.body.textContent ?? "").not.toMatch(/restart stagepilot/i);
  });

  it("the MIDI method tints the panel blue-violet but the Playback title keeps the Playback blue", async () => {
    const user = userEvent.setup();
    renderPanel();
    const panel = document.getElementById("midi-configuration")!;
    const title = screen.getByRole("heading", { name: "Playback" });
    expect(title).toHaveClass("text-integration-playback-soft");
    await user.click(screen.getByRole("radio", { name: /^Playback API/ }));
    expect(panel.className).toContain("border-t-integration-playback-soft");
    expect(panel.className).toContain("from-integration-playback/[0.13]");
    expect(title).toHaveClass("text-integration-playback-soft");
    await user.click(screen.getByRole("radio", { name: /^MIDI/ }));
    // The panel's own look (top border, background wash, title) never changes hue with the method.
    expect(panel.className).toContain("border-t-integration-playback-soft");
    expect(panel.className).toContain("from-integration-playback/[0.13]");
    expect(panel.className).not.toContain("integration-playback-midi");
    expect(title).toHaveClass("text-integration-playback-soft");
    expect(title).not.toHaveClass("text-integration-playback-midi-soft");
    // Only the controls inside take the blue-violet accent.
    expect(screen.getByRole("button", { name: "Refresh inputs" })).toHaveClass("text-integration-playback-midi-text");
  });

  it("saves advanced MIDI configuration without offering a simulated source", async () => {
    const onSaveSettings = vi.fn();
    const user = userEvent.setup();
    renderPanel({
      onSaveSettings,
      value: { ...midi, enabled: false },
    });

    expect(screen.queryByLabelText("MIDI source")).not.toBeInTheDocument();
    const fixedNote = screen.getByLabelText("Fixed note");
    expect(fixedNote).toHaveValue("112");
    expect(within(fixedNote).getAllByRole("option")).toHaveLength(128);
    expect(within(fixedNote).getByRole("option", { name: "E7 (MIDI 112)" })).toBeInTheDocument();
    expect(within(fixedNote).getByRole("option", { name: "A6 (MIDI 105)" })).toBeInTheDocument();
    await user.selectOptions(fixedNote, "105");
    await user.clear(screen.getByLabelText("MIDI channel"));
    await user.type(screen.getByLabelText("MIDI channel"), "4");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));

    expect(onSaveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: true, channel: 4, note: 105, debounce_ms: 250 }),
    );
  });

  it("shows received note details and the reason an input was ignored", () => {
    renderPanel({
      messages: [
        {
          timestamp: "2026-07-13T20:00:00Z",
          input_name: "Network Session 1",
          message_type: "note_on",
          channel: 2,
          note: 16,
          note_name: "E-1",
          velocity: 100,
          disposition: "wrong_channel",
          detail: "Ignored: StagePilot listens on channel 1.",
          action: null,
          simulated: false,
        },
      ],
    });

    const monitor = within(screen.getByRole("table"));
    expect(monitor.getByText("Network Session 1")).toBeInTheDocument();
    expect(monitor.getByText("E-1 (16)")).toBeInTheDocument();
    expect(monitor.getByText("2")).toBeInTheDocument();
    expect(monitor.getByText("100")).toBeInTheDocument();
    expect(monitor.getByText("wrong channel")).toHaveAttribute(
      "title",
      "Ignored: StagePilot listens on channel 1.",
    );
  });
});

