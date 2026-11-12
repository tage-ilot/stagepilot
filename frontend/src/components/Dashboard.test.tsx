import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi, afterEach } from "vitest";

import type {
  AccessCapabilities,
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
import * as api from "../api";
import { REMOTE_OFFLINE_GRACE_MS } from "../hooks/useRemoteOffline";
import { LOCAL_CAPABILITIES } from "../access/accessState";
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
    playing_song: null, playing_song_index: null, up_next_song: null,
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

function renderDashboard(serviceLoad: ServiceLoadState, options: DashboardTestOptions = {}) {
  return render(dashboardElement(serviceLoad, options));
}

type DashboardTestOptions = Parameters<typeof dashboardElement>[1];

function dashboardElement(
  serviceLoad: ServiceLoadState,
  {
    actionMessage = null,
    error = null,
    live = true,
    pendingPlanId = null,
    selectPlan = vi.fn(),
    dispatch = vi.fn(),
    state = applicationState(serviceLoad),
    settings = productionSettings,
    updater,
    playback,
    capabilities,
  }: {
    capabilities?: AccessCapabilities;
    actionMessage?: string | null;
    error?: string | null;
    live?: boolean;
    pendingPlanId?: string | null;
    selectPlan?: (planId: string) => void;
    dispatch?: () => void;
    state?: ApplicationState;
    settings?: SettingsResponse | null;
    updater?: UpdaterController;
    playback?: PlaybackController;
  } = {},
) {
  return (
    <Dashboard
      capabilities={capabilities}
      actionMessage={actionMessage}
      dispatch={dispatch}
      error={error}
      health={null}
      live={live}
      midi={midi}
      midiMessages={[]}
      midiError={null}
      midiMessage={null}
      pendingAction={null}
      pendingMidiOperation={null}
      pendingPlanId={pendingPlanId}
      propresenter={propresenter}
      refreshMidi={vi.fn()}
      selectMidi={vi.fn()}
      selectPlan={selectPlan}
      settings={settings}
      state={state}
      updater={updater}
      playback={playback}
    />
  );
}

// A test that fails before its own cleanup (or never restores) must not leave fake timers behind for the next test: under
// --sequence.shuffle that made unrelated tests time out.
afterEach(() => {
  vi.useRealTimers();
});

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

  it("shows the newest connection entry per integration beside the real events", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:30:00Z"));
    const state = applicationState(loadedServiceState, {
      recent_events: [
        // Connected 30 minutes ago and not refreshed since: still the last time it was connected.
        { id: "pp-connected", type: "connection.changed", timestamp: "2026-07-13T16:00:00Z", source: "propresenter" },
        { id: "lights-connected", type: "connection.changed", timestamp: "2026-07-13T16:10:00Z", source: "lights" },
        { id: "song", type: "song_started", timestamp: "2026-07-13T16:29:30Z", source: "propresenter" },
      ],
    });
    renderDashboard(loadedServiceState, { state });

    const rows = [...document.querySelectorAll(".event-row")].map((row) => row.textContent);
    expect(rows).toHaveLength(3);
    expect(rows.filter((row) => row?.includes("connection.changed"))).toHaveLength(2);
    expect(screen.getByText("song_started")).toBeInTheDocument();
  });

  it("never lets connection entries push real events out of the Latest 10", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:05:30Z"));
    const real = Array.from({ length: 10 }, (_, i) => ({
      id: `real-${i}`, type: "song_started", timestamp: "2026-07-13T16:05:00Z", source: "propresenter",
    }));
    const state = applicationState(loadedServiceState, {
      recent_events: [
        ...real,
        { id: "c1", type: "connection.changed", timestamp: "2026-07-13T16:05:10Z", source: "propresenter" },
        { id: "c2", type: "connection.changed", timestamp: "2026-07-13T16:05:20Z", source: "lights" },
      ],
    });
    renderDashboard(loadedServiceState, { state });

    expect(screen.getAllByText("song_started")).toHaveLength(10);
    expect(screen.getAllByText("connection.changed")).toHaveLength(2);
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
      "-translate-y-[18px]", // 2px higher than the old -translate-y-4 (owner, 2026-10-14)
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

    // The message is announced through a live region and drawn in the status slot, which sits in the header.
    const notification = screen.getByRole("status");
    const header = screen.getByRole("banner");
    expect(header).toContainElement(notification);
    expect(notification).toHaveTextContent("Service position and timer reset.");
    // The status pill itself turns into the message: same element, info colours, the message's words.
    const chip = within(header).getByTestId("header-chip");
    expect(chip).toHaveAttribute("data-state", "message");
    // A dark pill like the widgets; the Previous-button blue is carried by the text and the border.
    expect(chip).toHaveClass("bg-surface-950", "text-live-back-text", "border-live-back-soft", "font-pixel");
    expect(within(header).getByTestId("header-message")).toHaveTextContent("Service position and timer reset.");
  });

  it("keeps the same reserved header slot when no notification is visible", () => {
    renderDashboard(loadedServiceState);

    const header = screen.getByRole("banner");
    const chip = within(header).getByTestId("header-chip");
    // Ready: the pill rests green, in the pixel font, and it is the same pill that carries messages.
    expect(chip).toHaveAttribute("data-state", "status");
    expect(chip).toHaveClass("bg-surface-950", "border-success-soft", "text-success-text", "font-pixel");
    expect(header.querySelector('[role="status"]')).toHaveTextContent("");
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
        "Looking for the current or next upcoming Planning Center plan. The last successful service plan is still displayed as stale.",
      );

      act(() => vi.advanceTimersByTime(6_000));
      expect(screen.getByTestId("header-chip")).toHaveAttribute("data-state", "status");
      expect(notification).toHaveTextContent("");
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
      expect(screen.getByTestId("header-chip")).toHaveAttribute("data-state", "status");
      expect(notification).toHaveTextContent("");
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });

  it("shows errors in the same header slot with error styling", () => {
    renderDashboard(loadedServiceState, { error: "Backend unavailable." });

    expect(screen.getByRole("status")).toHaveTextContent("Backend unavailable.");
    expect(screen.getByTestId("header-message")).toHaveAttribute("data-tone", "error");
    const chip = screen.getByTestId("header-chip");
    expect(chip).toHaveAttribute("data-state", "message");
    expect(chip).toHaveClass("bg-surface-950", "border-danger-edge", "text-danger-pill");
  });

  describe("header layout (owner, 2026-10-15)", () => {
    const parts = () => {
      const header = screen.getByRole("banner");
      return {
        header,
        logo: within(header).getByRole("heading", { name: "StagePilot" }),
        brand: header.querySelector(".app-header-brand")!,
        slot: header.querySelector(".app-header-status")!,
        chip: header.querySelector(".header-chip-wrap")!,
      };
    };

    it("pins the logo 32px inside the background image at every width, including windows wider than the image (stylesheet rule)", async () => {
      renderDashboard(loadedServiceState);
      expect(parts().logo).toHaveClass("app-logo");
      const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
      const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
      // 32px in from the left edge of the 1700px background image: it cancels the page padding and the dashboard's centring (1680px),
      // then adds the image's own centring offset (1700px), so on a wide window it follows the image instead of the window edge.
      expect(css).toMatch(/\.app-logo \{\s*margin-left: calc\(2rem - var\(--page-pad\) - max\(0px, \(var\(--page-w\) - 1680px\) \/ 2\) \+ max\(0px, \(var\(--page-w\) - 1700px\) \/ 2\)\);/);
      expect(css).toMatch(/\.app-header \{\s*--page-pad: 1rem;/);
      expect(css).toMatch(/@media \(min-width: 1024px\) \{\s*\.app-header \{\s*--page-pad: 2rem;/);
    });

    it("is a single row with the logo first, on every width: no grid that re-flows or wraps the logo", () => {
      renderDashboard(loadedServiceState);
      const { header, brand, slot, logo } = parts();
      expect(header).toHaveClass("flex", "items-center");
      expect(header.className).not.toMatch(/\bgrid\b|grid-cols|flex-wrap|sm:col-|row-start/);
      expect(header.firstElementChild).toBe(brand);
      expect(brand.contains(logo)).toBe(true);
      expect(brand).toHaveClass("min-w-0"); // the Update button may wrap inside the brand; the logo itself never moves
      expect(slot).toHaveClass("flex-1");
      expect(header.lastElementChild).toBe(slot);
    });

    it("keeps the pill, the message and a screen-reader copy in the header's own slot", () => {
      renderDashboard(loadedServiceState, { actionMessage: "Service position and timer reset." });
      const { slot, chip } = parts();
      expect(slot.contains(chip)).toBe(true);
      expect(slot.contains(screen.getByTestId("header-chip"))).toBe(true);
      expect(slot.querySelector('[role="status"]')).toHaveClass("sr-only");
    });

    it("is green when Ready and red when not; blue for an info message, red for an error message; no dot anywhere", () => {
      const ready = renderDashboard(loadedServiceState);
      expect(screen.getByTestId("header-chip")).toHaveClass("bg-surface-950", "text-success-text");
      expect(screen.getByTestId("header-chip").querySelector(".header-chip-dot")).toBeNull();
      ready.unmount();
      renderDashboard({ ...loadedServiceState, is_stale: true });
      expect(screen.getByTestId("header-chip")).toHaveClass("bg-surface-950", "text-danger-pill");
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Check system");
    });

    it("a status longer than its slot is cut with an ellipsis instead of vanishing; only a slot narrower than \"Ready\" hides it (F-H7)", async () => {
      const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
      const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
      expect(css).toMatch(/\.header-chip-status \{[^}]*overflow: hidden;[^}]*text-overflow: ellipsis;/);

      // jsdom has no layout: give the slot a width and the hidden measuring copy a (long) text width.
      let slotWidth = 120;
      const client = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(function (this: HTMLElement) {
        return this.classList.contains("app-header-status") ? slotWidth : 900;
      });
      const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
        const width = this.classList.contains("header-chip-probe-status") ? 380 : 0;
        return { x: 0, y: 0, top: 0, left: 0, right: width, bottom: 20, width, height: 20, toJSON: () => ({}) } as DOMRect;
      });
      try {
        const { unmount } = renderDashboard(loadedServiceState, { state: applicationState(loadedServiceState, { plugins: {} }), settings: { ...productionSettings, settings_degraded: true } });
        const chip = screen.getByTestId("header-chip");
        const wrap = chip.closest(".header-chip-wrap");
        // 380px of words in a 120px slot: still drawn, exactly the slot wide (the stylesheet adds the "...").
        await waitFor(() => expect(chip).toHaveStyle({ width: "120px" }));
        expect(wrap).toHaveAttribute("data-fits", "true");
        // A slot narrower than the smallest pill ("Ready", 66px): it steps aside as before.
        slotWidth = 50;
        unmount();
        renderDashboard(loadedServiceState, { state: applicationState(loadedServiceState, { plugins: {} }), settings: { ...productionSettings, settings_degraded: true } });
        await waitFor(() => expect(screen.getByTestId("header-chip").closest(".header-chip-wrap")).toHaveAttribute("data-fits", "false"));
      } finally {
        client.mockRestore();
        rect.mockRestore();
      }
    });

    it("steps aside, instead of shrinking to a dot, when the slot is too small; a message still shows (stylesheet rules)", async () => {
      const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
      const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
      // Only the STATUS pill steps aside; while a message is showing (data-notice) the pill stays so the message is never lost.
      expect(css).toMatch(/\.header-chip-wrap\[data-fits="false"\]\[data-notice="false"\] \{\s*display: none;/);
      expect(css).not.toMatch(/header-chip-dot/);
      // The pill animates its size, and the words slide in when one message replaces another.
      expect(css).toMatch(/\.header-chip \{[^}]*transition:\s*width 360ms/);
      // The words wait for the pill to finish resizing (330ms) before they appear, so they never wrap mid-change.
      expect(css).toMatch(/\.header-chip-note-text \{\s*display: block;[\s\S]*?animation: header-note-in 260ms[^;]* 330ms both;/);
      expect(css).toMatch(/\.header-chip\[data-state="message"\] \.header-chip-note \{[^}]*transition-delay: 330ms/);
      expect(css).toMatch(/\.header-chip-note \{[^}]*white-space: nowrap;/);
    });

    it("keeps a notification to one line that ends in an ellipsis, and never wraps it (stylesheet rules, owner 2026-10-16)", async () => {
      const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
      const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
      expect(css).toMatch(/\.header-chip-note-text \{[^}]*overflow: hidden;[^}]*text-overflow: ellipsis;[^}]*white-space: nowrap;/);
      // The old wrap-to-two-lines state and the under-the-header fallback are gone.
      expect(css).not.toMatch(/data-wrap/);
      expect(css).not.toMatch(/\.header-message/);
      expect(css).not.toMatch(/\.header-chip-probe-note \{[^}]*white-space: normal;/);
    });

    it("shows a long message on one line in the chip, growing no wider than the header row", () => {
      renderDashboard(loadedServiceState, { error: "x ".repeat(120) });
      const chip = screen.getByTestId("header-chip");
      expect(chip).toHaveAttribute("data-state", "message");
      expect(chip).not.toHaveAttribute("data-wrap");
      expect(screen.queryByTestId("header-message-float")).toBeNull();
    });
  });

  it("renders ambiguous candidates and sends the selected plan ID", async () => {
    const selectPlan = vi.fn();
    const user = userEvent.setup();
    renderDashboard(ambiguousServiceState, { selectPlan });

    expect(screen.getByText("Service plan selection required")).toBeInTheDocument();
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

  it("tells the operator when Remote Access is on but has stayed unreachable, and stays Ready otherwise", async () => {
    vi.useFakeTimers();
    let get: ReturnType<typeof vi.spyOn> | undefined;
    try {
      const remote = (state: api.RemoteStatus["state"], enabled = true): api.RemoteStatus => ({
        available: true, provisioned: true, credential_available: true, enabled, state, url: null,
        needs_operator: false, message: null, temporary_url: false, permanently_revoked: false,
      });
      get = vi.spyOn(api, "getRemoteStatus").mockResolvedValue(remote("reconnecting"));
      renderDashboard(loadedServiceState, { state: applicationState(loadedServiceState, { plugins: {} }) });
      // Reconnecting is normal for a while: still "Ready".
      await act(async () => { await vi.advanceTimersByTimeAsync(REMOTE_OFFLINE_GRACE_MS / 2); });
      expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      // Through the retries and still down: the pill says so, and the list names it (as an error row, no "Optional" tag).
      await act(async () => { await vi.advanceTimersByTimeAsync(REMOTE_OFFLINE_GRACE_MS); });
      const pill = screen.getByRole("button", { name: /^Remote access offline$/ });
      expect(pill).toHaveClass("border-danger-edge");
      expect(screen.queryByRole("button", { name: /^Ready$/ })).not.toBeInTheDocument();
      expect(screen.getByRole("tooltip", { hidden: true })).toHaveTextContent("Remote access offline");
      expect(screen.getByRole("tooltip", { hidden: true })).not.toHaveTextContent(/optional/i);
      // It comes back: back to Ready.
      get.mockResolvedValue(remote("connected"));
      await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
      expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      // Turned off by the user: never mentioned.
      get.mockResolvedValue(remote("off", false));
      await act(async () => { await vi.advanceTimersByTimeAsync(REMOTE_OFFLINE_GRACE_MS * 2); });
      expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      expect(screen.queryByText("Remote access offline")).not.toBeInTheDocument();
    } finally {
      get?.mockRestore();
      vi.useRealTimers();
    }
  });

  it("does not require the demo integration in production mode", async () => {
    const user = userEvent.setup();
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    expect(screen.getByRole("heading", { name: "StagePilot" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Playback" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback Connected/ }));
    expect(screen.getByRole("heading", { name: "Playback" })).toBeInTheDocument();
    expect(screen.getAllByText("Connected to Playback").length).toBeGreaterThan(0);
    expect(screen.queryByText("Demo integration running")).not.toBeInTheDocument();
    expect(screen.getAllByText("Ready").length).toBeGreaterThan(0);
    // Owner 2026-10-13: the Ready pill is the deep green with light text and NO status dot.
    const ready = screen.getByRole("button", { name: /^Ready$/ });
    expect(ready).toHaveClass("bg-surface-950", "text-success-text", "border-success-soft");
    expect(ready.querySelector("[data-size]")).toBeNull();
    // ...and none of the five status cards has a dot either.
    expect(document.querySelectorAll(".connections-status [data-size], .connections-status .status-card-dot")).toHaveLength(0);
    expect(screen.queryByText(/^Lighting (MIDI|OSC)/)).not.toBeInTheDocument();
  });

  it("keeps MIDI setup closed until its connection card is clicked", async () => {
    const user = userEvent.setup();
    renderDashboard(loadedServiceState);

    expect(screen.queryByRole("heading", { name: "Playback" })).not.toBeInTheDocument();
    expect(screen.getByText("Connected to Playback")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback Connected/ }));

    expect(screen.getByRole("heading", { name: "Playback" })).toBeInTheDocument();
  });

  describe("leaving a setup panel with held changes (any panel, not only Playback)", () => {
    const cases: Array<{ card: RegExp; close: string; field: string; value: string }> = [
      { card: /^Presentation/, close: "Close Presentation configuration", field: "Host", value: "192.0.2.10" },
      { card: /^Lighting/, close: "Close Lighting configuration", field: "Note length (ms)", value: "250" },
      { card: /^StagePilot/, close: "Close StagePilot configuration", field: "Log level", value: "DEBUG" },
    ];
    it.each(cases)("asks before discarding held $field changes; clean panels close without asking", async ({ card, close, field, value }) => {
      const user = userEvent.setup();
      renderDashboard(loadedServiceState, { settings: productionSettings });
      await user.click(screen.getByRole("button", { name: card }));
      await user.click(screen.getByRole("button", { name: close }));
      expect(screen.queryByRole("dialog", { name: "Discard unsaved changes?" })).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: card }));
      fireEvent.change(screen.getByLabelText(field), { target: { value } });
      await user.click(screen.getByRole("button", { name: close }));
      expect(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).toBeVisible();
      await user.click(screen.getByRole("button", { name: "Keep editing" }));
      expect((screen.getByLabelText(field) as HTMLInputElement).value).toBe(value);
      await user.click(screen.getByRole("button", { name: close }));
      await user.click(within(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).getByRole("button", { name: "Discard" }));
      // The panel unmounts when its 500 ms close fallback fires; allow for a loaded runner instead of the 1 s default.
      await waitFor(() => expect(screen.queryByLabelText(field)).not.toBeInTheDocument(), { timeout: 5_000 });
    });
  });

  describe("leaving Playback with unsaved changes", () => {
    const apiSettings = { ...productionSettings, settings: { ...productionSettings.settings, integration_modes: { ...productionSettings.settings.integration_modes, midi_source: "playback_api" as const } } };
    const playback = (): PlaybackController => ({
      status: playbackStatus(), events: [], error: null, message: null, pending: null, saveArrangement: vi.fn(async () => true),
      save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(),
    });
    const openPlayback = async () => {
      const user = userEvent.setup();
      renderDashboard(loadedServiceState, { playback: playback(), settings: apiSettings });
      await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback/ }));
      return user;
    };

    it("asks 'Discard unsaved changes?' when closing a dirty panel; Keep editing stays, Discard closes", async () => {
      const user = await openPlayback();
      await user.click(screen.getByRole("radio", { name: /^MIDI/ }));
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      expect(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).toBeVisible();
      await user.click(screen.getByRole("button", { name: "Keep editing" }));
      expect(screen.getByRole("heading", { name: "Playback" })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      await user.click(within(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).getByRole("button", { name: "Discard" }));
      expect(screen.queryByRole("heading", { name: "Playback" })).not.toBeInTheDocument();
    });

    it("discard dialog focuses Discard, ignores Escape and backdrop clicks, and is a dialog with the same name", async () => {
      const user = await openPlayback();
      await user.click(screen.getByRole("radio", { name: /^MIDI/ }));
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      const dialog = screen.getByRole("dialog", { name: "Discard unsaved changes?" });
      await waitFor(() => expect(within(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).getByRole("button", { name: "Discard" })).toHaveFocus());
      await user.keyboard("{Escape}");
      await user.click(dialog.parentElement!);
      expect(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).toBeVisible();
    });

    it("focuses Discard once on open; Dashboard re-renders at 250 ms and 5 s do not pull focus back from Keep editing", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      await openPlayback();
      await user.click(screen.getByRole("radio", { name: /^MIDI/ }));
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      await waitFor(() => expect(within(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).getByRole("button", { name: "Discard" })).toHaveFocus());
      await user.tab({ shift: true });
      expect(screen.getByRole("button", { name: "Keep editing" })).toHaveFocus();
      for (const ms of [300, 600, 5_400, 5_400]) {
        await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
        expect(screen.getByRole("button", { name: "Keep editing" })).toHaveFocus();
      }
    });

    it("also asks when switching to another tab, and never asks when nothing changed", async () => {
      const user = await openPlayback();
      await user.click(screen.getByRole("button", { name: "Close MIDI / Playback configuration" }));
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: /^MIDI \/ Playback/ }));
      await user.click(screen.getByRole("radio", { name: /^MIDI/ }));
      await user.click(screen.getByRole("button", { name: /^Presentation/ }));
      expect(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).toBeVisible();
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

    expect(screen.getByText("Services disconnected")).toBeInTheDocument();
    expect(screen.getByText("Service plan not loaded")).toBeInTheDocument();
    expect(screen.getByText("Song durations invalid")).toBeInTheDocument();
    // No saved settings here, so Playback reads as the MIDI method; the entry names whichever method is saved.
    expect(screen.getByText("Playback MIDI disconnected")).toBeInTheDocument();
    expect(screen.getByText("Presentation disconnected")).toBeInTheDocument();
  });

  it("shows when the service plan was last successfully loaded", () => {
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, {
        last_successful_plan_reload_at: "2026-06-15T18:28:00",
      }),
    });

    expect(screen.getByText("Current 18:28 06-15-2026")).toBeInTheDocument();
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
    expect(screen.getByText("Service plan loaded")).toBeInTheDocument();
    expect(screen.getByText("Service plan")).toBeInTheDocument();
    expect(screen.getAllByText("Ready").length).toBeGreaterThan(0);
    expect(screen.queryByText(/^Lighting (MIDI|OSC)/)).not.toBeInTheDocument();
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

    expect(screen.queryByRole("button", { name: "Move Service plan later" })).not.toBeInTheDocument();
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
    await user.click(screen.getByRole("button", { name: "Move Service plan later" }));

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

  describe("Scan Playback again in the header status pill (F-S1)", () => {
    // The plan is on 2026-07-13 at 09:00 in Los Angeles (settings.timezone), which is 16:00 UTC.
    const apiSettings: SettingsResponse = { ...productionSettings, settings: { ...productionSettings.settings, integration_modes: { ...productionSettings.settings.integration_modes, midi_source: "playback_api" } } };
    const controller = (overrides: Parameters<typeof playbackStatus>[0] = {}): PlaybackController => ({
      status: playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: "2026-07-12T20:00:00Z", ...overrides }), events: [], error: null, message: null, pending: null,
      saveArrangement: vi.fn(async () => true), save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(),
    });
    const readyState = () => applicationState(loadedServiceState, { plugins: {} });
    const rows = () => within(screen.getByRole("tooltip", { hidden: true })).getAllByRole("listitem");
    const rowFor = (label: string) => rows().find((row) => row.textContent?.includes(label));
    const at = (iso: string) => { vi.useFakeTimers(); vi.setSystemTime(new Date(iso)); };
    const SENTENCE = "The service starts in 30 minutes and Playback has not been scanned in the hour before it. Scan it so song order and lengths are right.";

    it("30 minutes before the service: the pill reads the label in the alarm colours and the list has an error row", () => {
      at("2026-07-13T15:30:00Z");
      renderDashboard(loadedServiceState, { state: readyState(), settings: apiSettings, playback: controller() });
      const pill = screen.getByRole("button", { name: /^Scan Playback again$/ });
      expect(pill).toBe(screen.getByTestId("header-chip"));
      expect(pill).toHaveClass("border-danger-edge", "text-danger-pill");
      expect(pill).toHaveAttribute("title", SENTENCE);
      expect(pill.querySelector(".header-chip-status")?.textContent).toBe("Scan Playback again");
      const row = rowFor("Scan Playback again");
      expect(row).toHaveTextContent("!");
      expect(row).not.toHaveTextContent(/optional/i);
      expect(row).toHaveAttribute("title", SENTENCE);
      expect(screen.getByRole("tooltip", { hidden: true })).not.toHaveTextContent(/optional/i);
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /Got it|Dismiss/ })).not.toBeInTheDocument();
    });

    it("an hour or more before the service, or already scanned in the hour, the pill stays Ready and the row is not there", () => {
      at("2026-07-13T14:59:00Z");
      const { unmount } = renderDashboard(loadedServiceState, { state: readyState(), settings: apiSettings, playback: controller() });
      expect(screen.getByRole("button", { name: /^Ready$/ })).toHaveClass("border-success-soft");
      expect(rowFor("Scan Playback again")).toBeUndefined();
      unmount();
      vi.setSystemTime(new Date("2026-07-13T15:30:00Z"));
      renderDashboard(loadedServiceState, { state: readyState(), settings: apiSettings, playback: controller({ captured_at: "2026-07-13T15:10:00Z" }) });
      expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      expect(rowFor("Scan Playback again")).toBeUndefined();
    });

    it("goes away by itself when a scan completes, and when the service start passes", () => {
      at("2026-07-13T15:30:00Z");
      const options = { state: readyState(), settings: apiSettings };
      const { rerender } = renderDashboard(loadedServiceState, { ...options, playback: controller() });
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again");
      // Playback is polled: the next answer carries the new scan time.
      rerender(dashboardElement(loadedServiceState, { ...options, playback: controller({ captured_at: "2026-07-13T15:31:00Z" }) }));
      expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      expect(rowFor("Scan Playback again")).toBeUndefined();
      // Not scanned, but the clock reaches the start: the dashboard's own tick takes the row away.
      rerender(dashboardElement(loadedServiceState, { ...options, playback: controller() }));
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again");
      act(() => { vi.advanceTimersByTime(31 * 60_000); });
      expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      expect(rowFor("Scan Playback again")).toBeUndefined();
    });

    it("counts the minutes down with the clock, without a reload", () => {
      at("2026-07-13T15:30:00Z");
      renderDashboard(loadedServiceState, { state: readyState(), settings: apiSettings, playback: controller() });
      expect(screen.getByTestId("header-chip")).toHaveAttribute("title", expect.stringContaining("starts in 30 minutes"));
      act(() => { vi.advanceTimersByTime(10 * 60_000); });
      expect(screen.getByTestId("header-chip")).toHaveAttribute("title", expect.stringContaining("starts in 20 minutes"));
    });

    it("sits with another problem: both rows are listed and the reminder leads", () => {
      at("2026-07-13T15:30:00Z");
      renderDashboard(loadedServiceState, {
        state: readyState(),
        settings: { ...apiSettings, settings_set_aside: [{ song_key: "k1", song_title: "Ever Be" }] },
        playback: controller(),
      });
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again");
      expect(rowFor("Scan Playback again")).toBeDefined();
      expect(rowFor("Record Ever Be again")).toBeDefined();
    });

    it("is not shown while a song plays", () => {
      at("2026-07-13T15:30:00Z");
      renderDashboard(loadedServiceState, { state: readyState(), settings: apiSettings, playback: controller({ playing: true }) });
      expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
    });

    describe("quiet while the loaded plan is going (F-S2)", () => {
      // Services at 09:00 (16:00Z) and 10:00 (17:00Z) with two 5-minute songs; the second service's window opens at 16:00Z.
      const songs = [0, 1].map((n) => ({ ...loadedPlan.songs[0]!, id: `item-${n + 1}`, title: `Song ${n + 1}`, duration_seconds: 300, order: n + 1 }));
      const plan: ServicePlan = { ...loadedPlan, songs, service_times: ["09:00", "10:00"] };
      const stopped = { status: "stopped" as const, duration_seconds: 300, started_at: null, position_seconds: 300, remaining_seconds: 0, last_error: null };
      const withTimer = (overrides: Partial<ApplicationState>) => applicationState(loadedServiceState, { plan, plugins: {}, ...overrides });
      const pillText = () => screen.getByTestId("header-chip").textContent;

      it("stays Ready through the first service, between songs and during the last song, and comes back (with the minutes) after the last song, until a scan", () => {
        at("2026-07-13T15:30:00Z");
        const options = { settings: apiSettings };
        const { rerender } = renderDashboard(loadedServiceState, { ...options, state: withTimer({}), playback: controller() });
        expect(pillText()).toContain("Scan Playback again"); // 30 minutes before the 09:00 service

        // 09:06: song 1 has ended, song 2 has not started (between songs).
        act(() => { vi.setSystemTime(new Date("2026-07-13T16:06:00Z")); vi.advanceTimersByTime(6_000); });
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller(), state: withTimer({ playing_song_index: 0, playing_song: songs[0]!, current_song_index: 0, current_song: songs[0]!, up_next_song: songs[1]!, timer: stopped }) }));
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
        expect(rowFor("Scan Playback again")).toBeUndefined();

        // 09:12: the last song runs (it was started at 09:12).
        const running = { status: "running" as const, duration_seconds: 300, started_at: "2026-07-13T16:12:00Z", position_seconds: 0, last_error: null };
        act(() => { vi.setSystemTime(new Date("2026-07-13T16:12:00Z")); vi.advanceTimersByTime(1_000); });
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller(), state: withTimer({ playing_song_index: 1, playing_song: songs[1]!, current_song_index: 1, current_song: songs[1]!, up_next_song: null, timer: running }) }));
        act(() => { vi.advanceTimersByTime(4 * 60_000); });
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();

        // 09:18: its countdown has reached 0:00 and stopped. The plan is over; the 10:00 service is 42 minutes away and Playback was last scanned yesterday.
        act(() => { vi.advanceTimersByTime(2 * 60_000); });
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller(), state: withTimer({ playing_song_index: 1, playing_song: songs[1]!, current_song_index: 1, current_song: songs[1]!, up_next_song: null, timer: stopped }) }));
        expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again");
        expect(screen.getByTestId("header-chip")).toHaveAttribute("title", expect.stringContaining("starts in 42 minutes"));
        expect(rowFor("Scan Playback again")).toBeDefined();

        // A scan completes: gone. Later the 10:00 service starts: still gone.
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller({ captured_at: "2026-07-13T16:20:00Z" }), state: withTimer({ playing_song_index: 1, playing_song: songs[1]!, current_song_index: 1, current_song: songs[1]!, up_next_song: null, timer: stopped }) }));
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      });

      it("the last song counts as ended when its countdown reaches 0:00 while the page is open, without a reload", () => {
        at("2026-07-13T16:12:00Z");
        const running = { status: "running" as const, duration_seconds: 300, started_at: "2026-07-13T16:12:00Z", position_seconds: 0, last_error: null };
        renderDashboard(loadedServiceState, { settings: apiSettings, playback: controller(), state: withTimer({ playing_song_index: 1, playing_song: songs[1]!, current_song_index: 1, current_song: songs[1]!, up_next_song: null, timer: running }) });
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
        act(() => { vi.advanceTimersByTime(4 * 60_000 + 50_000); });
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument(); // 09:16:50: 10 seconds left
        act(() => { vi.advanceTimersByTime(20_000); });
        expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again"); // over, though StagePilot's timer never stopped
      });

      it("one service (Hunter's church): quiet from the start, and after the last song nothing is ahead so it never returns", () => {
        at("2026-07-13T15:30:00Z");
        const single: ServicePlan = { ...plan, service_times: ["09:00"] };
        const options = { settings: apiSettings };
        const { rerender } = renderDashboard(loadedServiceState, { ...options, playback: controller(), state: applicationState(loadedServiceState, { plan: single, plugins: {} }) });
        expect(pillText()).toContain("Scan Playback again");
        act(() => { vi.setSystemTime(new Date("2026-07-13T16:00:00Z")); vi.advanceTimersByTime(5_000); });
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument(); // the service has started
        act(() => { vi.advanceTimersByTime(40 * 60_000); });
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument(); // 09:40, 30 minutes after the estimate; nothing ahead
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller(), state: applicationState(loadedServiceState, { plan: single, plugins: {}, playing_song_index: 1, timer: stopped }) }));
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      });
    });

    describe("a sound check before the service does not hide the reminder (F-S3)", () => {
      // One service at 09:00 (16:00Z), two 5-minute songs. The team plays song 1 as a sound check at 08:20 (15:20Z), 40 minutes before.
      const songs = [0, 1].map((n) => ({ ...loadedPlan.songs[0]!, id: `item-${n + 1}`, title: `Song ${n + 1}`, duration_seconds: 300, order: n + 1 }));
      const plan: ServicePlan = { ...loadedPlan, songs, service_times: ["09:00"] };
      const timerState = (timer: ApplicationState["timer"]) => applicationState(loadedServiceState, { plan, plugins: {}, playing_song_index: 0, playing_song: songs[0]!, current_song_index: 0, current_song: songs[0]!, up_next_song: songs[1]!, timer });
      const running = { status: "running" as const, duration_seconds: 300, started_at: "2026-07-13T15:20:00Z", position_seconds: 0, last_error: null };
      const stopped = { status: "stopped" as const, duration_seconds: 300, started_at: null, position_seconds: 300, remaining_seconds: 0, last_error: null };

      it("is Ready while the sound check plays and for 5 minutes after, shows the reminder at 6 minutes (the last song was never played), and is gone at the service start", () => {
        at("2026-07-13T15:20:05Z");
        const options = { settings: apiSettings };
        const { rerender } = renderDashboard(loadedServiceState, { ...options, playback: controller(), state: timerState(running) });
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
        act(() => { vi.advanceTimersByTime(4 * 60_000 + 50_000); }); // 08:24:55, the song is nearly over
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
        act(() => { vi.advanceTimersByTime(10_000); }); // 08:25:05, the song ended and its timer stopped
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller(), state: timerState(stopped) }));
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
        act(() => { vi.advanceTimersByTime(3 * 60_000); }); // 3 minutes after
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
        act(() => { vi.advanceTimersByTime(3 * 60_000); }); // 08:31, 6 minutes after the song
        expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again");
        expect(screen.getByTestId("header-chip")).toHaveAttribute("title", expect.stringContaining("starts in 29 minutes"));
        expect(rowFor("Scan Playback again")).toBeDefined();
        act(() => { vi.advanceTimersByTime(28 * 60_000); }); // 08:59, still there
        expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again");
        act(() => { vi.advanceTimersByTime(90_000); }); // the service has started
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      });

      it("a scan after the sound check clears the reminder", () => {
        at("2026-07-13T15:20:05Z");
        const options = { settings: apiSettings };
        const { rerender } = renderDashboard(loadedServiceState, { ...options, playback: controller(), state: timerState(running) });
        act(() => { vi.advanceTimersByTime(10 * 60_000); });
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller(), state: timerState(stopped) }));
        expect(screen.getByTestId("header-chip")).toHaveTextContent("Scan Playback again");
        rerender(dashboardElement(loadedServiceState, { ...options, playback: controller({ captured_at: "2026-07-13T15:31:00Z" }), state: timerState(stopped) }));
        expect(screen.getByRole("button", { name: /^Ready$/ })).toBeInTheDocument();
      });
    });

    it("the read-only Live status view shows nothing new", () => {
      at("2026-07-13T15:30:00Z");
      renderDashboard(loadedServiceState, {
        state: readyState(), settings: apiSettings, playback: controller(),
        capabilities: { ...LOCAL_CAPABILITIES, canConfigure: false },
      });
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Live status");
      expect(rows().some((row) => /Scan Playback again/.test(row.textContent ?? ""))).toBe(false);
    });
  });

  describe("settings problems in the header status pill (F-H7)", () => {
    const readyState = () => applicationState(loadedServiceState, { plugins: {} });
    const damaged = (...songs: Array<[string, string]>): SettingsResponse => ({
      ...productionSettings,
      settings_set_aside: songs.map(([song_key, song_title]) => ({ song_key, song_title })),
      settings_notice: songs.map(([, title]) => `The lighting data for ${title} was damaged and set aside; record it again.`).join(" "),
    });
    const rows = () => within(screen.getByRole("tooltip", { hidden: true })).getAllByRole("listitem");
    const rowFor = (label: string) => rows().find((row) => row.textContent?.includes(label));
    // The remote-access test shows how Ready is reached: a plan, no demo plugins and a healthy backend.
    const readyBase = (settings: SettingsResponse) => ({ state: readyState(), settings });

    it("shows nothing about settings while nothing is wrong, and no banner is ever drawn", () => {
      renderDashboard(loadedServiceState, readyBase({ ...productionSettings, settings_degraded: false, settings_set_aside: [], settings_notice: null }));
      expect(rows().some((row) => /settings|Record /.test(row.textContent ?? ""))).toBe(false);
      expect(screen.queryByTestId("settings-notices-main")).not.toBeInTheDocument();
    });

    it("unreadable settings: the pill is the error, the list row has the ! mark and the sentence", () => {
      renderDashboard(loadedServiceState, readyBase({ ...productionSettings, settings_degraded: true, settings_degraded_reason: "not valid JSON" }));
      const pill = screen.getByTestId("header-chip");
      expect(screen.getByRole("button", { name: /^Error: Saved settings could not be read$/ })).toBe(pill);
      expect(pill).toHaveClass("border-danger-edge", "text-danger-pill");
      expect(pill).toHaveAttribute("title", expect.stringContaining("It started with the default settings, so Planning Center, Lighting and the other saved settings are not loaded."));
      const row = rowFor("Saved settings could not be read");
      expect(row).toBeDefined();
      expect(row).toHaveTextContent("!");
      expect(row).not.toHaveTextContent("Optional");
      expect(row).toHaveAttribute("title", expect.stringContaining("default settings"));
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /Got it|Dismiss/ })).not.toBeInTheDocument();
    });

    it("a settings file from a newer StagePilot has its own words", () => {
      renderDashboard(loadedServiceState, readyBase({ ...productionSettings, settings_degraded: true, settings_degraded_reason: "schema_version 3 is newer than 2" }));
      expect(screen.getByRole("button", { name: /^Error: Settings are from a newer StagePilot$/ })).toHaveClass("border-danger-edge");
      expect(rowFor("Settings are from a newer StagePilot")).toHaveAttribute("title", expect.stringContaining("Open the newer StagePilot"));
    });

    it("one set-aside song: the ready pill shows the warning words in the alarm colours, the row is a plain error row", () => {
      renderDashboard(loadedServiceState, readyBase(damaged(["k1", "Ever Be"])));
      const pill = screen.getByRole("button", { name: /^Record Ever Be again$/ });
      expect(pill).toHaveClass("border-danger-edge", "text-danger-pill");
      expect(pill).toHaveAttribute("title", "The lighting data for Ever Be was damaged and set aside; record it again.");
      const row = rowFor("Record Ever Be again");
      expect(row).toHaveTextContent("!");
      expect(row).not.toHaveTextContent(/optional/i);
      expect(row).toHaveAttribute("title", "The lighting data for Ever Be was damaged and set aside; record it again.");
    });

    it("both: the settings error leads and both rows are listed", () => {
      renderDashboard(loadedServiceState, readyBase({ ...damaged(["k1", "Ever Be"]), settings_degraded: true, settings_degraded_reason: "not valid JSON" }));
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Error: Saved settings could not be read");
      expect(rowFor("Saved settings could not be read")).toBeDefined();
      expect(rowFor("Record Ever Be again")).toBeDefined();
      expect(screen.getByRole("tooltip", { hidden: true })).not.toHaveTextContent(/optional/i);
    });

    it("two songs: the first leads and the list shows both", () => {
      renderDashboard(loadedServiceState, readyBase(damaged(["k1", "Ever Be"], ["k2", "Elohim"])));
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Record Ever Be again");
      expect(rowFor("Record Ever Be again")).toBeDefined();
      expect(rowFor("Record Elohim again")).toBeDefined();
    });

    it("a very long song name stays one line in the pill (cut by the pill) and whole in the list and title", () => {
      const title = "Great Are You Lord, Reign In Us, Holy Spirit Come, and Every Other Verse Of The Medley We Sing";
      renderDashboard(loadedServiceState, readyBase(damaged(["k1", title])));
      const pill = screen.getByTestId("header-chip");
      expect(pill).toHaveTextContent(`Record ${title} again`);
      expect(pill).toHaveAttribute("title", `The lighting data for ${title} was damaged and set aside; record it again.`);
      expect(pill.querySelector(".header-chip-status")?.textContent).toBe(`Record ${title} again`);
    });

    it("the row and the pill warning are gone as soon as the song is recorded again, without a restart", () => {
      const options = readyBase(damaged(["k1", "Ever Be"], ["k2", "Elohim"]));
      const { rerender } = renderDashboard(loadedServiceState, options);
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Record Ever Be again");
      const withCapture = (keys: string[]): SettingsResponse => ({
        ...options.settings,
        settings: {
          ...options.settings.settings,
          lights: {
            ...options.settings.settings.lights,
            cue_maps: Object.fromEntries(keys.map((key) => [key, {
              song_key: key, song_title: key, cues: [], osc_cues: [],
              capture: { complete: true, coverage_seconds: 200, recorded_at: "2026-11-10T09:00:00Z" },
            }])),
          },
        },
      });
      // The server still lists both songs (it says what happened at start); only the saved data tells.
      rerender(dashboardElement(loadedServiceState, { ...options, settings: withCapture(["k1"]) }));
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Record Elohim again");
      expect(rowFor("Record Ever Be again")).toBeUndefined();
      expect(rowFor("Record Elohim again")).toBeDefined();
      rerender(dashboardElement(loadedServiceState, { ...options, settings: withCapture(["k1", "k2"]) }));
      expect(screen.getByRole("button", { name: /^Ready$/ })).toHaveClass("border-success-soft");
      expect(rowFor("Record Elohim again")).toBeUndefined();
    });

    it("the read-only Live status view is unchanged: no settings rows, no warning", () => {
      renderDashboard(loadedServiceState, {
        ...readyBase({ ...damaged(["k1", "Ever Be"]), settings_degraded: true }),
        capabilities: { ...LOCAL_CAPABILITIES, canConfigure: false },
      });
      expect(screen.getByTestId("header-chip")).toHaveTextContent("Live status");
      expect(rows().some((row) => /settings|Record /.test(row.textContent ?? ""))).toBe(false);
    });
  });

  it("a configured Lighting output that disconnects is not part of the system check (it has its own card)", () => {
    const configuredlightssettings: SettingsResponse = {
      ...productionSettings,
      settings: { ...productionSettings.settings, lights: { ...productionSettings.settings.lights, enabled: true, output_name: "StagePilot to Lightkey" } },
    };

    renderDashboard(loadedServiceState, {
      settings: configuredlightssettings,
      state: applicationState(loadedServiceState, { lights_status: "disconnected", plugins: {} }),
    });

    expect(screen.queryByText(/^Lighting (MIDI|OSC)/)).not.toBeInTheDocument();
    expect(screen.queryByText("Optional")).not.toBeInTheDocument();
  });

  it("renders manual controls as live buttons in the four tones with 44px targets", () => {
    renderDashboard(loadedServiceState);

    const expected: Record<string, string> = {
      "Start next": "go",
      "Restart current": "go",
      Previous: "back",
      "Reload service plan": "reload",
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
        expect(button.className).toContain(tone === "go" ? "active:text-ink" : tone === "back" ? "active:text-ink" : tone === "reload" ? "active:text-on-info" : "active:text-on-danger");
      }
      expect(button.className).not.toMatch(/hover:text-white|slate-|rose-|emerald-/);
    }
  });

  it("has no spacer tool: widgets are placed freely, and Reset layout needs no confirmation (owner 2026-10-14)", async () => {
    window.localStorage.removeItem("stagepilot.dashboard-layout.v2");
    const user = userEvent.setup();
    renderDashboard(loadedServiceState);

    await user.click(screen.getByRole("button", { name: "Edit layout" }));
    expect(screen.queryByRole("button", { name: "Add spacer" })).not.toBeInTheDocument();
    expect(screen.queryByText("Intentional spacer")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Compact layout" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Reset layout" }));
    expect(screen.queryByRole("dialog", { name: "Reset dashboard layout?" })).not.toBeInTheDocument();
    const reset = JSON.parse(window.localStorage.getItem("stagepilot.dashboard-layout.v2")!);
    expect(reset.desktop.map((item: { id: string }) => item.id).sort()).toEqual(["events", "manual-controls", "now-playing", "service-plan"]);
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

describe("Dashboard reduced motion", () => {
  const runResize = async (reduce: boolean) => {
    const originalWidth = window.innerWidth;
    const animate = vi.fn(() => ({ cancel: vi.fn(), finished: Promise.resolve() }));
    Object.defineProperty(HTMLElement.prototype, "animate", { configurable: true, value: animate });
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: reduce && query.includes("prefers-reduced-motion"),
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1200 });
    try {
      renderDashboard(loadedServiceState);
      animate.mockClear();
      Object.defineProperty(window, "innerWidth", { configurable: true, value: 800 });
      act(() => window.dispatchEvent(new Event("resize")));
      await waitFor(() => expect(document.querySelector(".connections-status")).toHaveClass("status-compact"));
      return animate;
    } finally {
      Object.defineProperty(window, "innerWidth", { configurable: true, value: originalWidth });
      delete (HTMLElement.prototype as { animate?: unknown }).animate;
      vi.unstubAllGlobals();
    }
  };

  it("reduced motion: status cards switch to the compact layout with no tween", async () => {
    const animate = await runResize(true);
    expect(animate).not.toHaveBeenCalled();
  });

  it("motion allowed: the same switch still animates the cards", async () => {
    const animate = await runResize(false);
    expect(animate).toHaveBeenCalled();
  });
});

/** Longer than the panel reveal (EMERGE_MS, 500 ms) so a closing panel's fallback timer has fired. */
const CLOSE_REVEAL_MS = 1_000;
/** waitFor budget for a panel that swaps or unmounts on a real timer: the reveal plus headroom for a loaded runner. */
const WAIT_FOR_REVEAL_MS = 5_000;

describe("Dashboard connection configuration panels", () => {
  it("keeps the retained first-launch setup UI disabled", () => {
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    expect(screen.queryByLabelText("StagePilot configuration progress")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Finish configuring StagePilot" }),
    ).not.toBeInTheDocument();
  });

  it("opens only the clicked connection and toggles it closed on a second click", async () => {
    const user = userEvent.setup();
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    const planningCenter = screen.getByRole("button", { name: /^Services Connected/ });
    const midiConnection = screen.getByRole("button", { name: /^MIDI \/ Playback Connected/ });

    expect(planningCenter).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("heading", { name: "Services" })).not.toBeInTheDocument();

    await user.click(planningCenter);

    expect(planningCenter).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("heading", { name: "Services" })).toBeInTheDocument();

    await user.click(midiConnection);

    expect(planningCenter).toHaveAttribute("aria-expanded", "false");
    expect(midiConnection).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Services" })).not.toBeInTheDocument(), { timeout: WAIT_FOR_REVEAL_MS });
    await waitFor(() => expect(screen.getByRole("heading", { name: "Playback" })).toBeInTheDocument(), { timeout: WAIT_FOR_REVEAL_MS });

    await user.click(midiConnection);

    expect(midiConnection).toHaveAttribute("aria-expanded", "false");
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Playback" })).not.toBeInTheDocument(), { timeout: WAIT_FOR_REVEAL_MS });
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
    // A closing panel leaves the accessibility tree at once but stays mounted until the reveal's fallback timer (jsdom has no
    // animations to finish) fires, and the next panel only opens after that. The old test waited for that 500 ms timer with real
    // timers behind a 1 s waitFor, which a loaded full-suite run overran. Fake timers make it deterministic: advance past the
    // reveal explicitly instead of racing the wall clock.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plugins: {} }),
    });

    const panels = [
      {
        card: /^Services Connected/,
        close: "Close Services configuration",
        heading: "Services",
      },
      {
        card: /^MIDI \/ Playback Connected/,
        close: "Close MIDI / Playback configuration",
        heading: "Playback",
      },
      {
        card: /^Presentation Connected/,
        close: "Close Presentation configuration",
        heading: "Presentation",
      },
      {
        card: /^Lighting Disconnected/,
        close: "Close Lighting configuration",
        heading: "Lighting",
      },
      {
        card: /^StagePilot Connected/,
        close: "Close StagePilot configuration",
        heading: "StagePilot",
      },
    ] as const;

    const setupSections = () => document.querySelectorAll("section[data-variant=\"setup\"]");
    for (const panel of panels) {
      const panelHeading = () => screen.queryAllByRole("heading", { name: panel.heading }).filter((h) => h.closest("section[data-variant=\"setup\"]") !== null);
      await user.click(screen.getByRole("button", { name: panel.card }));
      expect(panelHeading()).toHaveLength(1);

      await user.click(screen.getByRole("button", { name: panel.close }));
      // The closing panel leaves the accessibility tree at once but stays mounted until the reveal ends.
      expect(panelHeading()).toHaveLength(0);
      act(() => { vi.advanceTimersByTime(CLOSE_REVEAL_MS); });
      expect(setupSections()).toHaveLength(0);
    }
  }, 20_000); // five full panel mounts: ~0.7 s idle, but 7x slower on a loaded runner (the 5 s default was overrun)

  it("shows a live remaining clock alongside elapsed song duration", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-13T16:00:30Z"));
    try {
      renderDashboard(loadedServiceState, {
        state: applicationState(loadedServiceState, {
          current_song: loadedPlan.songs[0] ?? null,
          current_song_index: 0,
          playing_song: loadedPlan.songs[0] ?? null, playing_song_index: 0, up_next_song: loadedPlan.songs[1] ?? null,
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

  it("keeps the frozen remaining time when the timer is stopped, using the existing stopped wording", () => {
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, {
        current_song: loadedPlan.songs[0] ?? null,
        current_song_index: 0,
        playing_song: loadedPlan.songs[0] ?? null, playing_song_index: 0, up_next_song: null,
        timer: {
          status: "stopped",
          duration_seconds: 336,
          started_at: null,
          position_seconds: 36,
          remaining_seconds: 300,
          last_error: null,
        },
      }),
    });

    expect(screen.queryByText(/^Timer /)).not.toBeInTheDocument();
    expect(screen.getByText("05:00")).toBeInTheDocument();
    expect(screen.queryByText(/paused/i)).not.toBeInTheDocument();
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
          playing_song: shortSong, playing_song_index: 0, up_next_song: null,
          timer: {
            status: "running",
            duration_seconds: 263,
            started_at: "2026-07-13T16:00:00.000Z",
            last_error: null,
          },
        }),
      });

      expect(screen.getByText("04:22")).toBeInTheDocument();
      expect(screen.getByText("00:01")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("turns the live controls off with the reason when StagePilot is unreachable, and back on when it returns", () => {
    const view = renderDashboard(loadedServiceState, { live: false });
    expect(screen.getByText("Can't reach StagePilot, so these controls are off until it is back.")).toBeInTheDocument();
    for (const name of ["Start next", "Restart current", "Previous", "Reload service plan", "Stop timer", "Reset position"]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
    view.unmount();
    renderDashboard(loadedServiceState, { live: true });
    expect(screen.queryByText(/Can't reach StagePilot/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start next" })).toBeEnabled();
  });
});


describe("Long content and empty lists", () => {
  const LONG = "W".repeat(200);
  const base = loadedPlan.songs[0]!;

  it("wraps a 200-character song title and plan title instead of truncating them", () => {
    const plan = {
      ...loadedPlan,
      title: LONG,
      songs: [{ ...base, id: "long", title: LONG }],
    };
    renderDashboard(loadedServiceState, { state: applicationState(loadedServiceState, { plan, next_song: plan.songs[0] }) });
    const row = within(screen.getByRole("list", { name: "Service plan order" })).getByText(LONG);
    expect(row).not.toHaveClass("truncate");
    expect(row).toHaveClass("break-words");
    expect(screen.getByRole("heading", { name: LONG })).toHaveClass("break-words");
  });

  it("marks only the next song with the info label and chip; the current song keeps the primary family", () => {
    const songs = [
      { ...base, id: "a", order: 1, title: "Alpha" },
      { ...base, id: "b", order: 2, title: "Bravo", is_generic: true },
      { ...base, id: "c", order: 3, title: "Charlie" },
    ];
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plan: { ...loadedPlan, songs }, current_song: songs[0], current_song_index: 0, playing_song: songs[0], playing_song_index: 0, up_next_song: songs[1], next_song: songs[1] }),
    });
    const rows = within(screen.getByRole("list", { name: "Service plan order" })).getAllByRole("listitem");
    const upNext = within(rows[1]!).getByText("Up next");
    expect(upNext).toHaveClass("text-info-text");
    expect(upNext).not.toHaveClass("text-warning-text");
    expect(within(rows[1]!).getByText("2")).toHaveClass("bg-info/15", "text-info-text");
    expect(within(rows[1]!).getByText("Generic item")).toHaveClass("text-warning-text");
    expect(within(rows[0]!).queryByText("Up next")).toBeNull();
    expect(within(rows[0]!).getByText("1")).toHaveClass("bg-primary", "text-on-primary");
    expect(within(rows[0]!).getByText("Current · live")).toHaveClass("text-primary-text");
    expect(within(rows[2]!).queryByText("Up next")).toBeNull();
    expect(within(rows[2]!).getByText("3")).toHaveClass("bg-ink/5", "text-ink-muted");
    expect(within(rows[2]!).getByText("3")).not.toHaveClass("bg-info/15");
  });

  it("after Previous the playing song keeps Current · live and the previous song is Up next (owner 2026-10-13)", () => {
    const songs = [
      { ...base, id: "a", order: 1, title: "Alpha" },
      { ...base, id: "b", order: 2, title: "Bravo" },
      { ...base, id: "c", order: 3, title: "Charlie" },
    ];
    // Bravo is playing; Previous moved the cursor to Alpha without starting it, so Start next will play Alpha.
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, {
        plan: { ...loadedPlan, songs },
        current_song: songs[0], current_song_index: 0, next_song: songs[1],
        playing_song: songs[1], playing_song_index: 1, up_next_song: songs[0],
      }),
    });
    const rows = within(screen.getByRole("list", { name: "Service plan order" })).getAllByRole("listitem");
    expect(within(rows[0]!).getByText("Up next")).toBeInTheDocument();
    expect(within(rows[0]!).queryByText("Current · live")).toBeNull();
    expect(within(rows[1]!).getByText("Current · live")).toBeInTheDocument();
    expect(within(rows[1]!).queryByText("Up next")).toBeNull();
    expect(within(rows[2]!).queryByText("Up next")).toBeNull();
    // The Now playing panel agrees: it names the playing song and shows the same Up next.
    expect(screen.getByRole("heading", { level: 2, name: "Bravo" })).toBeInTheDocument();
    expect(screen.getByText("2 of 3")).toBeInTheDocument();
  });

  it("labels nothing as playing before the first song starts, and Up next is the first song", () => {
    const songs = [{ ...base, id: "a", order: 1, title: "Alpha" }, { ...base, id: "b", order: 2, title: "Bravo" }];
    renderDashboard(loadedServiceState, {
      state: applicationState(loadedServiceState, { plan: { ...loadedPlan, songs }, up_next_song: songs[0] }),
    });
    const rows = within(screen.getByRole("list", { name: "Service plan order" })).getAllByRole("listitem");
    expect(screen.queryByText("Current · live")).toBeNull();
    expect(within(rows[0]!).getByText("Up next")).toBeInTheDocument();
    expect(screen.getByText("Waiting for first cue")).toBeInTheDocument();
    expect(screen.getByText("Not started")).toBeInTheDocument();
  });

  it("keeps the Now playing Up next caption muted and its value plain ink", () => {
    renderDashboard(loadedServiceState, { state: applicationState(loadedServiceState, { next_song: base }) });
    const caption = screen.getByText("Up next", { selector: "p" });
    expect(caption).toHaveClass("text-xs", "text-ink-muted");
    expect(caption.nextElementSibling).toHaveClass("text-ink");
  });

  it("renders 300 songs in the plan list", () => {
    const songs = Array.from({ length: 300 }, (_, index) => ({ ...base, id: `s${index}`, order: index + 1, title: `Song ${index + 1}` }));
    renderDashboard(loadedServiceState, { state: applicationState(loadedServiceState, { plan: { ...loadedPlan, songs } }) });
    expect(within(screen.getByRole("list", { name: "Service plan order" })).getAllByRole("listitem")).toHaveLength(300);
  });

  it("shows one sentence and one button when the plan has no songs, and the button reloads the plan", async () => {
    const dispatch = vi.fn();
    renderDashboard(loadedServiceState, { dispatch, state: applicationState(loadedServiceState, { plan: { ...loadedPlan, songs: [] }, next_song: null }) });
    expect(screen.getByText("No songs in this service plan yet.")).toBeInTheDocument();
    const plan = screen.getByText("No songs in this service plan yet.").parentElement as HTMLElement;
    expect(within(plan).getAllByRole("button")).toHaveLength(1);
    await userEvent.click(within(plan).getByRole("button", { name: "Reload service plan" }));
    expect(dispatch).toHaveBeenCalledWith("reload_plan");
  });

  it("does not show the empty state when the plan has songs", () => {
    renderDashboard(loadedServiceState);
    expect(screen.queryByText("No songs in this service plan yet.")).not.toBeInTheDocument();
  });
});
