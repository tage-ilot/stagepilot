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
  // Four boxes in this order: Version, State revision, API endpoint, WebSocket (owner, 2026-10-14). The Status heading is gone;
  // the group is still labelled "Status" for assistive technology.
  it("shows Version, State revision, API endpoint and WebSocket as the four status boxes, in that order, with no Status heading", () => {
    renderPanel();
    expect(screen.queryByRole("heading", {name: "Status"})).not.toBeInTheDocument();
    expect(screen.getByLabelText("Status")).toBeInTheDocument();
    const order = Array.from(document.querySelectorAll(".backend-status > div dt")).map((dt) => dt.textContent);
    expect(order).toEqual(["Version", "State revision", "API endpoint", "WebSocket"]);
    for (const gone of ["Running since", "Connections", "Connection details"]) expect(screen.queryByText(gone)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", {name: /Connection details/})).not.toBeInTheDocument();
    expect(screen.queryByText(/\d of 5 connected/)).not.toBeInTheDocument();
    expect(document.querySelectorAll(".backend-status > div")).toHaveLength(4);
  });

  it("keeps some of the space under the header that the removed Status heading used to take: half of its 52.5px, so 26px", () => {
    renderPanel();
    expect(document.querySelector(".backend-status-wrap")).toHaveClass("mt-[26px]");
  });

  it("says Backend configuration above the title and drops the runtime-identity sentence", () => {
    renderPanel();
    expect(screen.getByText("Backend configuration")).toBeInTheDocument();
    expect(screen.queryByText("Connection configuration")).not.toBeInTheDocument();
    expect(screen.queryByText(/runtime identity/i)).not.toBeInTheDocument();
  });

  it("draws the four facts as small bordered cards, like the style guide", () => {
    renderPanel();
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

    // Enabled, but still no Operator: the bootstrap form is a required next step and sits under Remote access.
    expect(await screen.findByRole("heading", {name: "Create first Operator"})).toBeInTheDocument();
    expect(screen.getByText(/Remote Access is running\. Create the first Operator/)).toBeInTheDocument();
    expect(screen.getByRole("button", {name: /^Remote access/})).toBeInTheDocument();

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

  it("leaving Internet asks first, then turns Remote off and collapses Remote access", async () => {
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
    expect(screen.getByRole("button", {name: /^Remote access/})).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", {name: "Host"}));
    expect(await screen.findByRole("group", {name: "Confirm disable Remote Access"})).toBeInTheDocument();
    expect(api.setRemoteEnabled).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", {name: "Confirm disable"}));
    await waitFor(() => expect(api.setRemoteEnabled).toHaveBeenCalledWith(false));
    expect(desktop.setRemoteAutostart).toHaveBeenCalledWith(false);
    await waitFor(() => expect(screen.getByRole("radio", {name: "Host"})).toBeChecked());
    expect(screen.queryByRole("button", {name: /^Remote access/})).not.toBeInTheDocument();
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

  it("has no section headings while Internet is off: the status cards, Advanced, then Connection Access", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    // Remote access (the old People section) lives under Connection Access and only when Internet is chosen. Advanced is a collapsed row.
    expect(screen.queryAllByRole("heading", {level: 3})).toHaveLength(0);
    expect(screen.getByRole("button", {name: /^Advanced/})).toHaveAttribute("aria-expanded", "false");
  });

  it("puts the status cards, then Advanced (Timezone first), then Connection Access, then Save configuration", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    const cards = screen.getByLabelText("Status");
    const advanced = screen.getByRole("button", {name: /^Advanced/});
    const access = screen.getByText("Connection Access");
    expect(after(cards, advanced)).toBe(true);
    expect(after(advanced, access)).toBe(true);
    const labels = Array.from(document.querySelectorAll(".backend-advanced label")).map((l) => l.textContent);
    expect(labels).toEqual(["Timezone", "Server port", "Log level", "Release channel"]);
    expect(document.querySelector(".backend-advanced")).toContainElement(screen.getByLabelText("Timezone"));
    for (const label of ["Timezone", "Server port", "Log level", "Release channel"]) expect(after(advanced, screen.getByLabelText(label))).toBe(true);
    expect(after(access, screen.getByRole("button", {name: "Save configuration"}))).toBe(true);
  });

  it("lays Advanced out 1, 2 or 4 across by container width, switching with the status cards and never at 3", async () => {
    // vitest blanks CSS imports, so read the stylesheet from disk (no @types/node in the app tsconfig).
    const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
    const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
    for (const klass of ["backend-status", "backend-advanced"]) {
      const columns = [...css.matchAll(new RegExp(`\\.${klass} \\{\\s*grid-template-columns: ([^;]+);`, "g"))].map((m) => m[1]);
      expect(columns).toEqual(["minmax(0, 1fr)", "repeat(2, minmax(0, 1fr))", "repeat(4, minmax(0, 1fr))"]);
    }
    expect(css).toMatch(/\.backend-advanced-wrap \{\s*container-type: inline-size;/);
  });

  it("has no Network access dropdown, no Require a PIN checkbox and no Remote Access checkbox any more", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    expect(screen.queryByLabelText("Network access")).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByText(/Require a PIN/)).not.toBeInTheDocument();
    expect(screen.queryByText("This computer only")).not.toBeInTheDocument();
  });

  it("the Remote panel stays closed while Internet is off", async () => {
    renderPanel({settings: settingsResponse});
    await screen.findByRole("radio", {name: "Internet"});
    expect(document.getElementById("remote-access-inline")).toBeNull();
    expect(screen.queryByRole("heading", {name: "Create first Operator"})).not.toBeInTheDocument();
    expect(screen.queryByRole("button", {name: /^Remote access/})).not.toBeInTheDocument();
  });
});

describe("Remote access section (owner 2026-10-14)", () => {
  it("is titled Remote access, drops the 'Let people sign in' sentence, and keeps the link and the people in two separate groups", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    renderPanel({settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse});
    expect(await screen.findByRole("button", {name: /^Remote access/})).toBeInTheDocument();
    expect(screen.queryByText(/Let people sign in from a phone/)).not.toBeInTheDocument();

    const link = await screen.findByRole("group", {name: "Remote link"});
    const people = screen.getByRole("group", {name: "Remote users"});
    expect(link).not.toContainElement(people);
    expect(people).not.toContainElement(link);

    // Group one: the link and its status.
    expect(within(link).getByText(/Securely view or operate this StagePilot from another device/)).toBeInTheDocument();
    expect(within(link).getByText("Connected")).toBeInTheDocument();
    expect(within(link).getByText(/Stable Remote link|Temporary Remote link/)).toBeInTheDocument();
    expect(within(link).getByRole("button", {name: "Copy link"})).toBeInTheDocument();
    expect(within(link).getByRole("button", {name: "Regenerate Remote link"})).toBeInTheDocument();
    expect(within(link).queryByText(/Viewers are read-only/)).not.toBeInTheDocument();

    // Group two: the people, with the shorter sentence.
    expect(within(people).getByText("Viewers are read-only. Operators can control StagePilot.")).toBeInTheDocument();
    expect(screen.queryByText(/manage users\. Keep at least one/)).not.toBeInTheDocument();
    expect(within(people).getByText(/operator@example\.test · Enabled/)).toBeInTheDocument();
    expect(within(people).getByRole("button", {name: "Delete operator@example.test"})).toBeInTheDocument();
    expect(within(people).getByRole("button", {name: "Add a person"})).toBeInTheDocument();
    expect(within(people).queryByRole("button", {name: "Copy link"})).not.toBeInTheDocument();
  });
});

describe("Server port restart prompt (owner 2026-10-14)", () => {
  it("drops the 'dashboard remembers the saved server port' footnote", () => {
    renderPanel({settings: settingsResponse});
    expect(screen.queryByText(/remembers the saved server port/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Environment variables may still override/)).not.toBeInTheDocument();
  });

  it("tells the user to restart as soon as the port is changed, and not before", () => {
    renderPanel({settings: settingsResponse});
    fireEvent.click(screen.getByRole("button", {name: /^Advanced/}));
    expect(screen.queryByText("Takes effect after you restart StagePilot.")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "9000"}});
    expect(screen.getByText("Takes effect after you restart StagePilot.")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Server port"), {target: {value: "8765"}});
    expect(screen.queryByText("Takes effect after you restart StagePilot.")).not.toBeInTheDocument();
  });

  it("still shows the restart message after the port is saved", () => {
    renderPanel({settings: settingsResponse, message: "General configuration saved. Restart StagePilot and reload the dashboard to apply startup changes."});
    expect(screen.getByText(/Restart StagePilot and reload the dashboard/)).toBeInTheDocument();
  });
});

describe("Remote access collapses and remembers it (owner 2026-10-14)", () => {
  const KEY = "stagepilot.backend.remote-access-open";
  const on = {...off, enabled: true, state: "connected" as const, needs_operator: false, url: "https://test.trycloudflare.com"};
  const open = () => renderPanel({settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse});
  const toggle = () => screen.getByRole("button", {name: /^Remote access/});
  beforeEach(() => {
    window.localStorage.removeItem(KEY);
    vi.mocked(api.getRemoteStatus).mockResolvedValue(on);
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
  });

  it("starts expanded the first time, with nothing saved yet", async () => {
    open();
    await screen.findByRole("button", {name: /^Remote access/});
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(window.localStorage.getItem(KEY)).toBeNull();
    expect(await screen.findByRole("group", {name: "Remote link"})).toBeInTheDocument();
  });

  it("collapses on click, shows the state on the closed row, and saves the choice", async () => {
    open();
    await screen.findByRole("group", {name: "Remote link"});
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(window.localStorage.getItem(KEY)).toBe("false");
    expect(toggle()).toHaveTextContent("Connected");
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(window.localStorage.getItem(KEY)).toBe("true");
  });

  it("opens the way it was left after a restart (a fresh mount), collapsed or expanded", async () => {
    window.localStorage.setItem(KEY, "false");
    open();
    await screen.findByRole("button", {name: /^Remote access/});
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    cleanup();
    window.localStorage.setItem(KEY, "true");
    open();
    await screen.findByRole("button", {name: /^Remote access/});
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
  });

  it("opens by itself to ask whether to turn Remote Access off, without changing the saved choice", async () => {
    window.localStorage.setItem(KEY, "false");
    open();
    await screen.findByRole("button", {name: /^Remote access/});
    await waitFor(() => expect(screen.getByRole("radio", {name: "Internet"})).toBeChecked());
    fireEvent.click(screen.getByRole("radio", {name: "Host"}));
    expect(await screen.findByRole("group", {name: "Confirm disable Remote Access"})).toBeInTheDocument();
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(window.localStorage.getItem(KEY)).toBe("false");
    fireEvent.click(screen.getByRole("button", {name: "Cancel"}));
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("still works when storage is unavailable: starts expanded and the choice lasts for the session", async () => {
    const original = Storage.prototype.getItem;
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("blocked"); });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("blocked"); });
    open();
    await screen.findByRole("button", {name: /^Remote access/});
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    vi.restoreAllMocks();
    Storage.prototype.getItem = original;
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

  describe("the PIN and Remote access appear only when they apply", () => {
    it("Host: no PIN field, no Remote access", () => {
      withLan(false);
      expect(screen.queryByLabelText("Access PIN")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", {name: /^Remote access/})).not.toBeInTheDocument();
    });

    it("Local network: the PIN field, still no Remote access", () => {
      withLan(true);
      expect(screen.getByLabelText("Access PIN")).toHaveAttribute("type", "password");
      expect(screen.getByText(/The initial PIN is 1234/)).toBeInTheDocument();
      expect(screen.queryByRole("button", {name: /^Remote access/})).not.toBeInTheDocument();
    });

    it("Internet: the PIN field (the local network is on too) and Remote access, both directly under the switch, after Advanced", async () => {
      vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
      vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
      withLan(true);
      const people = await screen.findByRole("button", {name: /^Remote access/});
      const pin = screen.getByLabelText("Access PIN");
      const order = (a: HTMLElement, b: HTMLElement) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
      expect(order(screen.getByRole("radiogroup", {name: "Connection Access"}), pin)).toBe(true);
      expect(order(pin, people)).toBe(true);
      // Advanced now sits under the status cards, above Connection Access, so Connection Access and what hangs off it come last.
      expect(order(screen.getByRole("button", {name: /^Advanced/}), screen.getByRole("radiogroup", {name: "Connection Access"}))).toBe(true);
    });
  });

  describe("Save PIN", () => {
    it("replaces the PIN on its own, keeps the PIN requirement on, and clears the field once saved", () => {
      const onSave = vi.fn();
      const props = {settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse, onSave};
      const view = renderPanel(props);
      const pin = screen.getByLabelText("Access PIN");
      fireEvent.change(pin, {target: {value: "4321"}});
      expect(onSave).not.toHaveBeenCalled(); // typing never saves
      fireEvent.click(screen.getByRole("button", {name: "Save PIN"}));
      expect(onSave).toHaveBeenCalledTimes(1);
      expect(onSave.mock.calls[0]?.[0]).toMatchObject({lan_access: true, web_dashboard_pin_enabled: true, web_dashboard_pin: "4321"});
      view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null} pending {...props} />);
      view.rerender(<BackendSetupPanel state={state} health={null} live={false} onClose={vi.fn()} error={null} message={null} pending={false} {...props} />);
      expect(screen.getByLabelText("Access PIN")).toHaveValue("");
    });

    it("Enter in the field saves too", () => {
      const onSave = vi.fn();
      renderPanel({settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse, onSave});
      const pin = screen.getByLabelText("Access PIN");
      fireEvent.change(pin, {target: {value: "9999"}});
      fireEvent.keyDown(pin, {key: "Enter"});
      expect(onSave).toHaveBeenCalledWith(expect.objectContaining({web_dashboard_pin: "9999"}));
    });

    it("a PIN shorter than 4 characters cannot be saved", () => {
      const onSave = vi.fn();
      renderPanel({settings: {settings: {...saved, lan_access: true}} as unknown as SettingsResponse, onSave});
      fireEvent.change(screen.getByLabelText("Access PIN"), {target: {value: "12"}});
      expect(screen.getByRole("button", {name: "Save PIN"})).toBeDisabled();
      expect(screen.getByText("Enter a PIN of at least 4 characters.")).toBeInTheDocument();
      fireEvent.keyDown(screen.getByLabelText("Access PIN"), {key: "Enter"});
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
