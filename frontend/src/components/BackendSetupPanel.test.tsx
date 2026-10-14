import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import * as desktop from "../desktop";
import * as diagnostics from "../diagnostics";
import type { ApplicationState, SettingsResponse } from "../types";
import { BackendSetupPanel } from "./BackendSetupPanel";

vi.mock("../api", async (original) => ({
  ...await original<typeof import("../api")>(),
  getRemoteStatus: vi.fn(), getRemoteUsers: vi.fn(), setRemoteEnabled: vi.fn(),
  bootstrapRemote: vi.fn(), createRemoteUser: vi.fn(),
  updateRemoteUser: vi.fn(), deleteRemoteUser: vi.fn(), regenerateRemote: vi.fn(),
}));
vi.mock("../desktop", async (original) => ({
  ...await original<typeof import("../desktop")>(),
  setRemoteAutostart: vi.fn(),
}));
vi.mock("../diagnostics", async (original) => ({
  ...await original<typeof import("../diagnostics")>(),
  sendDiagnosticsBundle: vi.fn(),
  copyDiagnosticsLog: vi.fn(),
}));

const off: api.RemoteStatus = {available: true, provisioned: true, credential_available: true, enabled: false, state: "off", url: null, needs_operator: true, message: null, temporary_url: true, permanently_revoked: false};
const operator: api.RemoteUser = {id: "one", email: "operator@example.test", role: "Operator", enabled: true};

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
    status: "idle", target_date: null, candidates: [], skipped_items: [],
    message: null, is_stale: false, last_attempt_at: null,
  },
  timer: { status: "idle", duration_seconds: null, started_at: null, last_error: null },
  plugins: {},
  recent_events: [],
  recent_errors: [],
  last_successful_plan_reload_at: null,
  last_action: null,
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getRemoteStatus).mockResolvedValue(off);
  vi.mocked(api.getRemoteUsers).mockResolvedValue([]);
  vi.mocked(desktop.setRemoteAutostart).mockResolvedValue();
});

const health = {status: "healthy", version: "1.1.104-beta.36", application_status: "running", plugins: []} as unknown as NonNullable<React.ComponentProps<typeof BackendSetupPanel>["health"]>;
const baseState = state;

const saved = {
  timezone: "America/Los_Angeles", log_level: "INFO", server_port: 8765, lan_access: false,
  web_dashboard_pin_enabled: true, release_channel: "BETA",
};
const settingsResponse = {settings: saved} as unknown as SettingsResponse;

function renderPanel(props: Partial<React.ComponentProps<typeof BackendSetupPanel>> = {}) {
  return render(
    <BackendSetupPanel
      state={state}
      health={null}
      live={false}
      onClose={vi.fn()}
      settings={null}
      error={null}
      message={null}
      pending={false}
      onSave={vi.fn()}
      {...props}
    />,
  );
}

describe("Status section", () => {
  // The old "Connection health" block (per-connection name/status/version list) was removed on
  // purpose: the five status cards already show each connection. Keep Status to identity and endpoints.
  it("shows Version, Running since and Connections as cards; the endpoints sit one row down", () => {
    renderPanel();
    expect(screen.getByText("Status")).toBeInTheDocument();
    for (const label of ["Version", "Running since", "Connections"]) expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getByText(/\d of 5 connected/)).toBeInTheDocument();
    // The endpoints and revision moved into a collapsed "Connection details" row (Activity look).
    const details = screen.getByRole("button", {name: /Connection details/});
    expect(details).toHaveAttribute("aria-expanded", "false");
    for (const label of ["API endpoint", "WebSocket", "State revision"]) expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.queryByText(/connection health/i)).not.toBeInTheDocument();
    expect(screen.queryByText("No connections reported.")).not.toBeInTheDocument();
  });

  it("draws the three facts as small bordered cards, like the style guide, under a kicker-style heading", () => {
    renderPanel();
    expect(screen.getByRole("heading", {level: 3, name: "Status"})).toHaveClass("uppercase", "text-ink-muted");
    for (const label of ["Version", "Running since", "Connections"]) {
      const card = screen.getByText(label).parentElement!;
      expect(card).toHaveClass("rounded-ds-lg", "border", "border-edge-subtle", "bg-surface-950/40");
      expect(screen.getByText(label)).toHaveClass("uppercase", "text-ink-muted");
    }
  });

  it("offers UTC and the saved zone in the Timezone list, even though browsers omit UTC", () => {
    renderPanel({settings: {settings: {...saved, timezone: "Pacific/Chatham"}} as unknown as SettingsResponse});
    const options = within(screen.getByLabelText("Timezone")).getAllByRole("option").map((o) => (o as HTMLOptionElement).value);
    expect(options).toContain("UTC");
    expect(options).toContain("Pacific/Chatham");
    expect(new Set(options).size).toBe(options.length);
  });

  it("says Not reported when an older backend sends no start time, and the time when it does", () => {
    const {unmount} = renderPanel({health: {...health, started_at: null}});
    expect(screen.getByText("Running since").parentElement).toHaveTextContent("Not reported");
    unmount();
    renderPanel({health: {...health, started_at: "2026-10-14T08:02:00Z"}});
    expect(screen.getByText("Running since").parentElement!.textContent).toMatch(/\d{1,2}:02/);
  });

  it("counts connected connections from the live status of each one", () => {
    renderPanel({state: {...baseState, planning_center_status: "connected", midi_status: "connected", propresenter_status: "disconnected", lights_status: "disconnected"}, live: true});
    expect(screen.getByText("3 of 5 connected")).toBeInTheDocument();
  });
});

describe("Remote Access checkbox", () => {
  it("checking the box enables Remote immediately even with no Operator yet, then shows the bootstrap UI", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue(off);
    vi.mocked(api.setRemoteEnabled).mockImplementation(async (enabled) => {
      const next = {...off, enabled, state: enabled ? "enabling" as const : "off" as const, needs_operator: true};
      vi.mocked(api.getRemoteStatus).mockResolvedValue(next);
      return next;
    });
    renderPanel();
    const checkbox = await screen.findByRole("checkbox", {name: /Remote Access/});
    expect(checkbox).not.toBeChecked();

    fireEvent.click(checkbox);
    await waitFor(() => expect(api.setRemoteEnabled).toHaveBeenCalledWith(true));
    expect(desktop.setRemoteAutostart).toHaveBeenCalledWith(true);
    await waitFor(() => expect(checkbox).toBeChecked());
    expect(checkbox.closest("label")).not.toHaveAttribute("aria-expanded");
    expect(checkbox.closest("label")).not.toHaveAttribute("aria-controls");

    // Enabled, but still no Operator: the bootstrap form is a required next
    // step and stays visible, not a precondition that was waited on.
    expect(await screen.findByRole("heading", {name: "Create first Operator"})).toBeInTheDocument();
    expect(screen.getByText(/Remote Access is running\. Create the first Operator/)).toBeInTheDocument();

    // Creating the first Operator now must NOT call setRemoteEnabled again
    // (it's already enabled) -- only bootstrapRemote itself.
    vi.mocked(api.bootstrapRemote).mockResolvedValue(operator);
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    fireEvent.change(screen.getByLabelText("Email"), {target: {value: operator.email}});
    fireEvent.change(screen.getByLabelText("Password"), {target: {value: "long-test-password"}});
    fireEvent.click(screen.getByRole("button", {name: "Create Operator"}));
    await waitFor(() => expect(api.bootstrapRemote).toHaveBeenCalledWith(operator.email, "long-test-password"));
    expect(api.setRemoteEnabled).toHaveBeenCalledTimes(1);
  });


  it("unchecking with confirmation disables Remote and collapses the panel", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    vi.mocked(api.setRemoteEnabled).mockImplementation(async (enabled) => {
      const next = {...off, enabled, state: enabled ? "connected" as const : "off" as const, needs_operator: false, url: enabled ? "https://test.trycloudflare.com" : null};
      vi.mocked(api.getRemoteStatus).mockResolvedValue(next);
      return next;
    });
    renderPanel();
    const checkbox = await screen.findByRole("checkbox", {name: /Remote Access/});
    await waitFor(() => expect(checkbox).toBeChecked());

    fireEvent.click(checkbox);
    expect(await screen.findByRole("group", {name: "Confirm disable Remote Access"})).toBeInTheDocument();
    expect(api.setRemoteEnabled).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", {name: "Confirm disable"}));
    await waitFor(() => expect(api.setRemoteEnabled).toHaveBeenCalledWith(false));
    expect(desktop.setRemoteAutostart).toHaveBeenCalledWith(false);
    await waitFor(() => expect(checkbox).not.toBeChecked());
    expect(checkbox.closest("label")).not.toHaveAttribute("aria-expanded");
    expect(screen.queryByLabelText("Remote Access", {selector: "div"})).not.toBeInTheDocument();
  });

  it("unchecking then cancelling reverts the checkbox to checked with Remote Access still enabled", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
    renderPanel();
    const checkbox = await screen.findByRole("checkbox", {name: /Remote Access/});
    await waitFor(() => expect(checkbox).toBeChecked());

    fireEvent.click(checkbox);
    expect(await screen.findByRole("group", {name: "Confirm disable Remote Access"})).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", {name: "Cancel"}));
    expect(api.setRemoteEnabled).not.toHaveBeenCalled();
    expect(checkbox).toBeChecked();
    expect(screen.queryByRole("group", {name: "Confirm disable Remote Access"})).not.toBeInTheDocument();
  });

  it("disabling still works identically regardless of whether an Operator exists", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: true, url: "https://test.trycloudflare.com"});
    vi.mocked(api.setRemoteEnabled).mockImplementation(async (enabled) => {
      const next = {...off, enabled, state: enabled ? "connected" as const : "off" as const, needs_operator: true, url: enabled ? "https://test.trycloudflare.com" : null};
      vi.mocked(api.getRemoteStatus).mockResolvedValue(next);
      return next;
    });
    renderPanel();
    const checkbox = await screen.findByRole("checkbox", {name: /Remote Access/});
    await waitFor(() => expect(checkbox).toBeChecked());

    fireEvent.click(checkbox);
    expect(await screen.findByRole("group", {name: "Confirm disable Remote Access"})).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", {name: "Confirm disable"}));
    await waitFor(() => expect(api.setRemoteEnabled).toHaveBeenCalledWith(false));
    expect(desktop.setRemoteAutostart).toHaveBeenCalledWith(false);
    await waitFor(() => expect(checkbox).not.toBeChecked());
  });
});

describe("Send all logs to developer", () => {
  it("clicking sends the bundle, enters cooldown, and re-enables after 60s", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(diagnostics.sendDiagnosticsBundle).mockResolvedValue({
      ok: true,
      message: "Logs sent to the developer.",
    });
    renderPanel();

    const button = screen.getByRole("button", { name: "Send all logs to developer" });
    fireEvent.click(button);

    await waitFor(() => expect(diagnostics.sendDiagnosticsBundle).toHaveBeenCalledOnce());
    expect(await screen.findByText("Logs sent to the developer.")).toBeInTheDocument();

    const cooldownButton = screen.getByRole("button", { name: /Send all logs to developer \(\d+s\)/ });
    expect(cooldownButton).toBeDisabled();

    // Clicking during cooldown must not fire a second request.
    fireEvent.click(cooldownButton);
    expect(diagnostics.sendDiagnosticsBundle).toHaveBeenCalledOnce();

    await vi.advanceTimersByTimeAsync(60_000);
    const reenabled = screen.getByRole("button", { name: "Send all logs to developer" });
    expect(reenabled).not.toBeDisabled();

    fireEvent.click(reenabled);
    await waitFor(() => expect(diagnostics.sendDiagnosticsBundle).toHaveBeenCalledTimes(2));

    vi.useRealTimers();
  });

  it("shows an error message when the upload fails", async () => {
    vi.mocked(diagnostics.sendDiagnosticsBundle).mockResolvedValue({
      ok: false,
      message: "Unable to send logs to the developer.",
    });
    renderPanel();

    fireEvent.click(screen.getByRole("button", { name: "Send all logs to developer" }));

    expect(await screen.findByText("Unable to send logs to the developer.")).toBeInTheDocument();
  });
});

describe("Copy Log", () => {
  it("renders to the left of Send all logs to developer, copies via the content-copy command", async () => {
    vi.mocked(diagnostics.copyDiagnosticsLog).mockResolvedValue("log contents here");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    renderPanel();

    const copyButton = screen.getByRole("button", { name: "Copy Log" });
    const sendButton = screen.getByRole("button", { name: "Send all logs to developer" });
    // "to the left of" -- Copy Log must appear earlier in DOM order within
    // the shared button row than Send all logs to developer.
    expect(copyButton.compareDocumentPosition(sendButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    fireEvent.click(copyButton);

    expect(diagnostics.copyDiagnosticsLog).toHaveBeenCalledOnce();
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("log contents here"));
    expect(await screen.findByRole("button", { name: "Copy Log" })).toBeInTheDocument();
  });

  it("shows an error message when the copy fails", async () => {
    vi.mocked(diagnostics.copyDiagnosticsLog).mockRejectedValue(
      new Error("No backend log is available yet."),
    );
    renderPanel();

    fireEvent.click(screen.getByRole("button", { name: "Copy Log" }));

    expect(await screen.findByTitle("No backend log is available yet.")).toBeInTheDocument();
  });
});

describe("StagePilot panel sections", () => {
  it("shows Status, General configuration, People as headings, then Advanced as a collapsed row", async () => {
    renderPanel();
    await screen.findByRole("checkbox", {name: /Remote Access/});
    const names = screen.getAllByRole("heading", {level: 3}).map((h) => h.textContent);
    // Advanced is a collapsed row (a button with a summary), the guide's layout, so it is not a heading.
    expect(names).toEqual(["Status", "General configuration", "People"]);
    expect(screen.getByRole("button", {name: /^Advanced/})).toHaveAttribute("aria-expanded", "false");
  });

  it("puts the restart-needed fields in the right sections and keeps Release channel in Advanced", async () => {
    renderPanel();
    await screen.findByRole("checkbox", {name: /Remote Access/});
    const section = (name: string) => name === "Advanced"
      ? screen.getByRole("button", {name: /^Advanced/})
      : screen.getByRole("heading", {name});
    const after = (a: HTMLElement, b: HTMLElement) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(after(section("General configuration"), screen.getByLabelText("Timezone"))).toBe(true);
    expect(after(section("General configuration"), screen.getByLabelText("Network access"))).toBe(true);
    expect(after(section("General configuration"), screen.getByRole("checkbox", {name: /PIN/}))).toBe(true);
    expect(after(section("People"), screen.getByRole("checkbox", {name: /Remote Access/}))).toBe(true);
    for (const label of ["Server port", "Log level", "Release channel"]) {
      expect(after(section("Advanced"), screen.getByLabelText(label))).toBe(true);
    }
    expect(after(screen.getByLabelText("Release channel"), screen.getByRole("button", {name: "Save configuration"}))).toBe(true);
  });

  it("People is a plain section: no expander wiring, and the Remote panel stays closed while Remote is off", async () => {
    renderPanel();
    const checkbox = await screen.findByRole("checkbox", {name: /Remote Access/});
    expect(checkbox.closest("label")).not.toHaveAttribute("aria-expanded");
    expect(document.getElementById("remote-access-inline")).toBeNull();
    expect(screen.queryByRole("heading", {name: "Create first Operator"})).not.toBeInTheDocument();
  });
});

describe("Backend panel primitives", () => {
  it("renders as a setup Panel with labelled Field controls", async () => {
    renderPanel();
    const panel = document.getElementById("backend-configuration");
    expect(panel).toHaveAttribute("data-variant", "setup");
    for (const name of ["Timezone", "Server port"]) expect(screen.getByLabelText(name)).toHaveAttribute("data-state", "default");
    expect(screen.getByLabelText("Log level").tagName).toBe("SELECT");
    expect(screen.getByLabelText("Release channel").tagName).toBe("SELECT");
    expect(screen.getByLabelText("Replace dashboard PIN")).toHaveAttribute("type", "password");
    expect(await screen.findByRole("checkbox", {name: /Remote Access/})).toBeInTheDocument();
  });

  it("header close is a 44px secondary Button and the status badge uses the status tone", () => {
    renderPanel();
    const close = screen.getByRole("button", {name: "Close StagePilot configuration"});
    expect(close).toHaveAttribute("data-variant", "secondary");
    expect(close).toHaveAttribute("data-size", "touch");
    expect(close.className).toContain("min-h-ds-touch-target");
    const badge = document.querySelector("[data-status-badge]");
    expect(badge).toHaveAttribute("data-tone", "idle");
    expect(badge).toHaveTextContent("Not connected");
    expect(document.querySelector("[data-setup-panel-header]")!.className).toContain("min-[480px]:grid-cols-");
  });

  it("Save is the primary Button and does nothing with an empty timezone", () => {
    const onSave = vi.fn();
    renderPanel({onSave});
    const save = screen.getByRole("button", {name: "Save configuration"});
    expect(save).toHaveAttribute("data-variant", "primary");
    fireEvent.change(screen.getByLabelText("Timezone"), {target: {value: " "}});
    fireEvent.click(save);
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByText(/Enter a timezone, a port/)).toBeInTheDocument();
  });
});

describe("Save configuration SaveBar", () => {
  const bar = () => document.querySelector("[data-save-bar]")!;
  const withSettings = (extra: Partial<React.ComponentProps<typeof BackendSetupPanel>> = {}) =>
    renderPanel({settings: settingsResponse, ...extra});

  it("has one Save configuration, no old Save button, and starts clean", () => {
    withSettings();
    expect(screen.getAllByRole("button", {name: "Save configuration"})).toHaveLength(1);
    expect(screen.queryByRole("button", {name: "Save general settings"})).not.toBeInTheDocument();
    expect(bar()).toHaveAttribute("data-state", "clean");
    expect(screen.getByText("All changes saved")).toBeInTheDocument();
  });

  it("holds and counts every field, including log level", () => {
    withSettings();
    fireEvent.change(screen.getByLabelText("Timezone"), {target: {value: "UTC"}});
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "9000"}});
    fireEvent.change(screen.getByLabelText("Network access"), {target: {value: "network"}});
    expect(bar()).toHaveAttribute("data-state", "held");
    expect(bar()).toHaveTextContent("3 changes waiting. Not saved yet.");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(3);
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    expect(bar()).toHaveTextContent("4 changes waiting");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(4);
  });

  it("sends a payload byte-identical to the old save for the same edits", () => {
    const onSave = vi.fn();
    withSettings({onSave});
    fireEvent.change(screen.getByLabelText("Timezone"), {target: {value: "UTC"}});
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "9000"}});
    fireEvent.change(screen.getByLabelText("Network access"), {target: {value: "network"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    const expected = {
      timezone: "UTC", log_level: "INFO", server_port: 9000, lan_access: true,
      web_dashboard_pin_enabled: true, release_channel: "BETA",
    };
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0]?.[0]).toEqual(expected);
    expect(JSON.stringify(onSave.mock.calls[0]?.[0])).toBe(JSON.stringify(expected));
  });

  it("a clean Save does nothing", () => {
    const onSave = vi.fn();
    withSettings({onSave});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("Discard restores the saved port, network access and timezone", () => {
    withSettings();
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "9000"}});
    fireEvent.change(screen.getByLabelText("Timezone"), {target: {value: "UTC"}});
    fireEvent.change(screen.getByLabelText("Network access"), {target: {value: "network"}});
    fireEvent.click(screen.getByRole("button", {name: "Discard"}));
    expect(screen.getByLabelText("Server port")).toHaveValue("8765");
    expect(screen.getByLabelText("Timezone")).toHaveValue("America/Los_Angeles");
    expect(screen.getByLabelText("Network access")).toHaveValue("local");
    expect(bar()).toHaveAttribute("data-state", "clean");
  });

  it("shows saving while pending", () => {
    withSettings({pending: true});
    expect(bar()).toHaveAttribute("data-state", "saving");
    expect(bar()).toHaveTextContent("Saving…");
  });

  it("shows Couldn't save with Try again that resends the same payload, keeps restart messaging", () => {
    const onSave = vi.fn();
    const props = {settings: settingsResponse, onSave};
    const view = renderPanel({...props});
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "9000"}});
    view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null} pending {...props} />);
    view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} message={null} pending={false} {...props} error="boom" />);
    expect(bar()).toHaveAttribute("data-state", "error");
    expect(bar()).toHaveTextContent("Couldn't save");
    fireEvent.click(screen.getByRole("button", {name: "Try again"}));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({server_port: 9000}));
    expect(screen.getByText(/take effect after a backend restart/)).toBeInTheDocument();
  });

  it("keeps diagnostics and remote actions as actions", async () => {
    withSettings();
    expect(screen.getByRole("button", {name: "Copy Log"})).toBeInTheDocument();
    expect(screen.getByRole("button", {name: "Send all logs to developer"})).toBeInTheDocument();
    expect(await screen.findByRole("checkbox", {name: /Remote Access/})).toBeInTheDocument();
  });
});

describe("Log level, release channel and PIN are held until Save configuration", () => {
  const bar = () => document.querySelector("[data-save-bar]")!;
  const savedBase = {timezone: "America/Los_Angeles", log_level: "INFO", server_port: 8765, lan_access: false, web_dashboard_pin_enabled: true, release_channel: "BETA"};
  const build = (over: Record<string, unknown> = {}, savedOver: Record<string, unknown> = {}) => (
    <BackendSetupPanel
      state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null}
      pending={false} onSave={vi.fn()} settings={{settings: {...savedBase, ...savedOver}} as unknown as SettingsResponse}
      {...over}
    />
  );
  it("does not save on change, blur or Enter, and counts each field", () => {
    const onSave = vi.fn();
    render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Release channel"), {target: {value: "STABLE"}});
    const pin = screen.getByLabelText("Replace dashboard PIN");
    fireEvent.change(pin, {target: {value: "4321"}});
    fireEvent.blur(pin);
    fireEvent.keyDown(pin, {key: "Enter"});
    expect(onSave).not.toHaveBeenCalled();
    expect(bar()).toHaveTextContent("3 changes waiting. Not saved yet.");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(3);
    fireEvent.click(screen.getByRole("checkbox", {name: /Require a PIN/}));
    expect(onSave).not.toHaveBeenCalled();
    // Turning the PIN requirement off hides the PIN field and drops the typed PIN from the count.
    expect(bar()).toHaveTextContent("3 changes waiting");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(3);
  });

  it("Save configuration sends all of them in one payload", () => {
    const onSave = vi.fn();
    render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Release channel"), {target: {value: "STABLE"}});
    fireEvent.change(screen.getByLabelText("Replace dashboard PIN"), {target: {value: "4321"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith({...savedBase, log_level: "DEBUG", release_channel: "STABLE", web_dashboard_pin: "4321"});
  });

  it("a PIN shorter than 4 characters blocks Save", () => {
    const onSave = vi.fn();
    render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Replace dashboard PIN"), {target: {value: "12"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByText(/PIN of at least 4 characters before saving/)).toBeInTheDocument();
  });

  it("Discard restores log level, release channel, PIN requirement and clears the PIN", () => {
    render(build());
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Release channel"), {target: {value: "STABLE"}});
    fireEvent.change(screen.getByLabelText("Replace dashboard PIN"), {target: {value: "4321"}});
    fireEvent.click(screen.getByRole("checkbox", {name: /Require a PIN/}));
    fireEvent.click(screen.getByRole("button", {name: "Discard"}));
    expect(screen.getByLabelText("Log level")).toHaveValue("INFO");
    expect(screen.getByLabelText("Release channel")).toHaveValue("BETA");
    expect(screen.getByRole("checkbox", {name: /Require a PIN/})).toBeChecked();
    expect(screen.getByLabelText("Replace dashboard PIN")).toHaveValue("");
    expect(bar()).toHaveAttribute("data-state", "clean");
  });

  it("a failed save keeps the typed values and markers", () => {
    const onSave = vi.fn();
    const view = render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Replace dashboard PIN"), {target: {value: "4321"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    view.rerender(build({onSave, pending: true}));
    view.rerender(build({onSave, error: "boom"}));
    expect(screen.getByLabelText("Log level")).toHaveValue("DEBUG");
    expect(screen.getByLabelText("Replace dashboard PIN")).toHaveValue("4321");
    expect(bar()).toHaveAttribute("data-state", "error");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(2);
  });

  it("after a successful save the PIN field clears and the bar is clean", () => {
    const onSave = vi.fn();
    const view = render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Replace dashboard PIN"), {target: {value: "4321"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    view.rerender(build({onSave, pending: true}));
    view.rerender(build({onSave}, {log_level: "DEBUG"}));
    expect(screen.getByLabelText("Replace dashboard PIN")).toHaveValue("");
    expect(bar()).toHaveAttribute("data-state", "auto");
  });
});

describe("StagePilot hold-until-saved conformance (guard)", () => {
  const bar = () => document.querySelector("[data-save-bar]")!;
  const savedBase = {timezone: "America/Los_Angeles", log_level: "INFO", server_port: 8765, lan_access: false, web_dashboard_pin_enabled: true, release_channel: "BETA"};
  const build = (over: Record<string, unknown> = {}) => (
    <BackendSetupPanel
      state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null}
      pending={false} onSave={vi.fn()} settings={{settings: savedBase} as unknown as SettingsResponse}
      {...over}
    />
  );
  const held: Array<{ label: string; value: string; saved: string; payload: Record<string, unknown> }> = [
    { label: "Timezone", value: "America/New_York", saved: "America/Los_Angeles", payload: { timezone: "America/New_York" } },
    { label: "Server port", value: "9000", saved: "8765", payload: { server_port: 9000 } },
    { label: "Log level", value: "DEBUG", saved: "INFO", payload: { log_level: "DEBUG" } },
    { label: "Release channel", value: "STABLE", saved: "BETA", payload: { release_channel: "STABLE" } },
    { label: "Replace dashboard PIN", value: "4321", saved: "", payload: { dashboard_pin: "4321" } },
  ];
  it.each(held)("$label is held: marked, counted, not saved on blur, flagged dirty, restored by Discard, saved only by Save configuration", ({ label, value, saved }) => {
    const onSave = vi.fn();
    const onDirtyChange = vi.fn();
    render(build({ onSave, onDirtyChange }));
    const control = screen.getByLabelText(label);
    fireEvent.change(control, { target: { value } });
    fireEvent.blur(control);
    fireEvent.keyDown(control, { key: "Enter" });
    expect(onSave).not.toHaveBeenCalled();
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(1);
    expect(bar()).toHaveTextContent("1 change waiting. Not saved yet.");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect((screen.getByLabelText(label) as HTMLInputElement).value).toBe(saved);
    expect(bar()).toHaveAttribute("data-state", "clean");
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });
});
