import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ApplicationState, PlanningCenterStatusResponse, SettingsResponse } from "../types";
import { PlanningCenterSetupPanel, TIMEZONE_OPTIONS } from "./PlanningCenterSetupPanel";

const desktop = vi.hoisted(() => ({
  openExternalUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../desktop", () => ({
  openExternalUrl: desktop.openExternalUrl,
}));

const settings: SettingsResponse = {
  settings: {
    schema_version: 1,
    onboarding: { general_completed: false },
    integration_modes: {
      service_source: "demo",
      midi_source: "simulated",
      timer_output: "simulated",
    },
    timezone: "America/Los_Angeles",
    log_level: "INFO",
    server_port: 8765,
    planning_center: {
      app_id: "saved-app-id",
      service_type_id: "sunday",
      plan_title_preference: "Sunday Morning",
      preferred_service_time: "09:00",
      upcoming_lookahead_days: 30,
      request_timeout_seconds: 10,
    },
    midi: {
      enabled: false,
      input_name: null,
      channel: 1,
      note: 112,
      mappings: {
        start_next: 100,
        restart_current: 101,
        previous: 102,
        next: 103,
        reload_plan: 104,
        stop_timer: 105,
      },
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

const state: ApplicationState = {
  revision: 1,
  updated_at: "2026-07-14T12:00:00Z",
  application_status: "running",
  plan: null,
  current_song: null,
  next_song: null,
  current_song_index: null,
  planning_center_status: "disconnected",
  midi_status: "disconnected",
  propresenter_status: "disconnected",
  lights_status: "disconnected",
  service_load: {
    status: "idle",
    target_date: null,
    candidates: [],
    skipped_items: [],
    message: null,
    is_stale: false,
    last_attempt_at: null,
  },
  timer: {
    status: "idle",
    duration_seconds: null,
    started_at: null,
    last_error: null,
  },
  plugins: {},
  recent_events: [],
  recent_errors: [],
  last_successful_plan_reload_at: null,
  last_action: null,
};

function renderPanel({
  onTest = vi.fn(),
  onSave = vi.fn(),
  onLoadServiceTypes = vi.fn(),
  onSignInOAuth = vi.fn(),
  onDisconnectOAuth = vi.fn(),
  panelSettings = settings,
  panelState = state,
  status = {
    connection_status: "disconnected",
    configured: true,
    app_id: "saved-app-id",
    service_type_id: "sunday",
    planning_center_secret_saved: true,
    connection_method: "manual",
    oauth_connected: false,
    oauth_needs_reconnect: false,
    detail: null,
  } as PlanningCenterStatusResponse,
  pendingOperation = null,
} = {}) {
  render(
    <PlanningCenterSetupPanel
      error={null}
      message={null}
      onClose={vi.fn()}
      onDisconnectOAuth={onDisconnectOAuth}
      onLoadServiceTypes={onLoadServiceTypes}
      onReload={vi.fn()}
      onSave={onSave}
      onSelectPlan={vi.fn()}
      onSignInOAuth={onSignInOAuth}
      onTest={onTest}
      pendingAction={null}
      pendingOperation={pendingOperation}
      pendingPlanId={null}
      serviceTypes={[
        { id: "sunday", name: "Sunday Morning" },
        { id: "wednesday", name: "Wednesday Service" },
      ]}
      settings={panelSettings}
      state={panelState}
      status={status}
    />,
  );
}

describe("PlanningCenterSetupPanel", () => {
  it("uses design-system primitives for the panel, fields and buttons", () => {
    renderPanel();
    expect(document.getElementById("planning-center-configuration")).toHaveAttribute("data-variant", "setup");
    expect(screen.getByLabelText("Service type").tagName).toBe("SELECT");
    expect(screen.getByLabelText("Secret")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("Secret")).toHaveAttribute("autocomplete", "current-password");
    expect(screen.getByRole("button", { name: "Save settings" })).toHaveAttribute("data-variant", "primary");
    const load = screen.getByRole("button", { name: "Load service types" });
    expect(load).toHaveAttribute("data-variant", "tinted");
    expect(load).toHaveAttribute("data-tone", "services");
  });

  it.each([
    ["manual", false],
    ["oauth", true],
  ] as const)("defaults Manual API Connection closed for %s connections", (connectionMethod, oauthConnected) => {
    renderPanel({
      status: {
        connection_status: oauthConnected ? "connected" : "disconnected",
        configured: true,
        app_id: oauthConnected ? null : "saved-app-id",
        service_type_id: "sunday",
        planning_center_secret_saved: !oauthConnected,
        connection_method: connectionMethod,
        oauth_connected: oauthConnected,
        oauth_needs_reconnect: false,
        detail: null,
      },
    });

    const summary = screen.getByText("Manual API Connection");
    const disclosure = summary.closest("details");
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.queryByText("Advanced / manual connection")).not.toBeInTheDocument();
  });

  it("keeps shared service plan settings visible outside the closed manual disclosure", () => {
    renderPanel();

    const disclosure = screen.getByText("Manual API Connection").closest("details");
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.getByRole("heading", { name: "Service plan configuration" })).toBeInTheDocument();

    for (const label of [
      "Service type",
      "Timezone",
      "Service plan title preference",
      "Preferred service time",
    ]) {
      expect(screen.getByLabelText(label).closest("details")).toBeNull();
    }
    for (const name of ["Load service types", "Save settings", "Load today’s service plan"]) {
      expect(screen.getByRole("button", { name }).closest("details")).toBeNull();
    }
  });

  it("enables OAuth service actions without a manual Application ID or secret", async () => {
    const onLoadServiceTypes = vi.fn();
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({
      onLoadServiceTypes,
      onSave,
      panelSettings: {
        ...settings,
        planning_center_secret_saved: false,
        settings: {
          ...settings.settings,
          planning_center: {
            ...settings.settings.planning_center,
            app_id: null,
          },
        },
      },
      status: {
        connection_status: "connected",
        configured: true,
        app_id: null,
        service_type_id: "sunday",
        planning_center_secret_saved: false,
        connection_method: "oauth",
        oauth_connected: true,
        oauth_needs_reconnect: false,
        detail: null,
      },
    });

    const loadButton = screen.getByRole("button", { name: "Load service types" });
    const saveButton = screen.getByRole("button", { name: "Save settings" });
    expect(loadButton).toBeEnabled();
    expect(saveButton).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Test connection" })).not.toBeInTheDocument();

    await user.click(loadButton);
    await user.click(saveButton);
    expect(onLoadServiceTypes).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ app_id: "", service_type_id: "sunday" }),
      "America/Los_Angeles",
    );
  });

  it("shows PAT instructions after a one-second hover and keeps the linked panel interactive", () => {
    vi.useFakeTimers();
    try {
      renderPanel();
      fireEvent.click(screen.getByText("Manual API Connection"));

      const helpButton = screen.getByRole("button", {
        name: "How to get a Planning Center Personal Access Token",
      });
      expect(helpButton).toHaveClass("leading-none", "pb-px");
      const hoverArea = helpButton.parentElement!;
      const tooltip = screen.getByRole("tooltip");

      fireEvent.mouseEnter(hoverArea);
      act(() => vi.advanceTimersByTime(499));
      expect(tooltip).toHaveClass("invisible");
      act(() => vi.advanceTimersByTime(1));
      expect(tooltip).toHaveClass("visible");

      const link = screen.getByRole("link", { name: "Personal Access Tokens page" });
      expect(link).toHaveAttribute(
        "href",
        "https://api.planningcenteronline.com/personal_access_tokens",
      );
      fireEvent.click(link);
      expect(desktop.openExternalUrl).toHaveBeenCalledWith(
        "https://api.planningcenteronline.com/personal_access_tokens",
      );
      fireEvent.mouseEnter(tooltip);
      expect(tooltip).toHaveClass("visible");

      fireEvent.mouseLeave(hoverArea);
      expect(tooltip).toHaveClass("invisible");
      fireEvent.click(helpButton);
      expect(tooltip).toHaveClass("visible");
    } finally {
      vi.useRealTimers();
    }
  });

  it("populates saved public settings while keeping the credential masked", () => {
    renderPanel();

    expect(screen.getByLabelText("Application ID")).toHaveValue("saved-app-id");
    expect(screen.getByLabelText("Secret")).toHaveValue("");
    expect(screen.getByLabelText("Secret")).toHaveAttribute(
      "placeholder",
      "Saved securely — leave blank to keep",
    );
    expect(screen.getByLabelText("Service type")).toHaveValue("sunday");
    expect(screen.getByLabelText("Timezone")).toHaveValue("America/Los_Angeles");
    expect(screen.getByRole("option", { name: "Los Angeles (UTC-8)" })).toHaveProperty("selected", true);
  });

  it("renders one timezone select option per populated civil UTC offset", () => {
    renderPanel();

    const timezoneSelect = screen.getByLabelText("Timezone");
    expect(timezoneSelect.tagName).toBe("SELECT");
    expect(timezoneSelect.querySelector("input")).toBeNull();
    expect(screen.getAllByRole("option").filter((option) => (
      option.closest("select") === timezoneSelect
    ))).toHaveLength(TIMEZONE_OPTIONS.length);
    expect(screen.getByRole("option", { name: "New York (UTC-5)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "London (UTC+0)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Delhi (UTC+5:30)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Kathmandu (UTC+5:45)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Adelaide (UTC+9:30)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Baker Island (UTC-12)" })).toBeInTheDocument();
  });

  it("labels a saved Etc/GMT+12 preference as UTC-12, not Pago Pago's UTC-11", () => {
    renderPanel({
      panelSettings: {
        ...settings,
        settings: {
          ...settings.settings,
          timezone: "Etc/GMT+12",
        },
      },
    });

    expect(screen.getByLabelText("Timezone")).toHaveValue("Etc/GMT+12");
    expect(screen.getByRole("option", { name: "Baker Island (UTC-12)" })).toHaveProperty("selected", true);
  });

  it("defaults an unsaved timezone to the exact detected system IANA zone", async () => {
    const resolvedOptions = Intl.DateTimeFormat().resolvedOptions();
    const resolvedOptionsSpy = vi.spyOn(
      Intl.DateTimeFormat.prototype,
      "resolvedOptions",
    ).mockReturnValue({ ...resolvedOptions, timeZone: "America/Denver" });
    const onSave = vi.fn();
    const user = userEvent.setup();

    try {
      renderPanel({
        onSave,
        panelSettings: {
          ...settings,
          settings: { ...settings.settings, timezone: "" },
        },
      });

      expect(screen.getByLabelText("Timezone")).toHaveValue("America/Denver");
      expect(screen.getByRole("option", { name: "Denver (UTC-7)" })).toHaveProperty("selected", true);
      await user.click(screen.getByRole("button", { name: "Save settings" }));
      expect(onSave).toHaveBeenCalledWith(expect.any(Object), "America/Denver");
    } finally {
      resolvedOptionsSpy.mockRestore();
    }
  });

  it("preserves a saved IANA zone with its DST-aware rules intact", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({
      onSave,
      panelSettings: {
        ...settings,
        settings: { ...settings.settings, timezone: "America/Denver" },
      },
    });

    expect(screen.getByLabelText("Timezone")).toHaveValue("America/Denver");
    expect(screen.getByRole("option", { name: "Denver (UTC-7)" })).toHaveProperty("selected", true);
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    expect(onSave).toHaveBeenCalledWith(expect.any(Object), "America/Denver");
  });

  it("saves the selected representative city's exact IANA zone", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });

    await user.selectOptions(screen.getByLabelText("Timezone"), "Asia/Kolkata");
    await user.click(screen.getByRole("button", { name: "Save settings" }));

    expect(onSave).toHaveBeenCalledWith(expect.any(Object), "Asia/Kolkata");
    expect(onSave.mock.calls[0]?.[1]).not.toMatch(/^Etc\/GMT/);
  });

  it("renders and saves the All service types option", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });

    const serviceTypeSelect = screen.getByLabelText("Service type");
    expect(screen.getByRole("option", {
      name: "All service types (nearest upcoming service plan)",
    })).toBeInTheDocument();

    await user.selectOptions(serviceTypeSelect, "stagepilot:all-service-types");
    await user.click(screen.getByRole("button", { name: "Save settings" }));

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ service_type_id: "stagepilot:all-service-types" }),
      "America/Los_Angeles",
    );
  });

  it("shows tied All-service candidates with their service type names", () => {
    renderPanel({
      panelState: {
        ...state,
        service_load: {
          ...state.service_load,
          status: "ambiguous",
          target_date: "2026-09-27",
          candidates: [
            {
              id: "plan-sunday",
              title: "Sunday Morning",
              service_type_id: "sunday",
              service_type_name: "Sunday Services",
              target_date: "2026-09-27",
              service_times: ["09:00"],
            },
            {
              id: "plan-students",
              title: "Student Service",
              service_type_id: "students",
              service_type_name: "Student Ministry",
              target_date: "2026-09-27",
              service_times: ["18:00"],
            },
          ],
        },
      },
    });

    expect(screen.getByText("Sunday Services · 09:00")).toBeInTheDocument();
    expect(screen.getByText("Student Ministry · 18:00")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Use service plan" })).toHaveLength(2);
  });

  it("tests temporary credentials and saves a discovered service type", async () => {
    const onTest = vi.fn();
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave, onTest });

    await user.click(screen.getByText("Manual API Connection"));
    await user.type(screen.getByLabelText("Secret"), "replacement-secret");
    await user.click(screen.getByRole("button", { name: "Test connection" }));

    expect(onTest).toHaveBeenCalledWith({
      app_id: "saved-app-id",
      secret: "replacement-secret",
    });

    expect(screen.queryByLabelText("Service source")).not.toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Service type"), "wednesday");
    await user.click(screen.getByRole("button", { name: "Save settings" }));

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        app_id: "saved-app-id",
        secret: "replacement-secret",
        service_type_id: "wednesday",
        plan_title_preference: "Sunday Morning",
        preferred_service_time: "09:00",
      }),
      "America/Los_Angeles",
    );
  });

  it("shows the sign-in button by default and triggers the start-flow call", async () => {
    const onSignInOAuth = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSignInOAuth });

    const button = screen.getByRole("button", { name: "Sign in with Planning Center" });
    await user.click(button);

    expect(onSignInOAuth).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Connected to Planning Center")).not.toBeInTheDocument();
  });

  it("shows the connected state and hides the sign-in button once OAuth-connected", () => {
    renderPanel({
      status: {
        connection_status: "connected",
        configured: true,
        app_id: null,
        service_type_id: "sunday",
        planning_center_secret_saved: false,
        connection_method: "oauth",
        oauth_connected: true,
        oauth_needs_reconnect: false,
        detail: null,
      },
    });

    expect(screen.getByText("Connected to Planning Center")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Sign in with Planning Center" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Disconnect" })).toBeInTheDocument();
  });

  it("the advanced/manual section still works independently when disconnected", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });

    await user.click(screen.getByText("Manual API Connection"));
    await user.clear(screen.getByLabelText("Application ID"));
    await user.type(screen.getByLabelText("Application ID"), "manual-app-id");
    await user.type(screen.getByLabelText("Secret"), "manual-secret");
    await user.selectOptions(screen.getByLabelText("Service type"), "wednesday");
    await user.click(screen.getByRole("button", { name: "Save settings" }));

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        app_id: "manual-app-id",
        secret: "manual-secret",
        service_type_id: "wednesday",
      }),
      "America/Los_Angeles",
    );
  });

  it("renders the reconnect-needed state distinctly from a generic error banner", () => {
    renderPanel({
      status: {
        connection_status: "error",
        configured: true,
        app_id: null,
        service_type_id: "sunday",
        planning_center_secret_saved: false,
        connection_method: "oauth",
        oauth_connected: false,
        oauth_needs_reconnect: true,
        detail: null,
      },
    });

    const reconnectAlert = screen.getByRole("alert");
    expect(reconnectAlert).toHaveTextContent("Your Services connection has expired");
    expect(
      screen.getByRole("button", { name: "Reconnect to Planning Center" }),
    ).toBeInTheDocument();
    // The generic error/message banner (a <p>, not role="alert") is a
    // separate code path and must not render for this state.
    expect(screen.queryByText("Services connection test failed.")).not.toBeInTheDocument();
  });
});
