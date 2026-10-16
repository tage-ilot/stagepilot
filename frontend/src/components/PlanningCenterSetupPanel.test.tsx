import React from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
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
  playing_song: null, playing_song_index: null, up_next_song: null,
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

function panelElement({
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
  pendingOperation = null as "save" | null,
  error = null as string | null,
  panelPlugins = {} as Record<string, unknown>,
} = {}) {
  return (
    <PlanningCenterSetupPanel
      error={error}
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
      state={{ ...panelState, plugins: { ...panelState.plugins, ...panelPlugins } } as ApplicationState}
      status={status}
    />
  );
}

function renderPanel(options: Parameters<typeof panelElement>[0] = {}) {
  return render(panelElement(options));
}

describe("PlanningCenterSetupPanel", () => {
  it("uses design-system primitives for the panel, fields and buttons", () => {
    renderPanel();
    expect(document.getElementById("planning-center-configuration")).toHaveAttribute("data-variant", "setup");
    expect(screen.getByLabelText("Service type").tagName).toBe("SELECT");
    expect(screen.getByLabelText("Secret")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("Secret")).toHaveAttribute("autocomplete", "current-password");
    expect(screen.getByRole("button", { name: "Save configuration" })).toHaveAttribute("data-variant", "primary");
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

    const toggle = screen.getByRole("button", { name: /Manual API Connection/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Advanced / manual connection")).not.toBeInTheDocument();
  });

  it("is an advanced Services Disclosure operated by button, Enter and Space, with a real-value summary only while closed", async () => {
    const user = userEvent.setup();
    renderPanel();
    const toggle = screen.getByRole("button", { name: /Manual API Connection/ });
    expect(toggle.tagName).toBe("BUTTON");
    expect(toggle.closest("[data-variant=advanced]")).toHaveAttribute("data-tone", "services");
    const summary = within(toggle).getByText("App ID and Secret · App ID saved-app-id");
    expect(summary).toHaveAttribute("title", "App ID and Secret · App ID saved-app-id");
    expect(screen.queryByText(/replacement-secret|saved-secret/)).toBeNull();
    const bodyId = toggle.getAttribute("aria-controls");
    expect(bodyId).toBeTruthy();
    expect(document.getElementById(bodyId!)).toContainElement(screen.getByLabelText("Application ID"));
    toggle.focus();
    await user.keyboard("{Enter}");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(within(toggle).queryByText(/App ID saved-app-id/)).toBeNull();
    await user.keyboard(" ");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(toggle).getByText(/App ID saved-app-id/)).toBeInTheDocument();
  });

  it("summarises OAuth sign-in and an unset App ID", () => {
    renderPanel({ status: { connection_status: "connected", configured: true, app_id: null, service_type_id: "sunday", planning_center_secret_saved: false, connection_method: "oauth", oauth_connected: true, oauth_needs_reconnect: false, detail: null } as PlanningCenterStatusResponse, panelSettings: { ...settings, settings: { ...settings.settings, planning_center: { ...settings.settings.planning_center, app_id: null } } } as typeof settings });
    expect(screen.getByRole("button", { name: /Manual API Connection/ })).toHaveTextContent("Sign-in · App ID not set");
  });

  it("keeps shared service plan settings visible outside the closed manual disclosure", () => {
    renderPanel();

    expect(screen.getByRole("button", { name: /Manual API Connection/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("heading", { name: "Service plan configuration" })).toBeInTheDocument();

    for (const label of [
      "Service type",
      "Timezone",
      "Service plan title preference",
      "Preferred service time",
    ]) {
      expect(screen.getByLabelText(label).closest("[data-variant=advanced]")).toBeNull();
    }
    for (const name of ["Load service types", "Save configuration", "Load today’s service plan"]) {
      expect(screen.getByRole("button", { name }).closest("[data-variant=advanced]")).toBeNull();
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
    const saveButton = screen.getByRole("button", { name: "Save configuration" });
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
      fireEvent.click(screen.getByRole("button", { name: /Manual API Connection/ }));

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
      "Saved securely — leave blank to keep…",
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
      await user.click(screen.getByRole("button", { name: "Save configuration" }));
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
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledWith(expect.any(Object), "America/Denver");
  });

  it("saves the selected representative city's exact IANA zone", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });

    await user.selectOptions(screen.getByLabelText("Timezone"), "Asia/Kolkata");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));

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
    await user.click(screen.getByRole("button", { name: "Save configuration" }));

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

    await user.click(screen.getByRole("button", { name: /Manual API Connection/ }));
    await user.type(screen.getByLabelText("Secret"), "replacement-secret");
    await user.click(screen.getByRole("button", { name: "Test connection" }));

    expect(onTest).toHaveBeenCalledWith({
      app_id: "saved-app-id",
      secret: "replacement-secret",
    });

    expect(screen.queryByLabelText("Service source")).not.toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Service type"), "wednesday");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));

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

    const button = screen.getByRole("button", { name: "Planning Center OAuth" });
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
      screen.queryByRole("button", { name: "Planning Center OAuth" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Disconnect" })).toBeInTheDocument();
  });

  it("the advanced/manual section still works independently when disconnected", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });

    await user.click(screen.getByRole("button", { name: /Manual API Connection/ }));
    await user.clear(screen.getByLabelText("Application ID"));
    await user.type(screen.getByLabelText("Application ID"), "manual-app-id");
    await user.type(screen.getByLabelText("Secret"), "manual-secret");
    await user.selectOptions(screen.getByLabelText("Service type"), "wednesday");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));

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

describe("PlanningCenterSetupPanel SaveBar", () => {
  const saveButton = () => screen.getByRole("button", { name: "Save configuration" });
  const bar = () => document.querySelector("[data-save-bar]") as HTMLElement;

  it("replaces the old save buttons with one Save configuration", () => {
    renderPanel();
    expect(screen.getAllByRole("button", { name: "Save configuration" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Save settings" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save and connect" })).not.toBeInTheDocument();
  });

  it("is clean at first, dimmed, with the reason beside it, and the button is not hidden", () => {
    renderPanel({ panelPlugins: { planning_center: true } });
    expect(bar()).toHaveAttribute("data-state", "clean");
    expect(screen.getByText("All changes saved")).toBeInTheDocument();
    expect(saveButton()).toHaveAttribute("aria-disabled", "true");
    expect(screen.queryByText("Not saved yet")).not.toBeInTheDocument();
  });

  it("does not call onSave on a clean save when the connection is already enabled", async () => {
    const onSave = vi.fn();
    renderPanel({ onSave, panelPlugins: { planning_center: true } });
    await userEvent.setup().click(saveButton());
    expect(onSave).not.toHaveBeenCalled();
  });

  it("counts held changes and marks the four fields; nothing saves on change", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    await user.selectOptions(screen.getByLabelText("Service type"), "wednesday");
    await user.type(screen.getByLabelText("Preferred service time"), "1015");
    await user.type(screen.getByLabelText("Service plan title preference"), " Early");
    expect(bar()).toHaveAttribute("data-state", "held");
    expect(bar()).toHaveTextContent(/3\s+changes waiting\. Not saved yet\./);
    expect(screen.getAllByText("Not saved yet").filter((el) => el.hasAttribute("data-held-marker"))).toHaveLength(3);
    expect(screen.getByLabelText("Service type").closest("[data-field]")).toHaveAttribute("data-state", "held");
    expect(screen.getByLabelText("Service plan title preference").closest("[data-field]")).toHaveAttribute("data-state", "held");
    expect(screen.getByLabelText("Preferred service time").closest("[data-field]")).toHaveAttribute("data-state", "held");
    expect(onSave).not.toHaveBeenCalled();
  });

  it("marks Application ID and Remove saved secret as held", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    await user.click(screen.getByRole("button", { name: /Manual API Connection/ }));
    await user.type(screen.getByLabelText("Application ID"), "x");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(1);
    expect(bar()).toHaveTextContent(/1\s+change waiting/);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("Discard restores the saved values and returns to clean", async () => {
    const user = userEvent.setup();
    renderPanel();
    await user.selectOptions(screen.getByLabelText("Service type"), "wednesday");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(screen.getByLabelText("Service type")).toHaveValue("sunday");
    expect(bar()).toHaveAttribute("data-state", "clean");
  });

  it("sends the same payload as before, including secret and remove_secret", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    await user.click(screen.getByRole("button", { name: /Manual API Connection/ }));
    await user.type(screen.getByLabelText("Secret"), "new-secret");
    await user.click(saveButton());
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith(
      {
        app_id: "saved-app-id",
        service_type_id: "sunday",
        plan_title_preference: "Sunday Morning",
        preferred_service_time: "09:00",
        upcoming_lookahead_days: 30,
        request_timeout_seconds: 10,
        secret: "new-secret",
      },
      "America/Los_Angeles",
    );
  });

  it("sends remove_secret when the saved secret is removed", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    await user.click(screen.getByRole("button", { name: /Manual API Connection/ }));
    await user.click(screen.getByLabelText("Remove saved secret"));
    expect(bar()).toHaveTextContent(/1\s+change waiting/);
    await user.click(saveButton());
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ remove_secret: true }), "America/Los_Angeles");
    expect(onSave.mock.calls[0]?.[0]).not.toHaveProperty("secret");
  });

  it("shows Saving… while the save is pending", () => {
    renderPanel({ pendingOperation: "save" });
    expect(bar()).toHaveAttribute("data-state", "saving");
    expect(screen.getByText("Saving…")).toBeInTheDocument();
  });

  it("shows Couldn't save with Try again after a failed save, and Try again re-sends the same payload", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    const view = renderPanel({ onSave, pendingOperation: null });
    await user.selectOptions(screen.getByLabelText("Service type"), "wednesday");
    view.rerender(panelElement({ onSave, pendingOperation: "save" }));
    view.rerender(panelElement({ onSave, pendingOperation: null, error: "Planning Center said no." }));
    expect(bar()).toHaveAttribute("data-state", "error");
    expect(screen.getByText("Couldn't save")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ service_type_id: "wednesday" }), "America/Los_Angeles");
  });

  it("keeps the restart-needed note while the connection is not enabled", () => {
    renderPanel();
    expect(screen.getByText(/Restart StagePilot to start the connection/)).toBeInTheDocument();
  });

  it("keeps Reload service plan, Test connection and sign-in as plain actions outside the bar", () => {
    renderPanel();
    const reload = screen.getByRole("button", { name: "Load today’s service plan" });
    expect(bar().contains(reload)).toBe(false);
    expect(bar().contains(screen.getByRole("button", { name: "Planning Center OAuth" }))).toBe(false);
  });
});

describe("Services hold-until-saved conformance (guard)", () => {
  const bar = () => document.querySelector("[data-save-bar]")!;
  const held: Array<{ label: string; value: string; saved: string; payload: Record<string, unknown> }> = [
    { label: "Application ID", value: "new-app-id", saved: "saved-app-id", payload: { app_id: "new-app-id" } },
    { label: "Secret", value: "new-secret", saved: "", payload: { secret: "new-secret" } },
    { label: "Service type", value: "wednesday", saved: "sunday", payload: { service_type_id: "wednesday" } },
    { label: "Service plan title preference", value: "Morning", saved: "Sunday Morning", payload: { plan_title_preference: "Morning" } },
  ];
  it.each(held)("$label is held: marked, counted, not saved on blur, flagged dirty, restored by Discard, saved only by Save configuration", ({ label, value, saved, payload }) => {
    const onSave = vi.fn();
    const onDirtyChange = vi.fn();
    render(React.cloneElement(panelElement({ onSave }), { onDirtyChange }));
    const control = screen.getByLabelText(label);
    fireEvent.change(control, { target: { value } });
    fireEvent.blur(control);
    expect(onSave).not.toHaveBeenCalled();
    expect(document.querySelectorAll("[data-held-marker], [data-held-note]")).toHaveLength(1);
    expect(bar()).toHaveTextContent("1 change waiting. Not saved yet.");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining(payload), expect.any(String));
    // Save does not clear the draft, so Discard is still offered; the Secret field is the one field that Save clears itself.
    if (label === "Secret") { expect(screen.getByLabelText(label)).toHaveValue(""); return; }
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect((screen.getByLabelText(label) as HTMLInputElement).value).toBe(saved);
    expect(bar()).toHaveAttribute("data-state", "clean");
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  it("Remove saved secret is marked, counted, saved only by Save configuration and restored by Discard", () => {
    const onSave = vi.fn();
    const onDirtyChange = vi.fn();
    render(React.cloneElement(panelElement({ onSave }), { onDirtyChange }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Remove saved secret/ }));
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByRole("checkbox", { name: /Remove saved secret/ }).closest("label")).toHaveTextContent("Not saved yet");
    expect(bar()).toHaveTextContent("1 change waiting. Not saved yet.");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ remove_secret: true }), expect.any(String));
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(screen.getByRole("checkbox", { name: /Remove saved secret/ })).not.toBeChecked();
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  it("Timezone and Preferred service time are held and counted", () => {
    const onDirtyChange = vi.fn();
    render(React.cloneElement(panelElement(), { onDirtyChange }));
    fireEvent.change(screen.getByLabelText("Timezone"), { target: { value: "America/New_York" } });
    expect(bar()).toHaveTextContent("1 change waiting");
    fireEvent.change(screen.getByLabelText("Preferred service time"), { target: { value: "10:00" } });
    expect(bar()).toHaveTextContent("2 changes waiting");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
  });

  it("Load service types and Load today's plan are explicit actions that never change the held count", () => {
    const onLoadServiceTypes = vi.fn();
    render(panelElement({ onLoadServiceTypes }));
    fireEvent.click(screen.getByRole("button", { name: "Load service types" }));
    expect(onLoadServiceTypes).toHaveBeenCalledOnce();
    expect(bar()).toHaveAttribute("data-state", "clean");
  });

  it("keeps the four status checks on one row for as long as their words fit (follows the panel width, not the window)", async () => {
    renderPanel();
    const checks = document.querySelector(".status-checks")!;
    expect(checks.parentElement).toHaveClass("status-checks-box");
    expect(checks.querySelectorAll("dt")).toHaveLength(4);
    const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
    const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
    expect(css).toMatch(/\.status-checks-box \{\s*container-type: inline-size;/);
    expect(css).toMatch(/@container \(min-width: 30rem\) \{\s*\.status-checks \{\s*grid-template-columns: repeat\(4, minmax\(max-content, 1fr\)\);/);
    expect(css).toMatch(/@container \(min-width: 23rem\) \{\s*\.status-checks \{\s*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/);
  });
});
