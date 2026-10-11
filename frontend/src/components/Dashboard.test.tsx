import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi, afterEach } from "vitest";

import type {
  ApplicationState,
  MidiInputsResponse,
  ServiceLoadState,
  ServicePlan,
  SettingsResponse,
  ProPresenterStatusResponse,
} from "../types";
import type { UpdaterController } from "../hooks/useUpdater";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { playbackStatus } from "../test/playbackFixtures";
import { Dashboard } from "./Dashboard";

const loadedPlan: ServicePlan = {
  id: "previous-plan",
  title: "Previous Sunday Service",
  date: "2026-07-13",
  service_type: "Weekend Services",
  service_type_id: "weekend",
  service_times: ["09:00"],
  duration_source: "Planning Center scheduled item length",
  songs: [
    {
      id: "item-1",
      title: "Holy Forever",
      duration_seconds: 336,
      order: 1,
      service_sequence: 20,
      is_generic: false,
      source_song_id: "song-1",
    },
  ],
};

const loadedServiceState: ServiceLoadState = {
  status: "loaded",
  target_date: "2026-07-13",
  candidates: [],
  skipped_items: [],
  message: null,
  is_stale: false,
  last_attempt_at: "2026-07-13T16:00:00Z",
};

const ambiguousServiceState: ServiceLoadState = {
  status: "ambiguous",
  target_date: "2026-07-19",
  candidates: [
    {
      id: "plan-early",
      title: "Sunday Morning",
      service_type_id: "weekend",
      service_type_name: "Weekend Services",
      target_date: "2026-07-19",
      service_times: ["09:00"],
    },
    {
      id: "plan-late",
      title: "Sunday Evening",
      service_type_id: "weekend",
      service_type_name: "Weekend Services",
      target_date: "2026-07-19",
      service_times: ["18:00"],
    },
  ],
  skipped_items: [],
  message: "Multiple plans match the next service date.",
  is_stale: true,
  last_attempt_at: "2026-07-13T16:00:00Z",
};

const midi: MidiInputsResponse = {
  enabled: true,
  channel: 1,
  note: 112,
  configured_input_name: null,
  selected_input_name: "Playback",
  inputs: [{
    id: "playback",
    name: "Playback",
    ambiguous: false,
    selected: true,
    connected: true,
  }],
  mappings: {
    start_next: 100,
    restart_current: 101,
    previous: 102,
    next: 103,
    reload_plan: 104,
    stop_timer: 105,
  },
};

const productionSettings: SettingsResponse = {
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
    lan_access: false,
    planning_center: {
      app_id: "app-id",
      service_type_id: "weekend",
      plan_title_preference: null,
      preferred_service_time: null,
      upcoming_lookahead_days: 7,
      request_timeout_seconds: 10,
    },
    midi: {
      enabled: true,
      input_name: "Playback",
      channel: 1,
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
      look_id: null,
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

const propresenter: ProPresenterStatusResponse = {
  enabled: true,
  host: "127.0.0.1",
  port: 1025,
  timer_name: "Song Countdown",
  look_id: null,
  request_timeout_seconds: 3,
  connection_status: "connected",
  detail: "ProPresenter API connected.",
  timers: [],
  selected_timer_id: "timer-1",
  timer_found: true,
  looks: [],
  current_look_id: null,
  look_found: true,
  last_checked_at: "2026-07-13T16:00:00Z",
};

function applicationState(
  serviceLoad: ServiceLoadState = loadedServiceState,
  overrides: Partial<ApplicationState> = {},
): ApplicationState {
  return {
    revision: 7,
    updated_at: "2026-07-13T16:00:00Z",
    application_status: "running",
    plan: loadedPlan,
    current_song: null,
    next_song: loadedPlan.songs[0] ?? null,
    current_song_index: null,
    planning_center_status: "connected",
    midi_status: "connected",
    propresenter_status: "connected",
    lights_status: "disconnected",
    service_load: serviceLoad,
    timer: {
      status: "stopped",
      duration_seconds: null,
      started_at: null,
      last_error: null,
    },
    plugins: {
      demo: {
        name: "demo",
        version: "0.1.0",
        status: "running",
        last_error: null,
        last_activity_at: "2026-07-13T16:00:00Z",
      },
    },
    recent_events: [],
    recent_errors: [],
    last_successful_plan_reload_at: "2026-07-12T16:00:00Z",
    last_action: null,
    ...overrides,
  };
}

function renderDashboard(
  serviceLoad: ServiceLoadState,
  {
    actionMessage = null,
    error = null,
    pendingPlanId = null,
    selectPlan = vi.fn(),
    state = applicationState(serviceLoad),
    settings = productionSettings,
    updater,
    playback,
  }: {
    actionMessage?: string | null;
    error?: string | null;
    pendingPlanId?: string | null;
    selectPlan?: (planId: string) => void;
    state?: ApplicationState;
    settings?: SettingsResponse | null;
    updater?: UpdaterController;
    playback?: PlaybackController;
  } = {},
) {
  return render(
    <Dashboard
      actionMessage={actionMessage}
      dispatch={vi.fn()}
      error={error}
      health={null}
      live
      midi={midi}
      midiMessages={[]}
      midiError={null}
      midiMessage={null}
      pendingAction={null}
      pendingMidiCue={null}
      pendingMidiOperation={null}
      pendingPlanId={pendingPlanId}
      propresenter={propresenter}
      refreshMidi={vi.fn()}
      selectMidi={vi.fn()}
      selectPlan={selectPlan}
      settings={settings}
      simulateMidi={vi.fn()}
      state={state}
      updater={updater}
      playback={playback}
    />,
  );
}

beforeEach(() => {
  window.localStorage.removeItem("stagepilot.dashboard-layout.v2");
  window.localStorage.removeItem("stagepilot.dashboard-layout.invalid");
});

describe("Recent event stream 5-minute expiry", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("hides an event older than 5 minutes while a 30-second-old event still shows", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:05:30Z"));
    const state = applicationState(loadedServiceState, {
      recent_events: [
        { id: "old", type: "song_started", timestamp: "2026-07-13T16:00:00Z", source: "propresenter" },
        { id: "fresh", type: "song_started", timestamp: "2026-07-13T16:05:00Z", source: "propresenter" },
      ],
    });
    renderDashboard(loadedServiceState, { state });

    expect(document.querySelectorAll(".event-row")).toHaveLength(1);
    expect(screen.getByText("song_started")).toBeInTheDocument();
  });

  it("re-evaluates live: an event disappears on its own once it crosses 5 minutes", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:00:00Z"));
    const state = applicationState(loadedServiceState, {
      recent_events: [
        { id: "will-expire", type: "song_started", timestamp: "2026-07-13T16:00:00Z", source: "propresenter" },
      ],
    });
    renderDashboard(loadedServiceState, { state });

    expect(screen.getByText("song_started")).toBeInTheDocument();

    await act(async () => {
      vi.setSystemTime(new Date("2026-07-13T16:05:05Z"));
      await vi.advanceTimersByTimeAsync(6_000);
    });

    expect(screen.queryByText("song_started")).toBeNull();
  });

  it("leaves the pinned active-error mechanism unaffected by event age", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:00:00Z"));
    const state = applicationState(loadedServiceState, {
      propresenter_status: "error",
      recent_errors: [
        { component: "propresenter", event_id: "active-error", message: "Could not connect.", timestamp: "2026-07-13T10:00:00Z" },
      ],
      recent_events: [
        { id: "active-error", type: "connection.changed", timestamp: "2026-07-13T10:00:00Z", source: "propresenter" },
      ],
    });
    renderDashboard(loadedServiceState, { state });

    const errorMessage = screen.getByText(/Could not connect\./);
    expect(errorMessage).toBeInTheDocument();
    expect(errorMessage.closest("p")?.textContent).toMatch(/propresenter/i);
    expect(document.querySelectorAll(".border-danger\\/15")).toHaveLength(1);
    expect(screen.getByText("connection.changed")).toBeInTheDocument();
    expect(document.querySelectorAll(".event-row")).toHaveLength(1);

    await act(async () => {
      vi.setSystemTime(new Date("2026-07-13T16:10:00Z"));
      await vi.advanceTimersByTimeAsync(6_000);
    });

    // The pinned error and its matching raw event stay visible regardless of age.
    expect(screen.getByText(/Could not connect\./)).toBeInTheDocument();
    expect(screen.getByText("connection.changed")).toBeInTheDocument();
    expect(document.querySelectorAll(".event-row")).toHaveLength(1);
    expect(document.querySelectorAll(".border-danger\\/15")).toHaveLength(1);
  });

  it("keeps the pinned error inspectable once its raw event is evicted from recent_events", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:00:00Z"));
    const state = applicationState(loadedServiceState, {
      propresenter_status: "error",
      recent_errors: [
        { component: "propresenter", event_id: "evicted-error", message: "Could not connect.", timestamp: "2026-07-13T10:00:00Z" },
      ],
      // The matching raw event ("evicted-error") is no longer present because
      // 100 newer events pushed it out of the capped recent_events list.
      recent_events: [
        { id: "still-present", type: "song_started", timestamp: "2026-07-13T15:59:45Z", source: "propresenter" },
      ],
    });
    renderDashboard(loadedServiceState, { state });

    expect(screen.getByText(/Could not connect\./)).toBeInTheDocument();
    // A synthesized row keeps the pinned error's original failure inspectable
    // in the event stream even though its raw event was evicted.
    expect(screen.getByText("propresenter error")).toBeInTheDocument();
    expect(screen.getByText("song_started")).toBeInTheDocument();
  });
});

describe("Dashboard Planning Center plan states", () => {
  it("places the update control beside the StagePilot brand only when available", () => {
    const updater: UpdaterController = {
      status: "available",
      currentVersion: "1.1.5",
      availableVersion: "1.2.0",
      releaseNotes: null,
      releaseDate: null,
      progress: null,
      error: null,
      errorDialogOpen: false,
      successMessage: null,
      installBlockedReason: null,
      checkForUpdate: vi.fn(),
      openConfirmation: vi.fn(),
      cancelConfirmation: vi.fn(),
      install: vi.fn(),
      retry: vi.fn(),
      closeError: vi.fn(),
    };
    renderDashboard(loadedServiceState, { updater });

    const brand = screen.getByRole("heading", { name: "StagePilot" }).parentElement;
    const button = screen.getByRole("button", {
      name: "Update StagePilot to version 1.2.0",
    });
    expect(brand).toContainElement(button);
    expect(screen.getByRole("heading", { name: "StagePilot" })).toHaveClass("select-none");
    expect(screen.getByRole("heading", { name: "StagePilot" })).toHaveClass(
      "-translate-y-4",
    );
    expect(screen.getByRole("heading", { name: "StagePilot" })).not.toHaveClass("scale-[1.8]");
  });

  it("does not reserve header space for a current updater state", () => {
    const updater: UpdaterController = {
      status: "current",
      currentVersion: "1.1.5",
      availableVersion: null,
      releaseNotes: null,
      releaseDate: null,
      progress: null,
      error: null,
      errorDialogOpen: false,
      successMessage: null,
      installBlockedReason: null,
      checkForUpdate: vi.fn(),
      openConfirmation: vi.fn(),
      cancelConfirmation: vi.fn(),
      install: vi.fn(),
      retry: vi.fn(),
      closeError: vi.fn(),
    };
    renderDashboard(loadedServiceState, { updater });

    expect(screen.queryByRole("button", { name: /Update StagePilot/ })).not.toBeInTheDocument();
  });

  it("renders action notifications in the reserved header slot", () => {
    renderDashboard(loadedServiceState, {
      actionMessage: "Service position and timer reset.",
    });

    const notification = screen.getByRole("status");
    const header = screen.getByRole("banner");
    expect(header).toContainElement(notification);
    expect(notification).toHaveTextContent("Service position and timer reset.");
    expect(notification).toHaveClass("h-9", "w-fit", "max-w-full", "truncate");
  });

  it("keeps the same reserved header slot when no notification is visible", () => {
    renderDashboard(loadedServiceState);

    const header = screen.getByRole("banner");
    const notification = header.querySelector('[role="status"]');
    expect(notification).toHaveClass("h-9", "invisible");
  });

  it("queues simultaneous action and Planning Center notifications in the header", () => {
    vi.useFakeTimers();
    try {
      renderDashboard({
        ...loadedServiceState,
        status: "loading",
        message: "Looking for the current or next upcoming Planning Center plan.",
        is_stale: true,
      }, {
        actionMessage: "Service plan reload requested.",
      });

      const notification = screen.getByRole("status");
      expect(notification).toHaveTextContent("Service plan reload requested.");
      expect(screen.queryByText(/Looking for the current or next upcoming Planning Center plan/)).not.toBeInTheDocument();

      act(() => vi.advanceTimersByTime(6_000));
      expect(notification).toHaveTextContent(
        "Looking for the current or next upcoming Planning Center plan. The last successful plan is still displayed as stale.",
      );

      act(() => vi.advanceTimersByTime(6_000));
      expect(notification).toHaveClass("invisible");
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });

  it("retains only the two newest header notifications", () => {
    vi.useFakeTimers();
    try {
      renderDashboard({
        ...loadedServiceState,
        status: "loading",
        message: "Looking for the current or next upcoming Planning Center plan.",
      }, {
        actionMessage: "Service plan reload requested.",
        error: "Older backend error.",
      });

      const notification = screen.getByRole("status");
      expect(notification).toHaveTextContent("Service plan reload requested.");
      expect(notification).not.toHaveTextContent("Older backend error.");

      act(() => vi.advanceTimersByTime(6_000));
      expect(notification).toHaveTextContent(
        "Looking for the current or next upcoming Planning Center plan.",
      );

      act(() => vi.advanceTimersByTime(6_000));
      expect(notification).toHaveClass("invisible");
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });

  it("shows errors in the same header slot with error styling", () => {
    renderDashboard(loadedServiceState, { error: "Backend unavailable." });

    expect(screen.getByRole("status")).toHaveClass("border-danger/25");
  });

  it("renders ambiguous candidates and sends the selected plan ID", async () => {
    const selectPlan = vi.fn();
    const user = userEvent.setup();
    renderDashboard(ambiguousServiceState, { selectPlan });

    expect(screen.getByText("Plan selection required")).toBeInTheDocument();
    expect(screen.getByText("Sunday Morning")).toBeInTheDocument();
    expect(screen.getByText("Sunday Evening")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Multiple plans match 2026-07-19" })).toBeInTheDocument();
    expect(screen.getByText("Weekend Services \u00B7 09:00")).toBeInTheDocument();
    expect(screen.getByText("Weekend Services \u00B7 18:00")).toBeInTheDocument();

    expect(screen.getByText(ambiguousServiceState.message!)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Use Sunday Evening" }));

    expect(selectPlan).toHaveBeenCalledOnce();
    expect(selectPlan).toHaveBeenCalledWith("plan-late");
  });

  it("disables every candidate while a plan selection is pending", () => {
    renderDashboard(ambiguousServiceState, { pendingPlanId: "plan-late" });

    expect(screen.getByRole("button", { name: "Use Sunday Morning" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Loading Sunday Evening" })).toBeDisabled();
  });

  it("does not report readiness when the retained plan is stale", () => {
    renderDashboard({ ...loadedServiceState, is_stale: true });

    expect(screen.getByText("Check system")).toBeInTheDocument();
    expect(screen.getByRole("tooltip").firstElementChild).toHaveClass("bg-surface-950/85");
  });

  it("keeps readiness details open across its trigger and panel hover region", () => {
    vi.useFakeTimers();
    try {
      renderDashboard({ ...loadedServiceState, is_stale: true });

      const status = screen.getByRole("button", { name: "Check system" });
      const hoverArea = status.parentElement!;
      const tooltip = screen.getByRole("tooltip");

      fireEvent.mouseEnter(hoverArea);
      act(() => vi.advanceTimersByTime(499));
      expect(tooltip).toHaveClass("invisible");
      act(() => vi.advanceTimersByTime(1));
      expect(tooltip).toHaveClass("visible");

      fireEvent.mouseEnter(tooltip);
      expect(tooltip).toHaveClass("visible");
      fireEvent.mouseLeave(hoverArea);
      expect(tooltip).toHaveClass("invisible");
      fireEvent.click(status);
      expect(tooltip).toHaveClass("visible");
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not report readiness when the loaded plan date differs from the target date", () => {
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, {
        plan: { ...loadedPlan, date: "2026-07-12" },
      }),
    });

    expect(screen.getByText("Check system")).toBeInTheDocument();
  });

  it("does not require the demo integration in production mode", async () => {
    const user = userEvent.setup();
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    expect(screen.getByRole("heading", { name: "StagePilot" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Playback connection" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback connected/ }));
    expect(screen.getByRole("heading", { name: "Playback connection" })).toBeInTheDocument();
    expect(screen.getAllByText("Connected to Playback").length).toBeGreaterThan(0);
    expect(screen.queryByText("Demo integration running")).not.toBeInTheDocument();
    expect(screen.getAllByText("Ready").length).toBeGreaterThan(0);
    expect(screen.getByText("Lights MIDI output disconnected")).toHaveClass("text-ink-muted");
    expect(screen.getByText("Optional")).toBeInTheDocument();
  });

  it("keeps MIDI setup closed until its connection card is clicked", async () => {
    const user = userEvent.setup();
    renderDashboard(loadedServiceState);

    expect(screen.queryByRole("heading", { name: "Playback connection" })).not.toBeInTheDocument();
    expect(screen.getByText("Connected to Playback")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback connected/ }));

    expect(screen.getByRole("heading", { name: "Playback connection" })).toBeInTheDocument();
  });

  describe("leaving Playback with unsaved changes", () => {
    const playback = (): PlaybackController => ({
      status: playbackStatus(), events: [], error: null, message: null, pending: null,
      save: vi.fn().mockResolvedValue(true), selectSource: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(),
    });
    const openPlayback = async () => {
      const user = userEvent.setup();
      renderDashboard(loadedServiceState, { playback: playback() });
      await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback/ }));
      return user;
    };

    it("asks 'Discard unsaved changes?' when closing a dirty panel; Keep editing stays, Discard closes", async () => {
      const user = await openPlayback();
      await user.click(screen.getByLabelText("Connect to Playback automatically"));
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      expect(screen.getByRole("alertdialog", { name: "Discard unsaved changes?" })).toBeVisible();
      await user.click(screen.getByRole("button", { name: "Keep editing" }));
      expect(screen.getByRole("heading", { name: "Playback connection" })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      await user.click(screen.getByRole("button", { name: "Discard" }));
      expect(screen.queryByRole("heading", { name: "Playback connection" })).not.toBeInTheDocument();
    });

    it("also asks when switching to another tab, and never asks when nothing changed", async () => {
      const user = await openPlayback();
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback/ }));
      await user.click(screen.getByLabelText("Connect to Playback automatically"));
      await user.click(screen.getByRole("button", { name: /^ProPresenter/ }));
      expect(screen.getByRole("alertdialog", { name: "Discard unsaved changes?" })).toBeVisible();
    });
  });

  it("names the cause on the Error pill and ignores non-blocking subsystems", () => {
    const state = applicationState(loadedServiceState, { midi_status: "connected", propresenter_status: "error" });
    renderDashboard(loadedServiceState, { state });
    const pill = screen.getByRole("button", { name: /^Error/ });
    expect(pill).toHaveTextContent(/Error: .+/);
    expect(pill.getAttribute("title")).toMatch(/^Error: .+/);
  });

  it("uses clear failure labels for readiness checks", () => {
    const unavailableState = applicationState(
      { ...loadedServiceState, status: "not_found" },
      {
        plan: null,
        planning_center_status: "disconnected",
        midi_status: "disconnected",
        propresenter_status: "disconnected",
        lights_status: "disconnected",
      },
    );

    renderDashboard({ ...loadedServiceState, status: "not_found" }, {
      state: unavailableState,
    });

    expect(screen.getByText("Planning Center disconnected")).toBeInTheDocument();
    expect(screen.getByText("Planning Center plan not loaded")).toBeInTheDocument();
    expect(screen.getByText("Song durations invalid")).toBeInTheDocument();
    expect(screen.getByText("Playback disconnected")).toBeInTheDocument();
    expect(screen.getByText("ProPresenter disconnected")).toBeInTheDocument();
  });

  it("shows when the service plan was last successfully loaded", () => {
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, {
        last_successful_plan_reload_at: "2026-06-15T18:28:00",
      }),
    });

    expect(screen.getByText("Current as of 18:28 06-15-2026")).toBeInTheDocument();
    expect(screen.queryByText("Planning Center scheduled item length")).not.toBeInTheDocument();
  });

  it("displays a loaded upcoming plan as ready", () => {
    const upcomingServiceLoad: ServiceLoadState = {
      ...loadedServiceState,
      target_date: "2026-07-19",
      last_attempt_at: "2026-07-13T16:00:00Z",
    };
    const upcomingPlan: ServicePlan = {
      ...loadedPlan,
      id: "upcoming-plan",
      title: "Upcoming Sunday Service",
      date: "2026-07-19",
    };

    renderDashboard(upcomingServiceLoad, {
      state: applicationState(upcomingServiceLoad, {
        plan: upcomingPlan,
        plugins: {},
      }),
    });

    expect(screen.getByText("Upcoming Sunday Service")).toBeInTheDocument();
    expect(screen.getByText("Weekend Services \u00B7 2026-07-19 \u00B7 09:00")).toBeInTheDocument();
    expect(screen.getByText("Planning Center plan loaded")).toBeInTheDocument();
    expect(screen.getByText("Service plan")).toBeInTheDocument();
    expect(screen.getAllByText("Ready").length).toBeGreaterThan(0);
    expect(screen.getByText("Lights MIDI output disconnected")).toBeInTheDocument();
    expect(screen.queryByText("TodayÃ¢â‚¬â„¢s plan loaded")).not.toBeInTheDocument();
    expect(screen.queryByText("TodayÃ¢â‚¬â„¢s service")).not.toBeInTheDocument();
  });

  it("interleaves subdued non-song reference items with their durations", () => {
    renderDashboard({
      ...loadedServiceState,
      skipped_items: [
        {
          item_id: "header-1",
          title: "Welcome",
          description: "This header description is intentionally hidden",
          item_type: "header",
          sequence: 10,
          duration_seconds: 90,
          reason: "header",
        },
        {
          item_id: "item-2",
          title: "Announcements",
          description: "Pastor John",
          item_type: "item",
          sequence: 30,
          duration_seconds: 120,
          reason: "not_song",
        },
      ],
    });

    expect(screen.queryByText("2 non-song items were skipped")).not.toBeInTheDocument();
    expect(screen.queryByText("2 reference items")).not.toBeInTheDocument();
    const rows = within(screen.getByRole("list", { name: "Service plan order" })).getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("Welcome");
    expect(rows[0]).not.toHaveTextContent("01:30");
    expect(rows[0]).not.toHaveTextContent("Reference");
    expect(rows[0]).not.toHaveTextContent("This header description is intentionally hidden");
    expect(screen.getByText("Welcome")).toHaveClass("text-xs", "font-extrabold");
    expect(rows[1]).toHaveTextContent("Holy Forever");
    expect(rows[2]).toHaveTextContent("Announcements");
    expect(rows[2]).toHaveTextContent("Pastor John");
    expect(rows[2]).toHaveTextContent("02:00");
    expect(rows[2]).not.toHaveTextContent("Reference");
    expect(screen.getByText("Welcome")).toHaveClass("text-ink-muted");
    expect(screen.getByText("Pastor John")).toHaveClass("text-idle");
  });
});

describe("Service plan item hover preview", () => {
  it("shows a cursor popover with the item title and description after the hover-intent delay, and hides it on mouse-leave", () => {
    vi.useFakeTimers();
    try {
      renderDashboard({
        ...loadedServiceState,
      }, {
        state: applicationState(loadedServiceState, {
          plan: {
            ...loadedPlan,
            songs: [{ id: "item-1", title: "Holy Forever", duration_seconds: 336, order: 1, service_sequence: 20, is_generic: false, source_song_id: "song-1", description: "Key of G, start soft" }],
          },
        }),
      });

      expect(document.querySelector('[role="tooltip"].fixed')).not.toBeInTheDocument();
      const list = screen.getByRole("list", { name: "Service plan order" });
      const row = within(list).getByText("Holy Forever").closest("li")!;
      fireEvent.mouseEnter(row, { clientX: 100, clientY: 100 });

      // Not warmed up yet: no popover before the 500ms hover-intent delay elapses.
      act(() => vi.advanceTimersByTime(499));
      expect(screen.queryByText("Key of G, start soft")).not.toBeInTheDocument();

      act(() => vi.advanceTimersByTime(1));
      expect(screen.getByText("Key of G, start soft")).toBeInTheDocument();
      const tooltip = document.querySelector('[role="tooltip"].fixed') as HTMLElement;
      expect(tooltip).toHaveTextContent("Holy Forever");

      fireEvent.mouseLeave(row);
      expect(screen.queryByText("Key of G, start soft")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not re-apply the hover-intent delay when moving between adjacent items in the same widget hover session", () => {
    vi.useFakeTimers();
    try {
      renderDashboard({
        ...loadedServiceState,
      }, {
        state: applicationState(loadedServiceState, {
          plan: {
            ...loadedPlan,
            songs: [
              { id: "item-1", title: "Holy Forever", duration_seconds: 336, order: 1, service_sequence: 20, is_generic: false, source_song_id: "song-1", description: "Key of G, start soft" },
              { id: "item-2", title: "Way Maker", duration_seconds: 300, order: 2, service_sequence: 30, is_generic: false, source_song_id: "song-2", description: "Key of E" },
            ],
          },
        }),
      });

      const list = screen.getByRole("list", { name: "Service plan order" });
      const rowA = within(list).getByText("Holy Forever").closest("li")!;
      const rowB = within(list).getByText("Way Maker").closest("li")!;

      fireEvent.mouseEnter(rowA, { clientX: 100, clientY: 100 });
      act(() => vi.advanceTimersByTime(500));
      expect(screen.getByText("Key of G, start soft")).toBeInTheDocument();

      // Move to the adjacent item without leaving the widget: no re-delay.
      fireEvent.mouseLeave(rowA);
      fireEvent.mouseEnter(rowB, { clientX: 100, clientY: 130 });
      expect(screen.getByText("Key of E")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("re-applies the hover-intent delay after leaving the widget entirely and coming back", () => {
    vi.useFakeTimers();
    try {
      renderDashboard({
        ...loadedServiceState,
      }, {
        state: applicationState(loadedServiceState, {
          plan: {
            ...loadedPlan,
            songs: [
              { id: "item-1", title: "Holy Forever", duration_seconds: 336, order: 1, service_sequence: 20, is_generic: false, source_song_id: "song-1", description: "Key of G, start soft" },
              { id: "item-3", title: "Great Are You Lord", duration_seconds: 250, order: 3, service_sequence: 40, is_generic: false, source_song_id: "song-3", description: "Key of C" },
            ],
          },
        }),
      });

      const list = screen.getByRole("list", { name: "Service plan order" });
      const rowA = within(list).getByText("Holy Forever").closest("li")!;
      const rowC = within(list).getByText("Great Are You Lord").closest("li")!;

      fireEvent.mouseEnter(rowA, { clientX: 100, clientY: 100 });
      act(() => vi.advanceTimersByTime(500));
      expect(screen.getByText("Key of G, start soft")).toBeInTheDocument();

      // Leave the widget entirely (no other item hovered).
      fireEvent.mouseLeave(rowA);
      act(() => vi.advanceTimersByTime(0));
      expect(screen.queryByText("Key of G, start soft")).not.toBeInTheDocument();

      // Hovering a different item later requires the delay again.
      fireEvent.mouseEnter(rowC, { clientX: 100, clientY: 200 });
      act(() => vi.advanceTimersByTime(499));
      expect(screen.queryByText("Key of C")).not.toBeInTheDocument();
      act(() => vi.advanceTimersByTime(1));
      expect(screen.getByText("Key of C")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not show a popover for an item without a description", () => {
    vi.useFakeTimers();
    try {
      renderDashboard(loadedServiceState);

      const list = screen.getByRole("list", { name: "Service plan order" });
      const row = within(list).getByText("Holy Forever").closest("li")!;
      fireEvent.mouseEnter(row, { clientX: 100, clientY: 100 });
      act(() => vi.advanceTimersByTime(500));

      expect(document.querySelector('[role="tooltip"].fixed')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows a cursor popover for a reference item with a title heading and description", () => {
    vi.useFakeTimers();
    try {
      renderDashboard({
        ...loadedServiceState,
        skipped_items: [
          {
            item_id: "item-2",
            title: "Announcements",
            description: "Pastor John",
            item_type: "item",
            sequence: 30,
            duration_seconds: 120,
            reason: "not_song",
          },
        ],
      });

      const row = screen.getAllByText("Pastor John")[0]!.closest("li")!;
      fireEvent.mouseEnter(row, { clientX: 100, clientY: 100 });
      act(() => vi.advanceTimersByTime(500));

      expect(screen.getAllByText("Pastor John").length).toBeGreaterThan(1);
      const tooltip = document.querySelector('[role="tooltip"].fixed') as HTMLElement;
      expect(tooltip).toHaveTextContent("Announcements");
    } finally {
      vi.useRealTimers();
    }
  });

  it("clamps the popover so a long note near the bottom of the viewport stays fully visible", () => {
    vi.useFakeTimers();
    const originalInnerHeight = window.innerHeight;
    const originalInnerWidth = window.innerWidth;
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 480 });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 800 });
    try {
      renderDashboard({
        ...loadedServiceState,
      }, {
        state: applicationState(loadedServiceState, {
          plan: {
            ...loadedPlan,
            songs: [{
              id: "item-1",
              title: "Holy Forever",
              duration_seconds: 336,
              order: 1,
              service_sequence: 20,
              is_generic: false,
              source_song_id: "song-1",
              description: Array.from({ length: 13 }, (_, i) => `Line ${i + 1} of a very long note`).join("\n"),
            }],
          },
        }),
      });

      const list = screen.getByRole("list", { name: "Service plan order" });
      const row = within(list).getByText("Holy Forever").closest("li")!;
      // Stub layout globally: jsdom returns 0-sized rects for every element,
      // so pin the popover's measured size to what a 13-line note renders as.
      const height = 220;
      const getBoundingClientRectSpy = vi
        .spyOn(HTMLElement.prototype, "getBoundingClientRect")
        .mockReturnValue({
          width: 288,
          height,
          top: 0,
          left: 0,
          right: 288,
          bottom: height,
          x: 0,
          y: 0,
          toJSON: () => {},
        } as DOMRect);

      try {
        // Hover near the bottom of a short (480px) viewport, as in the reported repro.
        fireEvent.mouseEnter(row, { clientX: 100, clientY: 470 });
        act(() => vi.advanceTimersByTime(500));

        const tooltip = screen.getByText(/Line 1 of a very long note/, { exact: false }).closest('[role="tooltip"]') as HTMLElement;
        const top = parseFloat(tooltip.style.top);
        expect(top).toBeGreaterThanOrEqual(8);
        expect(top + height).toBeLessThanOrEqual(480);
      } finally {
        getBoundingClientRectSpy.mockRestore();
      }
    } finally {
      vi.useRealTimers();
      Object.defineProperty(window, "innerHeight", { configurable: true, value: originalInnerHeight });
      Object.defineProperty(window, "innerWidth", { configurable: true, value: originalInnerWidth });
    }
  });

  it("opens the popover on tap and dismisses it on a second tap (touch equivalent of hover)", () => {
    renderDashboard({
      ...loadedServiceState,
    }, {
      state: applicationState(loadedServiceState, {
        plan: {
          ...loadedPlan,
          songs: [{ id: "item-1", title: "Holy Forever", duration_seconds: 336, order: 1, service_sequence: 20, is_generic: false, source_song_id: "song-1", description: "Key of G, start soft" }],
        },
      }),
    });

    const list = screen.getByRole("list", { name: "Service plan order" });
    const row = within(list).getByText("Holy Forever").closest("li")!;

    fireEvent.touchStart(row, { touches: [{ clientX: 100, clientY: 100 }] });
    fireEvent.touchEnd(row, { changedTouches: [{ clientX: 100, clientY: 100 }] });
    expect(screen.getByText("Key of G, start soft")).toBeInTheDocument();

    fireEvent.touchStart(row, { touches: [{ clientX: 101, clientY: 101 }] });
    fireEvent.touchEnd(row, { changedTouches: [{ clientX: 101, clientY: 101 }] });
    expect(screen.queryByText("Key of G, start soft")).not.toBeInTheDocument();
  });

  it("treats a touch drag (scroll) as not a tap, so the popover does not open", () => {
    renderDashboard({
      ...loadedServiceState,
    }, {
      state: applicationState(loadedServiceState, {
        plan: {
          ...loadedPlan,
          songs: [{ id: "item-1", title: "Holy Forever", duration_seconds: 336, order: 1, service_sequence: 20, is_generic: false, source_song_id: "song-1", description: "Key of G, start soft" }],
        },
      }),
    });

    const list = screen.getByRole("list", { name: "Service plan order" });
    const row = within(list).getByText("Holy Forever").closest("li")!;

    fireEvent.touchStart(row, { touches: [{ clientX: 100, clientY: 100 }] });
    fireEvent.touchEnd(row, { changedTouches: [{ clientX: 100, clientY: 200 }] });

    expect(screen.queryByText("Key of G, start soft")).not.toBeInTheDocument();
  });

  it("dismisses a tap-opened popover when tapping elsewhere on the page", () => {
    renderDashboard({
      ...loadedServiceState,
    }, {
      state: applicationState(loadedServiceState, {
        plan: {
          ...loadedPlan,
          songs: [{ id: "item-1", title: "Holy Forever", duration_seconds: 336, order: 1, service_sequence: 20, is_generic: false, source_song_id: "song-1", description: "Key of G, start soft" }],
        },
      }),
    });

    const list = screen.getByRole("list", { name: "Service plan order" });
    const row = within(list).getByText("Holy Forever").closest("li")!;

    fireEvent.touchStart(row, { touches: [{ clientX: 100, clientY: 100 }] });
    fireEvent.touchEnd(row, { changedTouches: [{ clientX: 100, clientY: 100 }] });
    expect(screen.getByText("Key of G, start soft")).toBeInTheDocument();

    fireEvent.pointerDown(document.body);
    expect(screen.queryByText("Key of G, start soft")).not.toBeInTheDocument();
  });
});

describe("Dashboard widget layout", () => {
  it("locks layout controls by default and persists keyboard movement in edit mode", async () => {
    window.localStorage.removeItem("stagepilot.dashboard-layout.v2");
    const user = userEvent.setup();
    renderDashboard(loadedServiceState);

    expect(screen.queryByRole("button", { name: "Move Service Plan later" })).not.toBeInTheDocument();
    expect(screen.queryByRole("toolbar", { name: "Dashboard layout tools" })).not.toBeInTheDocument();

    const editLayout = screen.getByRole("button", { name: "Edit layout" });
    const dashboardGrid = editLayout.closest("section")?.querySelector(".stagepilot-dashboard-grid");
    expect(dashboardGrid).not.toBeNull();
    expect(
      dashboardGrid!.compareDocumentPosition(editLayout)
      & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    await user.click(editLayout);
    const layoutToolbar = screen.getByRole("toolbar", { name: "Dashboard layout tools" });
    expect(
      dashboardGrid!.compareDocumentPosition(layoutToolbar)
      & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Move Service Plan later" }));

    await waitFor(() => {
      const saved = JSON.parse(window.localStorage.getItem("stagepilot.dashboard-layout.v2")!);
      expect(saved.version).toBe(2);
      expect(saved.desktop).toEqual(expect.arrayContaining([
        expect.objectContaining({ id: "service-plan" }),
        expect.objectContaining({ id: "now-playing" }),
      ]));
    });
    expect(screen.getByRole("button", { name: "Drag Manual Controls to a new dashboard position" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("button", { name: "Drag Manual Controls to a new dashboard position" })).not.toBeInTheDocument();
  });

  it("blocks readiness when a configured Lights output disconnects", () => {
    const configuredLightsSettings: SettingsResponse = {
      ...productionSettings,
      settings: {
        ...productionSettings.settings,
        lights: {
          ...productionSettings.settings.lights,
          enabled: true,
          output_name: "StagePilot to Lightkey",
        },
      },
    };

    renderDashboard(loadedServiceState, {
      settings: configuredLightsSettings,
      state: applicationState(loadedServiceState, {
        lights_status: "disconnected",
        plugins: {},
      }),
    });

    expect(screen.getByRole("button", { name: "Check system" })).toBeInTheDocument();
    expect(screen.getByText("Lights MIDI output disconnected")).toHaveClass("text-ink-muted");
    expect(screen.queryByText("Optional")).not.toBeInTheDocument();
  });

  it("renders manual controls as live buttons in the four tones with 44px targets", () => {
    renderDashboard(loadedServiceState);

    const expected: Record<string, string> = {
      "Start next": "go",
      "Restart current": "go",
      Previous: "back",
      "Reload plan": "reload",
      "Stop timer": "stop",
      "Reset position": "stop",
    };
    for (const [name, tone] of Object.entries(expected)) {
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("data-variant", "live");
      expect(button).toHaveAttribute("data-tone", tone);
      expect(button).toHaveClass("min-h-ds-touch-target");
      // Pressed state comes from tokens; no raw white text on the fill (checked here, never pressed in capture).
      // A disabled live control (no current song) drops hover/press styling by design.
      if (!(button as HTMLButtonElement).disabled) {
        expect(button.className).toContain(tone === "go" || tone === "back" ? "active:text-on-primary" : "active:text-on-danger");
      }
      expect(button.className).not.toMatch(/hover:text-white|slate-|rose-|emerald-/);
    }
  });

  it("adds intentional spacers and resets them without confirmation", async () => {
    window.localStorage.removeItem("stagepilot.dashboard-layout.v2");
    const user = userEvent.setup();
    renderDashboard(loadedServiceState);

    await user.click(screen.getByRole("button", { name: "Edit layout" }));
    await user.click(screen.getByRole("button", { name: "Add spacer" }));
    expect(screen.getByText("Intentional spacer")).toBeInTheDocument();

    const customized = JSON.parse(window.localStorage.getItem("stagepilot.dashboard-layout.v2")!);
    expect(customized.desktop.some((item: { kind: string }) => item.kind === "spacer")).toBe(true);

    await user.click(screen.getByRole("button", { name: "Reset layout" }));
    expect(screen.queryByRole("dialog", { name: "Reset dashboard layout?" })).not.toBeInTheDocument();

    const reset = JSON.parse(window.localStorage.getItem("stagepilot.dashboard-layout.v2")!);
    expect(reset.desktop.some((item: { kind: string }) => item.kind === "spacer")).toBe(false);
  });

  it("switches responsive modes without replacing the saved desktop geometry", async () => {
    const originalWidth = window.innerWidth;
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1200 });
    try {
      renderDashboard(loadedServiceState);
      const dashboard = screen.getByRole("region", { name: "Customizable dashboard" });
      const grid = dashboard.querySelector(".stagepilot-dashboard-grid");
      expect(grid).toHaveAttribute("data-layout-mode", "desktop");
      const desktopBefore = JSON.parse(
        window.localStorage.getItem("stagepilot.dashboard-layout.v2")!,
      ).desktop;

      Object.defineProperty(window, "innerWidth", { configurable: true, value: 500 });
      act(() => window.dispatchEvent(new Event("resize")));
      await waitFor(() => expect(grid).toHaveAttribute("data-layout-mode", "mobile"));

      expect(JSON.parse(
        window.localStorage.getItem("stagepilot.dashboard-layout.v2")!,
      ).desktop).toEqual(desktopBefore);

      Object.defineProperty(window, "innerWidth", { configurable: true, value: 1200 });
      act(() => window.dispatchEvent(new Event("resize")));
      await waitFor(() => expect(grid).toHaveAttribute("data-layout-mode", "desktop"));
    } finally {
      Object.defineProperty(window, "innerWidth", { configurable: true, value: originalWidth });
    }
  });
});

describe("Dashboard connection configuration panels", () => {
  it("keeps the retained first-launch setup UI disabled", () => {
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    expect(screen.queryByLabelText("StagePilot setup progress")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Finish configuring StagePilot" }),
    ).not.toBeInTheDocument();
  });

  it("opens only the clicked connection and toggles it closed on a second click", async () => {
    const user = userEvent.setup();
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    const planningCenter = screen.getByRole("button", { name: /^Planning Center connected/ });
    const midiConnection = screen.getByRole("button", { name: /^MIDI \/ Playback connected/ });

    expect(planningCenter).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("heading", { name: "Planning Center Services" })).not.toBeInTheDocument();

    await user.click(planningCenter);

    expect(planningCenter).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("heading", { name: "Planning Center Services" })).toBeInTheDocument();

    await user.click(midiConnection);

    expect(planningCenter).toHaveAttribute("aria-expanded", "false");
    expect(midiConnection).toHaveAttribute("aria-expanded", "true");
    expect(screen.queryByRole("heading", { name: "Planning Center Services" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Playback connection" })).toBeInTheDocument();

    await user.click(midiConnection);

    expect(midiConnection).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("heading", { name: "Playback connection" })).not.toBeInTheDocument();
  });

  it("keeps all connection cards in one row with persistent icons", () => {
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    const connections = screen.getByRole("region", { name: "Connections" });
    expect(connections).toHaveClass("grid-cols-5");

    for (const icon of ["planning-center", "multitracks", "propresenter", "lights", "stagepilot"]) {
      const image = document.querySelector(`[data-status-icon="${icon}"]`);
      expect(image).toBeInTheDocument();
      expect(image?.parentElement).toHaveClass("h-9", "w-9");
      expect(image?.parentElement?.parentElement).toHaveClass(
        "grid",
        "h-12",
        "w-12",
      );
    }
  });

  it("provides close buttons for all five connection panels", async () => {
    const user = userEvent.setup();
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    const panels = [
      {
        card: /^Planning Center connected/,
        close: "Close Planning Center configuration",
        heading: "Planning Center Services",
      },
      {
        card: /^MIDI \/ Playback connected/,
        close: "Close MIDI / Playback configuration",
        heading: "Playback connection",
      },
      {
        card: /^ProPresenter connected/,
        close: "Close ProPresenter configuration",
        heading: "ProPresenter countdown",
      },
      {
        card: /^Lights disconnected/,
        close: "Close Lights configuration",
        heading: "Lighting configuration",
      },
      {
        card: /^StagePilot backend connected/,
        close: "Close StagePilot backend configuration",
        heading: "StagePilot backend",
      },
    ] as const;

    for (const panel of panels) {
      await user.click(screen.getByRole("button", { name: panel.card }));
      expect(screen.getByRole("heading", { name: panel.heading })).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: panel.close }));
      expect(screen.queryByRole("heading", { name: panel.heading })).not.toBeInTheDocument();
    }
  });

  it("shows a live remaining clock alongside elapsed song duration", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:00:30Z"));
    try {
      renderDashboard(loadedServiceState, {
        state: applicationState(loadedServiceState, {
          current_song: loadedPlan.songs[0] ?? null,
          current_song_index: 0,
          timer: {
            status: "running",
            duration_seconds: 336,
            started_at: "2026-07-13T16:00:00Z",
            last_error: null,
          },
        }),
      });

      expect(screen.getByText("Time remaining")).toBeInTheDocument();
      expect(screen.getByText("05:06")).toBeInTheDocument();
      expect(screen.getByText("Elapsed time")).toBeInTheDocument();
      expect(screen.getByText("00:30")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("advances the main countdown immediately instead of lagging ProPresenter", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:00:00.010Z"));
    try {
      const shortSong = {
        ...loadedPlan.songs[0]!,
        duration_seconds: 263,
      };
      renderDashboard(loadedServiceState, {
        state: applicationState(loadedServiceState, {
          plan: { ...loadedPlan, songs: [shortSong] },
          current_song: shortSong,
          current_song_index: 0,
          timer: {
            status: "running",
            duration_seconds: 263,
            started_at: "2026-07-13T16:00:00.000Z",
            last_error: null,
          },
        }),
      });

      expect(screen.getByText("04:22")).toBeInTheDocument();
      expect(screen.getByText("00:00")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

