import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  // Status is four boxes: Version, API endpoint, WebSocket and State revision (owner, 2026-10-14). Running since,
  // Connections and the collapsed Connection details row are gone.
  it("shows Version, API endpoint, WebSocket and State revision as the four status boxes", () => {
    renderPanel();
    expect(screen.getByText("Status")).toBeInTheDocument();
    for (const label of ["Version", "API endpoint", "WebSocket", "State revision"]) expect(screen.getByText(label)).toBeInTheDocument();
    for (const gone of ["Running since", "Connections", "Connection details"]) expect(screen.queryByText(gone)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", {name: /Connection details/})).not.toBeInTheDocument();
    expect(screen.queryByText(/\d of 5 connected/)).not.toBeInTheDocument();
    expect(document.querySelectorAll(".backend-status > div")).toHaveLength(4);
  });

  it("says Backend configuration above the title and drops the runtime-identity sentence", () => {
    renderPanel();
    expect(screen.getByText("Backend configuration")).toBeInTheDocument();
    expect(screen.queryByText("Connection configuration")).not.toBeInTheDocument();
    expect(screen.queryByText(/runtime identity/i)).not.toBeInTheDocument();
  });

  it("draws the four facts as small bordered cards, like the style guide, under a kicker-style heading", () => {
    renderPanel();
    expect(screen.getByRole("heading", {level: 3, name: "Status"})).toHaveClass("uppercase", "text-ink-muted");
    for (const label of ["Version", "API endpoint", "WebSocket", "State revision"]) {
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
});

describe("Internet access (Connection Access switch)", () => {
  const internet = () => screen.findByRole("radio", {name: "Internet"});
  const on = {...off, enabled: true, state: "connected" as const, needs_operator: false, url: "https://test.trycloudflare.com"};
  const withLan = (lan: boolean) => ({settings: {...saved, lan_access: lan}} as unknown as SettingsResponse);

  it("choosing Internet enables Remote immediately even with no Operator yet, then shows the bootstrap UI, and turns the local network on too", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue(off);
    vi.mocked(api.setRemoteEnabled).mockImplementation(async (enabled) => {
      const next = {...off, enabled, state: enabled ? "connected" as const : "off" as const, needs_operator: true};
      vi.mocked(api.getRemoteStatus).mockResolvedValue(next);
      return next;
    });
    const onSave = vi.fn();
    renderPanel({settings: settingsResponse, onSave});
    const radio = await internet();
    expect(radio).not.toBeChecked();

    fireEvent.click(radio);
    await waitFor(() => expect(api.setRemoteEnabled).toHaveBeenCalledWith(true));
    expect(desktop.setRemoteAutostart).toHaveBeenCalledWith(true);
    await waitFor(() => expect(radio).toBeChecked());

    // Internet implies the local network: it is saved at once, with the PIN requirement left on.
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0]?.[0]).toMatchObject({lan_access: true, web_dashboard_pin_enabled: true});

    // Enabled, but still no Operator: the bootstrap form is a required next step and sits under People.
    expect(await screen.findByRole("heading", {name: "Create first Operator"})).toBeInTheDocument();
    expect(screen.getByText(/Remote Access is running\. Create the first Operator/)).toBeInTheDocument();
    expect(screen.getByRole("heading", {name: "People"})).toBeInTheDocument();

    // Creating the first Operator must NOT call setRemoteEnabled again (it's already enabled) -- only bootstrapRemote itself.
    vi.mocked(api.bootstrapRemote).mockResolvedValue(operator);
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    fireEvent.change(screen.getByLabelText("Email"), {target: {value: operator.email}});
    fireEvent.change(screen.getByLabelText("Password"), {target: {value: "long-test-password"}});
    fireEvent.click(screen.getByRole("button", {name: "Create Operator"}));
    await waitFor(() => expect(api.bootstrapRemote).toHaveBeenCalledWith(operator.email, "long-test-password"));
    expect(api.setRemoteEnabled).toHaveBeenCalledTimes(1);
  });

  it("does not save the network again when Internet is chosen and the local network is already on", async () => {
    vi.mocked(api.setRemoteEnabled).mockImplementation(async (enabled) => {
      vi.mocked(api.getRemoteStatus).mockResolvedValue({...on, enabled});
      return {...on, enabled};
    });
    const onSave = vi.fn();
    renderPanel({settings: withLan(true), onSave});
    fireEvent.click(await internet());
    await waitFor(() => expect(api.setRemoteEnabled).toHaveBeenCalledWith(true));
    await waitFor(() => expect(screen.getByRole("radio", {name: "Internet"})).toBeChecked());
    expect(onSave).not.toHaveBeenCalled();
  });

  it("leaving Internet asks first, then turns Remote off and collapses People", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue(on);
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    vi.mocked(api.setRemoteEnabled).mockImplementation(async (enabled) => {
      const next = {...off, enabled, state: enabled ? "connected" as const : "off" as const, needs_operator: false, url: enabled ? "https://test.trycloudflare.com" : null};
      vi.mocked(api.getRemoteStatus).mockResolvedValue(next);
      return next;
    });
    const onSave = vi.fn();
    renderPanel({settings: withLan(true), onSave});
    const radio = await internet();
    await waitFor(() => expect(radio).toBeChecked());
    expect(screen.getByRole("heading", {name: "People"})).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", {name: "Host"}));
    expect(await screen.findByRole("group", {name: "Confirm disable Remote Access"})).toBeInTheDocument();
    expect(api.setRemoteEnabled).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", {name: "Confirm disable"}));
    await waitFor(() => expect(api.setRemoteEnabled).toHaveBeenCalledWith(false));
    expect(desktop.setRemoteAutostart).toHaveBeenCalledWith(false);
    await waitFor(() => expect(screen.getByRole("radio", {name: "Host"})).toBeChecked());
    expect(screen.queryByRole("heading", {name: "People"})).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Remote Access", {selector: "div"})).not.toBeInTheDocument();
    // Going back to Host also takes the local network away again, in the same flow.
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0]?.[0]).toMatchObject({lan_access: false});
  });

  it("leaving Internet for Local network keeps the local network on and saves nothing else", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue(on);
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    vi.mocked(api.setRemoteEnabled).mockImplementation(async (enabled) => {
      vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled, state: "off", needs_operator: false});
      return {...off, enabled};
    });
    const onSave = vi.fn();
    renderPanel({settings: withLan(true), onSave});
    await waitFor(async () => expect(await internet()).toBeChecked());
    fireEvent.click(screen.getByRole("radio", {name: "Local network"}));
    fireEvent.click(await screen.findByRole("button", {name: "Confirm disable"}));
    await waitFor(() => expect(screen.getByRole("radio", {name: "Local network"})).toBeChecked());
    expect(onSave).not.toHaveBeenCalled();
  });

  it("cancelling the disable confirmation keeps Internet chosen and changes nothing", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue(on);
    const onSave = vi.fn();
    renderPanel({settings: withLan(true), onSave});
    const radio = await internet();
    await waitFor(() => expect(radio).toBeChecked());

    fireEvent.click(screen.getByRole("radio", {name: "Host"}));
    expect(await screen.findByRole("group", {name: "Confirm disable Remote Access"})).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", {name: "Cancel"}));
    expect(api.setRemoteEnabled).not.toHaveBeenCalled();
    expect(radio).toBeChecked();
    expect(screen.queryByRole("group", {name: "Confirm disable Remote Access"})).not.toBeInTheDocument();
    expect(onSave).not.toHaveBeenCalled();
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
  const after = (a: HTMLElement, b: HTMLElement) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

  it("shows Status as the only heading while Internet is off, then Connection Access, then Advanced as a collapsed row", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    // People now lives under Connection Access and only when Internet is chosen. Advanced is a collapsed row, not a heading.
    expect(screen.getAllByRole("heading", {level: 3}).map((h) => h.textContent)).toEqual(["Status"]);
    expect(screen.getByRole("button", {name: /^Advanced/})).toHaveAttribute("aria-expanded", "false");
  });

  it("puts Timezone, then Connection Access, then Advanced, with the restart-needed fields in Advanced", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    const advanced = screen.getByRole("button", {name: /^Advanced/});
    const access = screen.getByText("Connection Access");
    expect(after(screen.getByRole("heading", {name: "Status"}), screen.getByLabelText("Timezone"))).toBe(true);
    expect(after(screen.getByLabelText("Timezone"), access)).toBe(true);
    expect(after(access, advanced)).toBe(true);
    for (const label of ["Server port", "Log level", "Release channel"]) expect(after(advanced, screen.getByLabelText(label))).toBe(true);
    expect(after(screen.getByLabelText("Release channel"), screen.getByRole("button", {name: "Save configuration"}))).toBe(true);
  });

  it("has no Network access dropdown, no Require a PIN checkbox and no Remote Access checkbox any more", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    expect(screen.queryByLabelText("Network access")).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByText(/Require a PIN/)).not.toBeInTheDocument();
    expect(screen.queryByText("This computer only")).not.toBeInTheDocument();
  });

  it("the Remote panel and People stay closed while Internet is off", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    expect(document.getElementById("remote-access-inline")).toBeNull();
    expect(screen.queryByRole("heading", {name: "Create first Operator"})).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", {name: "People"})).not.toBeInTheDocument();
  });
});

describe("Connection Access switch (owner 2026-10-14)", () => {
  const withLan = (lan: boolean, extra: Partial<React.ComponentProps<typeof BackendSetupPanel>> = {}) =>
    renderPanel({settings: {settings: {...saved, lan_access: lan}} as unknown as SettingsResponse, ...extra});
  const names = () => within(screen.getByRole("radiogroup", {name: "Connection Access"})).getAllByRole("radio").map((r) => (r as HTMLInputElement).getAttribute("aria-labelledby"));

  it("is the Playback connection-method switch titled Connection Access, with Host, Local network and Internet and no PRIMARY/ALTERNATE words", () => {
    withLan(false);
    const group = screen.getByRole("radiogroup", {name: "Connection Access"});
    expect(document.querySelector("[data-connection-method]")).toBeInTheDocument();
    expect(screen.queryByText("Connection method")).not.toBeInTheDocument();
    expect(within(group).getAllByRole("radio").map((r) => (r as HTMLInputElement).value)).toEqual(["host", "network", "internet"]);
    expect(within(group).getByText("Host")).toBeInTheDocument();
    expect(within(group).getByText("Local network")).toBeInTheDocument();
    expect(within(group).getByText("Internet")).toBeInTheDocument();
    expect(within(group).queryByText(/primary|alternate/i)).not.toBeInTheDocument();
    expect(names().every((id) => id && !id.includes("-role"))).toBe(true);
  });

  it("shows the saved choice: Host when only this computer, Local network when other devices can connect", () => {
    withLan(false);
    expect(screen.getByRole("radio", {name: "Host"})).toBeChecked();
    cleanup();
    withLan(true);
    expect(screen.getByRole("radio", {name: "Local network"})).toBeChecked();
  });

  it("saves the moment Local network is chosen: one save, PIN requirement on, nothing waiting on Save configuration", () => {
    const onSave = vi.fn();
    withLan(false, {onSave});
    fireEvent.click(screen.getByRole("radio", {name: "Local network"}));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0]?.[0]).toEqual({
      timezone: "America/Los_Angeles", log_level: "INFO", server_port: 8765, lan_access: true,
      web_dashboard_pin_enabled: true, release_channel: "BETA",
    });
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "clean");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(0);
    expect(screen.getByRole("radio", {name: "Local network"})).toBeChecked();
    expect(screen.getByRole("status")).toHaveTextContent(/Restart StagePilot to let other devices connect/);
  });

  it("saves Host the moment it is chosen from Local network", () => {
    const onSave = vi.fn();
    withLan(true, {onSave});
    fireEvent.click(screen.getByRole("radio", {name: "Host"}));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0]?.[0]).toMatchObject({lan_access: false});
  });

  it("choosing the option already shown saves nothing", () => {
    const onSave = vi.fn();
    withLan(true, {onSave});
    fireEvent.click(screen.getByRole("radio", {name: "Local network"}));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("saves only the access change: other fields being edited stay held, not saved by accident", () => {
    const onSave = vi.fn();
    withLan(false, {onSave});
    fireEvent.change(screen.getByLabelText("Timezone"), {target: {value: "UTC"}});
    fireEvent.click(screen.getByRole("radio", {name: "Local network"}));
    expect(onSave.mock.calls[0]?.[0]).toMatchObject({timezone: "America/Los_Angeles", lan_access: true});
    expect(screen.getByLabelText("Timezone")).toHaveValue("UTC");
    expect(document.querySelector("[data-save-bar]")).toHaveTextContent("1 change waiting");
  });

  it("is locked until the settings have loaded and while a save is in flight", () => {
    renderPanel();
    expect(screen.getByRole("radiogroup", {name: "Connection Access"}).closest("fieldset")).toBeDisabled();
    cleanup();
    withLan(false, {pending: true});
    expect(screen.getByRole("radiogroup", {name: "Connection Access"}).closest("fieldset")).toBeDisabled();
  });

  it("goes back to what is really saved when the save fails", () => {
    const props = {settings: {settings: {...saved, lan_access: false}} as unknown as SettingsResponse, onSave: vi.fn()};
    const view = renderPanel(props);
    fireEvent.click(screen.getByRole("radio", {name: "Local network"}));
    view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null} pending {...props} />);
    view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} message={null} pending={false} {...props} error="boom" />);
    expect(screen.getByRole("radio", {name: "Host"})).toBeChecked();
  });

  describe("the PIN and People appear only when they apply", () => {
    it("Host: no PIN field, no People", () => {
      withLan(false);
      expect(screen.queryByLabelText("Replace dashboard PIN")).not.toBeInTheDocument();
      expect(screen.queryByRole("heading", {name: "People"})).not.toBeInTheDocument();
    });

    it("Local network: the PIN field, still no People", () => {
      withLan(true);
      expect(screen.getByLabelText("Replace dashboard PIN")).toHaveAttribute("type", "password");
      expect(screen.getByText(/The initial PIN is 1234/)).toBeInTheDocument();
      expect(screen.queryByRole("heading", {name: "People"})).not.toBeInTheDocument();
    });

    it("Internet: the PIN field (the local network is on too) and People, both directly under the switch and above Advanced", async () => {
      vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
      vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
      withLan(true);
      const people = await screen.findByRole("heading", {name: "People"});
      const pin = screen.getByLabelText("Replace dashboard PIN");
      const order = (a: HTMLElement, b: HTMLElement) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
      expect(order(screen.getByRole("radiogroup", {name: "Connection Access"}), pin)).toBe(true);
      expect(order(pin, people)).toBe(true);
      expect(order(people, screen.getByRole("button", {name: /^Advanced/}))).toBe(true);
    });
  });

  describe("Save PIN", () => {
    it("replaces the PIN on its own, keeps the PIN requirement on, and clears the field once saved", () => {
      const onSave = vi.fn();
      const props = {settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse, onSave};
      const view = renderPanel(props);
      const pin = screen.getByLabelText("Replace dashboard PIN");
      fireEvent.change(pin, {target: {value: "4321"}});
      expect(onSave).not.toHaveBeenCalled(); // typing never saves
      fireEvent.click(screen.getByRole("button", {name: "Save PIN"}));
      expect(onSave).toHaveBeenCalledTimes(1);
      expect(onSave.mock.calls[0]?.[0]).toMatchObject({lan_access: true, web_dashboard_pin_enabled: true, web_dashboard_pin: "4321"});
      view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null} pending {...props} />);
      view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null} pending={false} {...props} />);
      expect(screen.getByLabelText("Replace dashboard PIN")).toHaveValue("");
    });

    it("Enter in the field saves too", () => {
      const onSave = vi.fn();
      renderPanel({settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse, onSave});
      const pin = screen.getByLabelText("Replace dashboard PIN");
      fireEvent.change(pin, {target: {value: "9999"}});
      fireEvent.keyDown(pin, {key: "Enter"});
      expect(onSave).toHaveBeenCalledWith(expect.objectContaining({web_dashboard_pin: "9999"}));
    });

    it("a PIN shorter than 4 characters cannot be saved", () => {
      const onSave = vi.fn();
      renderPanel({settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse, onSave});
      fireEvent.change(screen.getByLabelText("Replace dashboard PIN"), {target: {value: "12"}});
      expect(screen.getByRole("button", {name: "Save PIN"})).toBeDisabled();
      expect(screen.getByText("Enter a PIN of at least 4 characters.")).toBeInTheDocument();
      fireEvent.keyDown(screen.getByLabelText("Replace dashboard PIN"), {key: "Enter"});
      expect(onSave).not.toHaveBeenCalled();
    });
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
    expect(await screen.findByRole("radio", {name: "Internet"})).toBeInTheDocument();
  });

  it("header close is a 44px secondary Button and the status badge uses the status tone", () => {
    renderPanel();
    const close = screen.getByRole("button", {name: "Close StagePilot configuration"});
    expect(close).toHaveAttribute("data-variant", "secondary");
    expect(close).toHaveAttribute("data-size", "touch");
    expect(close.className).toContain("min-h-ds-touch-target");
    const badge = document.querySelector("[data-status-badge]");
    expect(badge).toHaveAttribute("data-tone", "idle");
    expect(badge).toHaveTextContent("Disconnected");
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
    expect(screen.getByText(/Enter a timezone and a port/)).toBeInTheDocument();
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
    expect(bar()).toHaveAttribute("data-state", "held");
    expect(bar()).toHaveTextContent("2 changes waiting. Not saved yet.");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(2);
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    expect(bar()).toHaveTextContent("3 changes waiting");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(3);
  });

  it("sends a payload byte-identical to the old save for the same edits", () => {
    const onSave = vi.fn();
    withSettings({onSave});
    fireEvent.change(screen.getByLabelText("Timezone"), {target: {value: "UTC"}});
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "9000"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    const expected = {
      timezone: "UTC", log_level: "INFO", server_port: 9000, lan_access: false,
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

  it("Discard restores the saved port and timezone", () => {
    withSettings();
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "9000"}});
    fireEvent.change(screen.getByLabelText("Timezone"), {target: {value: "UTC"}});
    fireEvent.click(screen.getByRole("button", {name: "Discard"}));
    expect(screen.getByLabelText("Server port")).toHaveValue("8765");
    expect(screen.getByLabelText("Timezone")).toHaveValue("America/Los_Angeles");
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

  it("keeps diagnostics and the Connection Access switch available", async () => {
    withSettings();
    expect(screen.getByRole("button", {name: "Copy Log"})).toBeInTheDocument();
    expect(screen.getByRole("button", {name: "Send all logs to developer"})).toBeInTheDocument();
    expect(await screen.findByRole("radio", {name: "Internet"})).toBeInTheDocument();
  });
});

describe("Log level and release channel are held until Save configuration", () => {
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
    const channel = screen.getByLabelText("Release channel");
    fireEvent.change(channel, {target: {value: "STABLE"}});
    fireEvent.blur(channel);
    fireEvent.keyDown(channel, {key: "Enter"});
    expect(onSave).not.toHaveBeenCalled();
    expect(bar()).toHaveTextContent("2 changes waiting. Not saved yet.");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(2);
  });

  it("Save configuration sends both in one payload, with the PIN requirement always on and no PIN value", () => {
    const onSave = vi.fn();
    render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Release channel"), {target: {value: "STABLE"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith({...savedBase, log_level: "DEBUG", release_channel: "STABLE"});
  });

  it("the PIN requirement is always sent as on, even if an older saved file had it off", () => {
    const onSave = vi.fn();
    render(build({onSave}, {web_dashboard_pin_enabled: false}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    expect(onSave.mock.calls[0]?.[0]).toMatchObject({web_dashboard_pin_enabled: true});
  });

  it("Discard restores log level and release channel", () => {
    render(build());
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Release channel"), {target: {value: "STABLE"}});
    fireEvent.click(screen.getByRole("button", {name: "Discard"}));
    expect(screen.getByLabelText("Log level")).toHaveValue("INFO");
    expect(screen.getByLabelText("Release channel")).toHaveValue("BETA");
    expect(bar()).toHaveAttribute("data-state", "clean");
  });

  it("a failed save keeps the typed values and markers", () => {
    const onSave = vi.fn();
    const view = render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.change(screen.getByLabelText("Release channel"), {target: {value: "STABLE"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    view.rerender(build({onSave, pending: true}));
    view.rerender(build({onSave, error: "boom"}));
    expect(screen.getByLabelText("Log level")).toHaveValue("DEBUG");
    expect(screen.getByLabelText("Release channel")).toHaveValue("STABLE");
    expect(bar()).toHaveAttribute("data-state", "error");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(2);
  });

  it("after a successful save the bar is clean", () => {
    const onSave = vi.fn();
    const view = render(build({onSave}));
    fireEvent.change(screen.getByLabelText("Log level"), {target: {value: "DEBUG"}});
    fireEvent.click(screen.getByRole("button", {name: "Save configuration"}));
    view.rerender(build({onSave, pending: true}));
    view.rerender(build({onSave}, {log_level: "DEBUG"}));
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
