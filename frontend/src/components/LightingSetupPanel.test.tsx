import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  ApplicationState,
  LightingCue,
  LightingMethod,
  OscLightingCue,
  LightsSettingsInput,
  LightsStatusResponse,
  SettingsResponse,
  Song,
} from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";

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
  method: "midi",
  osc_host: "127.0.0.1",
  osc_port: 21600,
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
      method: "midi",
          osc_host: "127.0.0.1",
      osc_port: 21600,
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
  playing_song: null, playing_song_index: null, up_next_song: null,
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

describe("LightingSetupPanel", () => {
  it("keeps 40 cues in a bounded scroll region and renders a 200-character cue label", () => {
    const label = "W".repeat(200);
    const cues = Array.from({ length: 40 }, (_, index) => ({ id: `c${index}`, at_seconds: index * 5, note: 60, velocity: 100, label: index === 0 ? label : `Cue ${index}` }));
    const many = { ...settings, settings: { ...settings.settings, lights: { ...settings.settings.lights, cue_maps: { "song-1": { song_key: "song-1", song_title: "Holy Forever", cues } } } } };
    render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={many} state={state} />,
    );
    const region = screen.getByTestId("lighting-cue-list");
    expect(region).toHaveClass("overflow-y-auto", "overscroll-contain");
    // A box that scrolls is an obvious bordered region (owner, 2026-10-15): bordered, edge glows, focusable and named.
    const frame = region.parentElement!;
    expect(frame).toHaveClass("scroll-region");
    expect(frame).toHaveAttribute("data-axis", "y");
    // The fade is an overlay ABOVE the scroller's contents (inputs and all), ignores the pointer, and is hidden at the ends.
    const fades = Array.from(frame.querySelectorAll<HTMLElement>(".scroll-fade"));
    expect(fades.map((fade) => fade.dataset.edge)).toEqual(["start", "end"]);
    for (const fade of fades) {
      expect(fade).toHaveAttribute("aria-hidden", "true");
      expect(region.contains(fade)).toBe(false);
      expect(frame.lastElementChild === fade || frame.children.length === 3).toBe(true);
    }
    expect(fades[0]).toHaveAttribute("data-visible", "false");
    expect(region).toHaveAttribute("role", "region");
    expect(region).toHaveAttribute("aria-label", "Lighting cues, scrollable");
    expect(region).toHaveAttribute("tabindex", "0");
    const labels = Array.from(region.querySelectorAll<HTMLInputElement>('input[aria-label="Cue label"]'));
    expect(labels).toHaveLength(40);
    expect(labels[0]).toHaveValue(label);
  });

  it("labels the cue fields Elapsed time and Cue label", () => {
    render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
    );
    expect(screen.getAllByLabelText("Elapsed time").length).toBeGreaterThan(0);
    expect(screen.getAllByLabelText("Cue label").length).toBeGreaterThan(0);
    expect(screen.queryByLabelText("Cue elapsed time")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Lighting cue label")).not.toBeInTheDocument();
  });

  describe("Song lighting timeline group", () => {
    const KEY = "stagepilot.lighting.timeline-open";
    const mount = () => render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
    );
    const header = () => screen.getByRole("button", { name: /^Song lighting timeline/ });
    beforeEach(() => window.localStorage.removeItem(KEY));

    it("starts open and holds the song picker, the cue board, the timeline and the cue list together", () => {
      mount();
      expect(header()).toHaveAttribute("aria-expanded", "true");
      expect(window.localStorage.getItem(KEY)).toBeNull();
      const body = header().parentElement!.querySelector("[data-part=region]")!;
      expect(within(body as HTMLElement).getByLabelText("Song")).toBeInTheDocument();
      expect(body.contains(screen.getByTestId("lighting-cue-list"))).toBe(true);
      expect(body.contains(screen.getByTestId("lighting-timeline"))).toBe(true);
    });

    it("collapses on click, saves the choice, and keeps the Test cue controls visible", async () => {
      const user = userEvent.setup();
      mount();
      await user.click(header());
      expect(header()).toHaveAttribute("aria-expanded", "false");
      expect(window.localStorage.getItem(KEY)).toBe("false");
      const body = header().parentElement!.querySelector("[data-part=region]")!;
      expect(body.contains(screen.getByRole("group", { name: "Test cue" }))).toBe(false);
      await user.click(header());
      expect(header()).toHaveAttribute("aria-expanded", "true");
      expect(window.localStorage.getItem(KEY)).toBe("true");
    });

    it("opens the way it was left after a restart", () => {
      window.localStorage.setItem(KEY, "false");
      mount();
      expect(header()).toHaveAttribute("aria-expanded", "false");
    });

    it("shows how many cues are saved while closed", async () => {
      window.localStorage.setItem(KEY, "false");
      mount();
      expect(header()).toHaveTextContent(/\d+ cues?|No cues yet/);
    });
  });

  it("keeps one short subtitle whichever connection method is chosen", async () => {
    const user = userEvent.setup();
    render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
    );
    const subtitle = "Sends timed cues to your lighting software.";
    expect(screen.getByText(subtitle)).toBeInTheDocument();
    const group = screen.getByRole("radiogroup", { name: "Connection method" });
    for (const name of [/MIDI/, /OSC/, /MIDI/]) {
      await user.click(within(group).getByRole("radio", { name }));
      expect(screen.getByText(subtitle)).toBeInTheDocument();
    }
    expect(screen.queryByText(/Note On\/Off/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Send elapsed-time/)).not.toBeInTheDocument();
  });

  it("draws no scroll box around an empty cue list", () => {
    const empty = { ...settings, settings: { ...settings.settings, lights: { ...settings.settings.lights, cue_maps: {} } } };
    render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={empty} state={state} />,
    );
    const region = screen.getByTestId("lighting-cue-list");
    expect(region.parentElement).not.toHaveClass("scroll-region");
    // Nothing to scroll: no frame is drawn, so no fade overlay is a sibling of the list.
    expect(Array.from(region.parentElement?.children ?? []).some((node) => node.classList.contains("scroll-fade"))).toBe(false);
    expect(region).not.toHaveAttribute("role");
    expect(region).not.toHaveAttribute("tabindex");
  });

  it("shows the test cue result in the Lighting colour with a dot; errors stay danger", () => {
    const { rerender } = render(
      <LightingSetupPanel error={null} lights={lights} message="Sent lighting test cue on MIDI note 60" onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
    );
    const status = screen.getByRole("status");
    expect(status).toHaveClass("text-integration-lighting-text");
    expect(status).toHaveTextContent("Sent lighting test cue on MIDI note 60");
    expect(status.querySelector('[data-part="dot"]')).toHaveClass("bg-integration-lighting-soft");
    rerender(
      <LightingSetupPanel error="Lighting failed" lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
    );
    expect(screen.queryByRole("status")).toBeNull();
    expect(document.querySelector('[data-part="dot"]')).toBeNull();
  });

  it("note display glyphs use the Lighting colour when active and muted when inactive", () => {
    render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
    );
    const glyphs = [screen.getByText("♪"), screen.getByText("#")];
    expect(glyphs.filter((g) => g.className.includes("text-integration-lighting-text"))).toHaveLength(1);
    expect(glyphs.filter((g) => g.className.includes("text-ink-muted"))).toHaveLength(1);
    const toggle = screen.getByRole("switch");
    expect(toggle.className).toContain("integration-lighting");
    expect(toggle.className).not.toContain("info");
  });

  it("has the shared Connection method switch (OSC primary, MIDI alternate) and no stray \"Sends cues by MIDI\" line", () => {
    render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
    );
    expect(screen.queryByText("Sends cues by MIDI")).not.toBeInTheDocument();
    expect(screen.getByRole("radiogroup", { name: "Connection method" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /OSC/ })).toHaveAttribute("value", "osc");
    expect(screen.getByRole("radio", { name: /MIDI/ })).toBeChecked();
  });

  it("uses the dashboard connection status while detail data is still loading", () => {
    render(
      <LightingSetupPanel
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

    expect(screen.getByText("Disconnected")).toBeInTheDocument();
    expect(screen.queryByText("loading")).not.toBeInTheDocument();
  });

  it("loads and automatically saves a per-song elapsed-time cue map", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    render(
      <LightingSetupPanel
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

    const label = screen.getByLabelText("Cue label");
    await user.clear(label);
    await user.type(label, "Chorus wash");
    await user.tab();

    expect(onSaveCues).toHaveBeenCalledOnce();
    expect(onSaveCues).toHaveBeenCalledWith(
      song,
      [expect.objectContaining({ at_seconds: 65, note: 72, velocity: 110, label: "Chorus wash" })],
      "midi",
    );
    expect(screen.queryByRole("button", { name: "Save song lighting cues" })).not.toBeInTheDocument();
  });

  it("toggles every note selector between music notation and numeric display", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    render(
      <LightingSetupPanel
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
      <LightingSetupPanel
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

    await user.click(screen.getByRole("button", { name: "Add cue" }));
    expect(onSaveCues).toHaveBeenLastCalledWith(song, [
      expect.objectContaining({ at_seconds: 0, note: 60, velocity: 127 }),
      expect.objectContaining({ at_seconds: 65, note: 72, velocity: 110 }),
    ], "midi");

    await user.click(screen.getByRole("button", { name: "Remove cue at 01:05" }));
    expect(onSaveCues).toHaveBeenLastCalledWith(song, [
      expect.objectContaining({ at_seconds: 0, note: 60, velocity: 127 }),
    ], "midi");
  });

  it("requires confirmation before clearing all cues", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(
      <LightingSetupPanel
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
    expect(onSaveCues).toHaveBeenCalledWith(song, [], "midi");
  });

  it("sends an explicit Note On/Off compatibility test", async () => {
    const user = userEvent.setup();
    const onTest = vi.fn();
    render(
      <LightingSetupPanel
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

    await user.selectOptions(screen.getByLabelText("Note"), "76");
    await user.clear(screen.getByLabelText("Velocity"));
    await user.type(screen.getByLabelText("Velocity"), "115");
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
      <LightingSetupPanel
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
      await user.selectOptions(screen.getByLabelText("MIDI Output"), "Other MIDI port");
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
      await user.selectOptions(screen.getByLabelText("MIDI Output"), "Other MIDI port");
      expect(screen.getByRole("button", { name: "Send test cue" })).toBeDisabled();
    });

    it("re-enables after the saved output updates to the draft and sends unchanged arguments", async () => {
      const onTest = vi.fn();
      const user = userEvent.setup();
      const { rerender } = render(panel(onTest, settings));
      await user.selectOptions(screen.getByLabelText("MIDI Output"), "Other MIDI port");
      expect(screen.getByRole("button", { name: "Send test cue" })).toBeDisabled();
      rerender(panel(onTest, withSaved("Other MIDI port", 3)));
      const button = screen.getByRole("button", { name: "Send test cue" });
      expect(button).toBeEnabled();
      expect(button).not.toHaveAttribute("aria-describedby");
      await user.selectOptions(screen.getByLabelText("Note"), "76");
      await user.click(button);
      expect(onTest).toHaveBeenCalledWith(76, 127);
    });
  });

  it("shows the kicker, one sentence, a visible label equal to each former aria-label-only control", () => {
    render(
      <LightingSetupPanel
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
    expect(screen.getByRole("heading", { name: "Lighting" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "General configuration" })).not.toBeInTheDocument();
    for (const name of ["Note", "Velocity", "Elapsed time", "Lighting cue note", "Lighting cue velocity", "Cue label"]) {
      const control = screen.getByLabelText(name);
      const visible = document.querySelector(`label[for="${control.id}"]`);
      expect(visible, name).not.toBeNull();
      expect(visible).toHaveTextContent(new RegExp(`^${name}$`));
      expect(control).toHaveAttribute("aria-label", name);
    }
  });

  describe("Save configuration bar", () => {
    const twoOutputs: LightsStatusResponse = {
      ...lights,
      outputs: [
        ...lights.outputs,
        { name: "Other MIDI port", ambiguous: false, selected: false, connected: false },
      ],
    };
    const make = (over: Partial<React.ComponentProps<typeof LightingSetupPanel>> = {}) => (
      <LightingSetupPanel
        error={null}
        lights={twoOutputs}
        message={null}
        onRefresh={vi.fn()}
        onSaveCues={vi.fn()}
        onSaveSettings={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        settings={settings}
        state={state}
        {...over}
      />
    );
    const bar = () => document.querySelector("[data-save-bar]") as HTMLElement;

    it("replaces Save and connect with one Save configuration, clean at first", () => {
      render(make());
      expect(screen.queryByRole("button", { name: "Save and connect" })).not.toBeInTheDocument();
      expect(screen.getAllByRole("button", { name: "Save configuration" })).toHaveLength(1);
      expect(bar()).toHaveAttribute("data-state", "clean");
      expect(screen.getByText("All changes saved")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Save configuration" })).toHaveAttribute("aria-disabled", "true");
    });

    it("holds output, channel and note length, counts three, and does not save on change", async () => {
      const user = userEvent.setup();
      const onSaveSettings = vi.fn();
      render(make({ onSaveSettings }));
      await user.selectOptions(screen.getByLabelText("MIDI Output"), "Other MIDI port");
      expect(bar()).toHaveTextContent("1 change waiting. Not saved yet.");
      await user.selectOptions(screen.getByLabelText("MIDI channel"), "5");
      expect(bar()).toHaveTextContent("2 changes waiting. Not saved yet.");
      const pulse = screen.getByLabelText("Note length (ms)");
      await user.clear(pulse);
      await user.type(pulse, "250{Enter}");
      await user.tab();
      expect(bar()).toHaveAttribute("data-state", "held");
      expect(bar()).toHaveTextContent("3 changes waiting. Not saved yet.");
      expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(3);
      expect(screen.getAllByText("Not saved yet").length).toBeGreaterThanOrEqual(3);
      expect(onSaveSettings).not.toHaveBeenCalled();
    });

    it("Note length alone is held, marked, and saved only by Save configuration", async () => {
      const user = userEvent.setup();
      const onSaveSettings = vi.fn();
      render(make({ onSaveSettings }));
      const pulse = screen.getByLabelText("Note length (ms)");
      await user.clear(pulse);
      await user.type(pulse, "250");
      await user.tab();
      expect(onSaveSettings).not.toHaveBeenCalled();
      expect(bar()).toHaveTextContent("1 change waiting. Not saved yet.");
      expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(1);
      await user.click(screen.getByRole("button", { name: "Save configuration" }));
      expect(onSaveSettings).toHaveBeenCalledTimes(1);
      expect(onSaveSettings).toHaveBeenCalledWith({ enabled: true, method: "midi", output_name: "StagePilot to Lightkey", channel: 3, pulse_ms: 250, osc_host: "127.0.0.1", osc_port: 21600 });
    });

    it("blocks Save with a reason while Note length is out of range", async () => {
      const user = userEvent.setup();
      const onSaveSettings = vi.fn();
      render(make({ onSaveSettings }));
      const pulse = screen.getByLabelText("Note length (ms)");
      await user.clear(pulse);
      await user.type(pulse, "5");
      await user.tab();
      expect(screen.getByText(/note length from 10 to 2000 ms/)).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Save configuration" }));
      expect(onSaveSettings).not.toHaveBeenCalled();
    });

    it("keeps the typed Note length and held markers after a failed save", async () => {
      const user = userEvent.setup();
      const onSaveSettings = vi.fn();
      const view = render(make({ onSaveSettings }));
      await user.selectOptions(screen.getByLabelText("MIDI channel"), "5");
      const pulse = screen.getByLabelText("Note length (ms)");
      await user.clear(pulse);
      await user.type(pulse, "250");
      await user.click(screen.getByRole("button", { name: "Save configuration" }));
      view.rerender(make({ onSaveSettings, pendingOperation: "save" }));
      view.rerender(make({ onSaveSettings, error: "No port" }));
      expect(screen.getByLabelText("Note length (ms)")).toHaveValue(250);
      expect(screen.getByLabelText("MIDI channel")).toHaveValue("5");
      expect(bar()).toHaveAttribute("data-state", "error");
      expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(2);
    });

    it("sends a byte-identical payload to onSaveSettings", async () => {
      const user = userEvent.setup();
      const onSaveSettings = vi.fn();
      render(make({ onSaveSettings }));
      await user.selectOptions(screen.getByLabelText("MIDI Output"), "Other MIDI port");
      await user.selectOptions(screen.getByLabelText("MIDI channel"), "5");
      await user.click(screen.getByRole("button", { name: "Save configuration" }));
      const payload = onSaveSettings.mock.calls[0]?.[0];
      expect(payload).toEqual({ enabled: true, method: "midi", output_name: "Other MIDI port", channel: 5, pulse_ms: 100, osc_host: "127.0.0.1", osc_port: 21600 });
      expect(JSON.stringify(payload)).toBe('{"enabled":true,"method":"midi","output_name":"Other MIDI port","channel":5,"pulse_ms":100,"osc_host":"127.0.0.1","osc_port":21600}');
    });

    it("a clean Save does nothing once Lighting is enabled", async () => {
      const user = userEvent.setup();
      const onSaveSettings = vi.fn();
      render(make({ onSaveSettings }));
      await user.click(screen.getByRole("button", { name: "Save configuration" }));
      expect(onSaveSettings).not.toHaveBeenCalled();
    });

    it("Discard restores the saved output, channel and note length", async () => {
      const user = userEvent.setup();
      render(make());
      await user.selectOptions(screen.getByLabelText("MIDI Output"), "Other MIDI port");
      await user.selectOptions(screen.getByLabelText("MIDI channel"), "5");
      await user.clear(screen.getByLabelText("Note length (ms)"));
      await user.type(screen.getByLabelText("Note length (ms)"), "250");
      await user.click(screen.getByRole("button", { name: "Discard" }));
      expect(screen.getByLabelText("MIDI Output")).toHaveValue("StagePilot to Lightkey");
      expect(screen.getByLabelText("MIDI channel")).toHaveValue("3");
      expect(screen.getByLabelText("Note length (ms)")).toHaveValue(100);
      expect(bar()).toHaveAttribute("data-state", "clean");
    });

    it("shows saving, then Couldn't save with Try again that resends the same payload", async () => {
      const user = userEvent.setup();
      const onSaveSettings = vi.fn();
      const view = render(make({ onSaveSettings }));
      await user.selectOptions(screen.getByLabelText("MIDI channel"), "5");
      view.rerender(make({ onSaveSettings, pendingOperation: "save" }));
      expect(bar()).toHaveAttribute("data-state", "saving");
      expect(screen.getByText("Saving…")).toBeInTheDocument();
      view.rerender(make({ onSaveSettings, error: "No port" }));
      expect(bar()).toHaveAttribute("data-state", "error");
      expect(screen.getByText("Couldn't save")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Try again" }));
      expect(onSaveSettings).toHaveBeenCalledWith({ enabled: true, method: "midi", output_name: "StagePilot to Lightkey", channel: 5, pulse_ms: 100, osc_host: "127.0.0.1", osc_port: 21600 });
    });

    it("shows Saved automatically only after a cue save, and keeps Refresh and Send test cue as actions", () => {
      const view = render(make({ pendingOperation: "save-cues" }));
      expect(bar()).toHaveAttribute("data-state", "clean");
      view.rerender(make());
      expect(bar()).toHaveAttribute("data-state", "auto");
      expect(bar()).toHaveTextContent(/Saved automatically at/);
      expect(screen.getByRole("button", { name: "Refresh outputs" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Send test cue" })).toBeInTheDocument();
    });

    it("does not show auto after a failed cue save", () => {
      const view = render(make({ pendingOperation: "save-cues" }));
      view.rerender(make({ error: "boom" }));
      expect(bar()).toHaveAttribute("data-state", "clean");
    });
  });
});

describe("Lighting hold-until-saved conformance (guard)", () => {
  const twoOutputs: LightsStatusResponse = {
    ...lights,
    outputs: [
      { name: "StagePilot to Lightkey", ambiguous: false, selected: true, connected: true },
      { name: "Other output", ambiguous: false, selected: false, connected: false },
    ],
  };
  type Fn<A extends unknown[]> = (...args: A) => void;
  const mount = (extra: { onSaveSettings?: Fn<[LightsSettingsInput]>; onSaveCues?: Fn<[Song, LightingCue[] | OscLightingCue[], LightingMethod]>; onTest?: Fn<[number, number]>; onDirtyChange?: Fn<[boolean]> } = {}) => render(
    <LightingSetupPanel
      error={null}
      lights={twoOutputs}
      message={null}
      onDirtyChange={extra.onDirtyChange}
      onRefresh={vi.fn()}
      onSaveCues={extra.onSaveCues ?? vi.fn()}
      onSaveSettings={extra.onSaveSettings ?? vi.fn()}
      onTest={extra.onTest ?? vi.fn()}
      pendingOperation={null}
      settings={settings}
      state={state}
    />,
  );
  const bar = () => document.querySelector("[data-save-bar]")!;

  // Held controls: class (a). Every one must be marked, counted, restored by Discard, drive the dirty flag, and not save on change/blur.
  const held: Array<{ label: string; value: string; saved: string; payload: Record<string, unknown> }> = [
    { label: "MIDI Output", value: "Other output", saved: "StagePilot to Lightkey", payload: { output_name: "Other output" } },
    { label: "MIDI channel", value: "5", saved: "3", payload: { channel: 5 } },
    { label: "Note length (ms)", value: "250", saved: "100", payload: { pulse_ms: 250 } },
  ];
  it.each(held)("$label is held: marked, counted, not saved on blur, flagged dirty, restored by Discard, saved only by Save configuration", ({ label, value, saved, payload }) => {
    const onSaveSettings = vi.fn();
    const onDirtyChange = vi.fn();
    mount({ onSaveSettings, onDirtyChange });
    const control = screen.getByLabelText(label);
    fireEvent.change(control, { target: { value } });
    fireEvent.blur(control);
    fireEvent.keyDown(control, { key: "Enter" });
    expect(onSaveSettings).not.toHaveBeenCalled();
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(1);
    expect(bar()).toHaveTextContent("1 change waiting. Not saved yet.");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSaveSettings).toHaveBeenCalledOnce();
    expect(onSaveSettings).toHaveBeenCalledWith(expect.objectContaining(payload));
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect((screen.getByLabelText(label) as HTMLInputElement).value).toBe(saved);
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(0);
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  // Immediate cue controls: class (b), saved on commit through onSaveCues, never counted as a held change.
  it("cue label, velocity, note and Remove save immediately and never appear as held changes", () => {
    const onSaveCues = vi.fn();
    mount({ onSaveCues });
    const label = screen.getByLabelText("Cue label");
    fireEvent.change(label, { target: { value: "Bridge" } });
    fireEvent.blur(label);
    const velocity = screen.getByLabelText("Lighting cue velocity");
    fireEvent.change(velocity, { target: { value: "90" } });
    fireEvent.blur(velocity);
    fireEvent.change(screen.getByLabelText("Lighting cue note"), { target: { value: "76" } });
    expect(onSaveCues).toHaveBeenCalledTimes(3);
    fireEvent.click(screen.getByRole("button", { name: /^Remove cue at/ }));
    expect(onSaveCues).toHaveBeenCalledTimes(4);
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(0);
    expect(bar()).toHaveAttribute("data-state", "clean");
  });

  // Explicit actions: class (c).
  it("Refresh outputs, Send test cue and the note-display switch are explicit actions that never touch held state", () => {
    const onTest = vi.fn();
    mount({ onTest });
    fireEvent.click(screen.getByRole("button", { name: "Send test cue" }));
    expect(onTest).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("switch"));
    fireEvent.change(screen.getByLabelText("Velocity"), { target: { value: "100" } });
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(0);
    expect(bar()).toHaveAttribute("data-state", "clean");
  });

  it("per-cue Test is disabled with the same reason as Send test cue while the draft output differs from the saved one", () => {
    const onTest = vi.fn();
    mount({ onTest });
    const cueTest = screen.getAllByRole("button", { name: "Test" })[0]!;
    expect(cueTest).toBeEnabled();
    fireEvent.change(screen.getByLabelText("MIDI Output"), { target: { value: "Other output" } });
    expect(cueTest).toBeDisabled();
    expect(cueTest).toHaveAttribute("aria-describedby", screen.getByRole("button", { name: "Send test cue" }).getAttribute("aria-describedby"));
    fireEvent.click(cueTest);
    expect(onTest).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(screen.getAllByRole("button", { name: "Test" })[0]).toBeEnabled();
  });
});
