import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { playbackSettings, playbackState, playbackStatus } from "../test/playbackFixtures";
import { MidiSetupPanel } from "./MidiSetupPanel";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { PlaybackSaveBar, usePlaybackDraft } from "./PlaybackSaveFooter";
import { usePlaybackSave, type PlaybackMidiStep } from "./usePlaybackSave";
import { SONG_ORDER_OPEN_KEY, validPlaybackHost } from "./playbackStatus";
import { SetupChecklist } from "./SetupChecklist";
import { buildConnectionCardViews, buildReadinessChecks } from "./dashboard/dashboardReadiness";

const noMidi: PlaybackMidiStep = { pending: false, count: 0, valid: true, snapshot: "", apply: async () => true, revert: () => undefined };
const scanOf = (state: "idle" | "scanning" | "found" | "not_found", extra = {}) => ({ state, candidates: [], reason: null, current: 0, total: 0, ...extra });
const none = (overrides = {}) => playbackStatus({
  connected: false, active_source: "none", reason: "Disconnected. Choose Scan Network to find Playback.",
  sources: { playback_api: { connected: false, reason: "Playback is disconnected." }, midi: { connected: false, reason: "MIDI is disconnected." } },
  ...overrides,
});
const controller = (overrides: Partial<PlaybackController> = {}): PlaybackController => ({
  status: playbackStatus(), events: [], error: null, message: null, pending: null, saveArrangement: vi.fn(async () => true),
  save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), ...overrides,
});
const panel = (playback = controller(), extra: { onDirtyChange?: (dirty: boolean) => void } = {}) => render(<MidiSetupPanel
  playback={playback} midi={null} messages={[]} error={null} message={null}
  pendingOperation={null} onRefresh={vi.fn()} onSelect={vi.fn()}
  settings={playbackSettings} onSaveSettings={vi.fn()} {...extra}
/>);

describe("Scan is always first and never dead", () => {
  it.each([
    ["fresh install, nothing configured", none({ selected: true, settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true } })],
    ["MIDI-selected install", none({ selected: false, enabled: false, active_source: "midi", connected: true, reason: "Connected through MIDI.", settings: { enabled: false, host: null, port: 8080, auto_scan: true, fast_transport: true } })],
    ["Playback switched off", none({ enabled: false, settings: { enabled: false, host: null, port: 8080, auto_scan: true, fast_transport: true } })],
  ])("%s", (_name, status) => {
    const playback = controller({ status });
    panel(playback);
    const scan = screen.getByRole("button", { name: "Search" });
    expect(scan).toBeEnabled();
    // (The little arrow inside the address box is not a tab stop: Scan is still the first button a person reaches.)
    const primary = screen.getAllByRole("button").find((b) => !/Close/.test(b.textContent ?? "") && !/Close/.test(b.getAttribute("aria-label") ?? "") && b.tabIndex !== -1);
    expect(primary).toBe(scan);
    fireEvent.click(scan);
    expect(playback.scan).toHaveBeenCalledWith();
  });

  it("shows scanning with progress and a working cancel, then returns to Scan", () => {
    const playback = controller({ status: none({ scan: scanOf("scanning", { current: 64, total: 254 }), reason: "Looking for Playback…" }), pending: "scan" });
    const view = render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByTestId("playback-banner")).toHaveTextContent("Looking for Playback…");
    expect(screen.getByText(/Checked 64 of 254 addresses/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Searching… (cancel)" }));
    expect(playback.cancelScan).toHaveBeenCalledOnce();
    view.rerender(<PlaybackApiPanel playback={controller({ status: none({ scan: scanOf("not_found", { reason: "x" }), reason: "Couldn't find Playback." }) })} />);
    expect(screen.queryByText(/Scanning/)).not.toBeInTheDocument();
  });

  it("link buttons use the Playback colour, underlined, with a 44px target", async () => {
    const playback = controller({ status: none({ scan: scanOf("not_found"), reason: "Couldn't find Playback." }) });
    render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByRole("button", { name: "Enter address instead" })).toHaveClass("text-integration-playback-text", "underline", "min-h-ds-touch-target", "can-hover:text-integration-playback");
  });

  it("not found: plain checklist, Scan and Enter address instead; never a stuck spinner", async () => {
    const playback = controller({ status: none({ scan: scanOf("not_found"), reason: "Couldn't find Playback. Check things." }) });
    render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByText("Make sure:")).toBeVisible();
    expect(screen.getByText("Remote Connections is turned on in Playback")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(playback.scan).toHaveBeenCalledWith();
    await userEvent.setup().click(screen.getByRole("button", { name: "Enter address instead" }));
    expect(screen.getByRole("combobox", { name: "Playback host" })).toHaveFocus();
  });

  it("several found: pick one by computer name", () => {
    const playback = controller({ status: none({ scan: scanOf("found", { candidates: [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }] }) }) });
    render(<PlaybackApiPanel playback={playback} />);
    const group = screen.getByRole("group", { name: "Found 2 Playback computers. Choose one:" });
    fireEvent.click(within(group).getByRole("button", { name: /Other PC/ }));
    expect(playback.scan).toHaveBeenCalledWith("192.0.2.2");
  });
});

describe("Playback computer address box (no select, no Other computer…)", () => {
  const user = () => userEvent.setup();
  const computer = () => screen.getByRole("combobox", { name: "Playback host" });
  const arrow = () => screen.getByRole("button", { name: "Show recent and found addresses" });
  const withHosts = (host: string | null, candidates: { name: string; host: string }[] = [], extra = {}) => controller({ status: none({
    scan: scanOf(candidates.length ? "found" : "idle", { candidates }),
    settings: { enabled: true, host, port: 8080, auto_scan: true, fast_transport: true },
    ...extra,
  }) });
  beforeEach(() => window.localStorage.clear());

  it("is one editable text box holding the saved host: there is no select and no Other computer anywhere", () => {
    render(<PlaybackApiPanel playback={withHosts("192.0.2.9", [{ name: "Example Mac", host: "192.0.2.1" }])} />);
    expect(computer().tagName).toBe("INPUT");
    expect(computer()).toHaveValue("192.0.2.9");
    expect(screen.queryByRole("combobox", { name: "Computer name or address" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Computer name or address")).not.toBeInTheDocument();
    expect(screen.queryByText(/Other computer/)).not.toBeInTheDocument();
    expect(document.querySelector("select")).toBeNull();
  });

  it("the arrow lists the scan's computers by name and address, under Scanned; the saved host is the box's value, not a list entry", async () => {
    render(<PlaybackApiPanel playback={withHosts("192.0.2.9", [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }])} />);
    await user().click(arrow());
    expect(within(screen.getByRole("group", { name: "Found" })).getAllByRole("option").map((o) => o.textContent)).toEqual(["Example Mac (192.0.2.1)", "Other PC (192.0.2.2)"]);
    expect(screen.queryByText(/\(saved\)|\(recent\)/)).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Recent" })).not.toBeInTheDocument();
  });

  it("does not repeat a saved host that is also a candidate, and marks it as the one in the box", async () => {
    render(<PlaybackApiPanel playback={withHosts("192.0.2.2", [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }])} />);
    await user().click(arrow());
    expect(screen.getAllByRole("option", { name: /192\.0\.2\.2/ })).toHaveLength(1);
    expect(screen.getByRole("option", { name: /192\.0\.2\.2/ })).toHaveAttribute("data-current", "true");
    expect(computer()).toHaveValue("192.0.2.2");
  });

  it("empty candidates and no saved host: an empty box, Connect off and saying what to do, and a calm list", async () => {
    render(<PlaybackApiPanel playback={withHosts(null)} />);
    expect(computer()).toHaveValue("");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connect" }).closest("[data-part=connect-wrap]")).toHaveAttribute("title", "Type an address to connect.");
    await user().click(arrow());
    expect(screen.getByText("No recent or found addresses yet.")).toBeVisible();
  });

  it("the old expander text is gone", () => {
    render(<PlaybackApiPanel playback={withHosts(null)} />);
    expect(screen.queryByText(/Connect a specific computer/)).not.toBeInTheDocument();
    expect(document.getElementById("playback-specific")).not.toHaveAttribute("aria-expanded");
  });

  it("choosing a scanned computer only fills the box; Connect then scans exactly that host", async () => {
    const playback = withHosts(null, [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }]);
    const u = user();
    render(<PlaybackApiPanel playback={playback} />);
    await u.click(arrow());
    await u.click(screen.getByRole("option", { name: /Other PC/ }));
    expect(computer()).toHaveValue("192.0.2.2");
    expect(playback.scan).not.toHaveBeenCalled();
    await u.click(screen.getByRole("button", { name: "Connect" }));
    expect(playback.scan).toHaveBeenCalledExactlyOnceWith("192.0.2.2");
  });

  it("a chosen scanned computer can be edited at once: Connect uses what the box holds then", async () => {
    const playback = withHosts(null, [{ name: "Example Mac", host: "192.0.2.1" }]);
    const u = user();
    render(<PlaybackApiPanel playback={playback} />);
    await u.click(arrow());
    await u.click(screen.getByRole("option", { name: /Example Mac/ }));
    await u.type(computer(), "0");
    expect(computer()).toHaveValue("192.0.2.10");
    await u.click(screen.getByRole("button", { name: "Connect" }));
    expect(playback.scan).toHaveBeenCalledExactlyOnceWith("192.0.2.10");
  });

  it("typing an address: Enter connects in one action (list closed), blank cannot connect, there is no line saying so", async () => {
    const playback = withHosts(null);
    const u = user();
    render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(screen.queryByText("Type an address to connect.")).not.toBeInTheDocument();
    await u.type(computer(), "  192.0.2.10  {Enter}");
    expect(playback.scan).toHaveBeenCalledExactlyOnceWith("192.0.2.10");
    expect(screen.queryByText(/Save address|Find \/ Scan|override/)).not.toBeInTheDocument();
  });

  it("Enter in the box with the list open chooses the active option and does not connect", async () => {
    const playback = withHosts(null, [{ name: "Example Mac", host: "192.0.2.1" }]);
    const u = user();
    render(<PlaybackApiPanel playback={playback} />);
    await u.click(computer());
    await u.keyboard("{ArrowDown}{Enter}");
    expect(computer()).toHaveValue("192.0.2.1");
    expect(playback.scan).not.toHaveBeenCalled();
  });

  it.each(["http://192.0.2.10", "192.0.2.10:8080", "bad host", "999.0.2.10", "a/b"])("rejects invalid address %s inline", async (value) => {
    const playback = withHosts(null);
    render(<PlaybackApiPanel playback={playback} />);
    fireEvent.change(computer(), { target: { value } });
    expect(screen.getByRole("alert")).toHaveTextContent("no http://");
    expect(computer()).toBeInvalid();
    expect(computer()).toHaveAccessibleDescription(/no http:\/\//);
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    fireEvent.submit(computer().closest("form")!);
    expect(playback.scan).not.toHaveBeenCalled();
  });

  it("Enter address instead (not found) puts the cursor in the box (it is always open to typing)", async () => {
    render(<PlaybackApiPanel playback={controller({ status: none({ scan: scanOf("not_found") }) })} />);
    await user().click(screen.getByRole("button", { name: "Enter address instead" }));
    expect(computer()).toHaveFocus();
  });

  it("Enter address instead in a scan error (refused, permission denied) does the same", async () => {
    for (const error_class of ["refused", "permission_denied"]) {
      const scan = scanOf("not_found", { error_class, typed: false, reason: "x", details: "{}" });
      const { unmount } = render(<PlaybackApiPanel playback={controller({ status: none({ scan }) })} />);
      await user().click(screen.getByRole("button", { name: "Enter address instead" }));
      expect(computer()).toHaveFocus();
      unmount();
    }
  });

  it("a computer that connected is in Recent the next time, newest first; one tap fills the box and Connect connects", async () => {
    const connected = withHosts("192.0.2.9", [], { connected: true, active_source: "playback_api", reason: "Connected to Playback at 192.0.2.9.", sources: { playback_api: { connected: true, reason: "Playback is connected." }, midi: { connected: false, reason: "MIDI is disconnected." } } });
    const first = render(<PlaybackApiPanel playback={connected} />);
    first.unmount();
    expect(JSON.parse(window.localStorage.getItem("stagepilot.recent-addresses.playback") ?? "[]")).toEqual(["192.0.2.9"]);
    const playback = withHosts("192.0.2.77");
    const u = user();
    render(<PlaybackApiPanel playback={playback} />);
    await u.click(arrow());
    expect(within(screen.getByRole("group", { name: "Recent" })).getAllByRole("option").map((o) => o.textContent)).toEqual(["192.0.2.9"]);
    await u.click(screen.getByRole("option", { name: "192.0.2.9" }));
    expect(computer()).toHaveValue("192.0.2.9");
    expect(playback.scan).not.toHaveBeenCalled();
    await u.click(screen.getByRole("button", { name: "Connect" }));
    expect(playback.scan).toHaveBeenCalledExactlyOnceWith("192.0.2.9");
  });
});

describe("validPlaybackHost", () => {
  it.each(["", "192.0.2.10", "playback.local", "127.0.0.1", "::1"])("accepts bare host %s", (host) => expect(validPlaybackHost(host)).toBe(true));
});

describe("Song order card", () => {
  it("disconnected: the explanation, and the one button greyed out with its reason (never hidden)", () => {
    render(<PlaybackApiPanel playback={controller({ status: none() })} />);
    expect(screen.getByText("Connect to Playback first.")).toBeVisible();
    const button = screen.getByRole("button", { name: "Scan Playback API" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Connect to the Playback API first.");
    expect(button).toHaveAccessibleDescription("Connect to the Playback API first.");
  });

  it("connected without order: one button, confirmation dialog, then discover", async () => {
    const playback = controller({ status: playbackStatus({ song_order: [], stale: true, captured_version: null }) });
    const user = userEvent.setup();
    render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByText(/steps through your Playback songs once/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Scan Playback API" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("must stay stopped");
    expect(dialog).toHaveTextContent("read their lengths");
    expect(dialog).toHaveTextContent("Nothing will be played.");
    expect(dialog).not.toHaveTextContent("2 seconds");
    expect(playback.discover).not.toHaveBeenCalled();
    expect(within(dialog).getByRole("button", { name: "Scan Playback API" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Scan Playback API" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Scan Playback API" }));
    expect(playback.discover).toHaveBeenCalledOnce();
  });

  it("works with nothing typed or saved: no address and no save needed", () => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ song_order: [], stale: true, settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true } }) })} />);
    expect(screen.getByRole("button", { name: "Scan Playback API" })).toBeEnabled();
  });

  it("playing: the explanation, the button greyed out with its reason; an open dialog closes", () => {
    const view = render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ song_order: [], stale: true }) })} />);
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback API" }));
    view.rerender(<PlaybackApiPanel playback={controller({ status: playbackStatus({ playing: true, song_order: [], stale: true }) })} />);
    expect(screen.getByText("Stop Playback to scan Playback songs.")).toBeVisible();
    const button = screen.getByRole("button", { name: "Scan Playback API" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Stop Playback to scan songs.");
    expect(button).toHaveAccessibleDescription("Stop Playback to scan songs.");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("stale: warning message with the same one button; saved: confirmation count", () => {
    const view = render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ stale: true }) })} />);
    expect(screen.getByText("Your Playback setlist changed. Scan the setlist again before updating Planning Center.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Scan Playback API" })).toBeEnabled();
    view.rerender(<PlaybackApiPanel playback={controller({ status: playbackStatus() })} />);
    expect(screen.getByText("Song order saved (2 songs)")).toBeVisible();
  });

  it.each([
    [5, 4, "Playback has 5 songs, this service plan has 4. Playback song 5 is ignored."],
    [4, 6, "Playback has 4 songs, this service plan has 6. Service plan songs 5-6 have no Playback song."],
    [6, 4, "Playback has 6 songs, this service plan has 4. Playback songs 5-6 are ignored."],
    [4, 5, "Playback has 4 songs, this service plan has 5. Service plan song 5 has no Playback song."],
    [1, 0, "Playback has 1 song, this service plan has 0. Playback song 1 is ignored."],
    [0, 1, "Playback has 0 songs, this service plan has 1. Service plan song 1 has no Playback song."],
  ])("shows saved count mismatch %i/%i, including while playing", (song_count, plan_song_count, text) => {
    const status = playbackStatus({ song_count, plan_song_count });
    const view = render(<PlaybackApiPanel playback={controller({ status })} />);
    expect(within(screen.getByRole("region", { name: "Song order" })).getByText(text)).toBeVisible();
    view.rerender(<PlaybackApiPanel playback={controller({ status: { ...status, playing: true } })} />);
    expect(screen.getByText(text)).toBeVisible();
  });

  it.each([
    { song_count: 2, plan_song_count: 2 },
    { song_count: 5, plan_song_count: 4, stale: true },
    { song_count: 5, plan_song_count: 4, song_order: [] },
  ])("hides mismatch for matching counts or unsaved order %o", (extra) => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus(extra) })} />);
    expect(screen.queryByText(/Playback has \d+ songs?/)).not.toBeInTheDocument();
  });

  it("uses singular for a saved one-song order", () => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ song_order: [101], song_count: 1, plan_song_count: 1 }) })} />);
    expect(screen.getByText("Song order saved (1 song)")).toBeVisible();
  });

  it("progress and failure are plain", () => {
    const view = render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ discovery: "running", progress: 3 }) })} />);
    expect(screen.getByRole("progressbar", { name: "Reading Playback song lengths" })).toBeVisible();
    expect(screen.queryByText(/step 3/)).not.toBeInTheDocument();
    view.rerender(<PlaybackApiPanel playback={controller({ status: playbackStatus({ discovery: "running", progress: 12, discovery_song: 3, discovery_total: 8 }) })} />);
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuemax", "100");
    expect(screen.queryByText(/Checking song/)).not.toBeInTheDocument();
    expect(screen.queryByText(/step 12/)).not.toBeInTheDocument();
    view.rerender(<PlaybackApiPanel playback={controller({ status: playbackStatus({ discovery: "failed", last_error: "Playback went away." }) })} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Playback went away.");
  });
  it("reuses the count mismatch line alongside length rows, including while playing", () => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus({
      playing: true, song_count: 2, plan_song_count: 1, song_lengths: [180, 202],
      plan_songs: [{ id: "p1", title: "Plan title", duration_seconds: 180 }],
    }) })} />);
    expect(screen.getAllByText("Playback has 2 songs, this service plan has 1. Playback song 2 is ignored.")).toHaveLength(1);
    expect(screen.getByRole("group", { name: "Song arrangement" })).toBeVisible();
    expect(screen.getByText("Plan title")).toBeVisible();
  });
});

describe("Recent activity has moved", () => {
  it("is not in the Playback config any more: the dashboard's Event stream shows Playback's song events", () => {
    render(<PlaybackApiPanel playback={controller()} />);
    expect(screen.queryByRole("button", { name: /Recent activity/ })).toBeNull();
    expect(screen.queryByText("Nothing yet.")).toBeNull();
  });
});

describe("Main view has no jargon", () => {
  it("avoids API, monotonic, velocity, manual address", () => {
    panel(controller());
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/monotonic|manual address|velocities/i);
  });
});

describe("Connection method control (Playback API primary / MIDI alternate)", () => {
  const validMidi = { ...playbackSettings, settings: { ...playbackSettings.settings, midi: { ...playbackSettings.settings.midi, mappings: { start_next: 100, restart_current: 101, previous: 102, next: 103, reload_plan: 104, stop_timer: 105 } } } };
  const midiStatus = () => playbackStatus({ sources: { playback_api: { connected: false, reason: "Playback is disconnected." }, midi: { connected: true, reason: "MIDI is connected." } } });
  // The Connection method control is the shared ConnectionMethod radio group (DESIGN.md "Connection method").
  const group = () => screen.getByRole("radiogroup", { name: "Connection method" });
  // A radio's accessible name is its label text, so a screen reader hears "Playback API Primary".
  const radio = (name: string) => within(group()).getByRole("radio", { name: new RegExp(`^${name.replace(/[()]/g, "\\$&")}`) });
  const names = { playback_api: "Playback API", real: "MIDI" } as const;
  const chosen = () => within(group()).getAllByRole("radio").find((r) => (r as HTMLInputElement).checked) as HTMLInputElement;
  const chooseMethod = async (user: ReturnType<typeof userEvent.setup>, value: keyof typeof names) => { await user.click(radio(names[value])); };

  it("opens on the saved method with the two-option Connection method control", () => {
    panel();
    expect(group()).toBeInTheDocument();
    expect(chosen()).toHaveAttribute("value", "playback_api");
    // Two options, each a name over PRIMARY / ALTERNATE (the guide's template).
    expect(radio("Playback API")).toHaveAccessibleName("Playback API Primary");
    expect(radio("MIDI")).toHaveAccessibleName("MIDI Alternate");
    expect(document.querySelector("[data-alternate-notice]")).toBeNull();
    expect(screen.queryByText(/switch back here/)).toBeNull();
    expect(screen.queryByRole("button", { name: /Alternate connection: MIDI settings/ })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("MIDI channel")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search" })).toBeInTheDocument();
  });

  it("the toggle sits in the panel header slot on the Playback API and the Playback MIDI screens alike", async () => {
    const user = userEvent.setup();
    panel();
    const inHeader = () => document.querySelector("[data-setup-panel-header] [data-header-method] [data-connection-method]");
    expect(inHeader()).not.toBeNull();
    const slotClass = document.querySelector("[data-header-method]")!.className;
    await chooseMethod(user, "real");
    expect(inHeader()).not.toBeNull();
    expect(document.querySelector("[data-header-method]")!.className).toBe(slotClass);
    expect(document.querySelectorAll("[data-connection-method]")).toHaveLength(1);
  });

  it("switching the method swaps the fields as a view and calls nothing", async () => {
    const playback = controller();
    const user = userEvent.setup();
    panel(playback);
    await chooseMethod(user, "real");
    expect(playback.selectSource).not.toHaveBeenCalled();
    expect(playback.save).not.toHaveBeenCalled();
    expect(screen.getByLabelText("MIDI channel")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Search" })).not.toBeInTheDocument();
    await chooseMethod(user, "playback_api");
    expect(screen.getByRole("button", { name: "Search" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save configuration" })).toHaveAttribute("aria-disabled", "true");
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "clean");
  });

  it("holds the method in the draft: unsaved, reported dirty, and discard restores the saved one", async () => {
    const onDirtyChange = vi.fn();
    const user = userEvent.setup();
    panel(controller(), { onDirtyChange });
    await chooseMethod(user, "real");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    expect(screen.getByRole("button", { name: "Save configuration" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(chosen()).toHaveAttribute("value", "playback_api");
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  it("saving applies selectSource first, then the Playback settings", async () => {
    const order: string[] = [];
    const playback = controller({
      selectSource: vi.fn(async () => { order.push("selectSource"); return true; }),
      save: vi.fn(async () => { order.push("save"); return true; }),
    });
    const user = userEvent.setup();
    panel(playback);
    await chooseMethod(user, "real");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(order).toEqual(["selectSource"]);
    expect(playback.selectSource).toHaveBeenCalledWith("real");
    expect(playback.save).not.toHaveBeenCalled();
  });

  it("a method change is applied by selectSource alone: there is no separate settings save", async () => {
    const order: string[] = [];
    const playback = controller({
      selectSource: vi.fn(async () => { order.push("selectSource"); return true; }),
      save: vi.fn(async () => { order.push("save"); return true; }),
    });
    const Host = () => { const draft = usePlaybackDraft(playback, "real"); const save = usePlaybackSave(draft, playback, noMidi); return <>
      <button onClick={() => { draft.setMethod("playback_api"); }} type="button">edit</button>
      <button onClick={() => void save.save()} type="button">go</button></>; };
    render(<Host />);
    await userEvent.click(screen.getByRole("button", { name: "edit" }));
    await userEvent.click(screen.getByRole("button", { name: "go" }));
    expect(order).toEqual(["selectSource"]);
    expect(playback.selectSource).toHaveBeenCalledWith("playback_api");
    expect(playback.save).not.toHaveBeenCalled();
  });

  it("stops at the first failure: save is not called and the draft stays", async () => {
    const playback = controller({ selectSource: vi.fn().mockResolvedValue(false), save: vi.fn().mockResolvedValue(true) });
    const user = userEvent.setup();
    panel(playback);
    await chooseMethod(user, "real");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(playback.selectSource).toHaveBeenCalledTimes(1);
    expect(playback.save).not.toHaveBeenCalled();
    expect(chosen()).toHaveAttribute("value", "real");
    expect(screen.getByRole("alert")).toHaveTextContent(/not saved/);
    expect(screen.getByRole("button", { name: "Save configuration" })).toBeEnabled();
  });

  const midiPanel = (playback: PlaybackController, onSaveSettings: (v: never) => unknown) => render(<MidiSetupPanel playback={playback} midi={null} messages={[]} error={null} message={null} pendingOperation={null} onRefresh={vi.fn()} onSelect={vi.fn()} settings={validMidi} onSaveSettings={onSaveSettings as never} />);

  it("a failing selectSource in the MIDI view leaves the MIDI save uncalled", async () => {
    const playback = controller({ selectSource: vi.fn().mockResolvedValue(false) });
    const onSaveSettings = vi.fn();
    const user = userEvent.setup();
    midiPanel(playback, onSaveSettings);
    await chooseMethod(user, "real");
    fireEvent.change(screen.getByLabelText("MIDI channel"), { target: { value: "5" } });
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(playback.selectSource).toHaveBeenCalledWith("real");
    expect(onSaveSettings).not.toHaveBeenCalled();
    expect(chosen()).toHaveAttribute("value", "real");
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "error");
  });

  it("method and MIDI edits are applied by ONE click: selectSource, then the MIDI save", async () => {
    const order: string[] = [];
    const playback = controller({
      selectSource: vi.fn(async () => { order.push("selectSource"); return true; }),
      save: vi.fn(async () => { order.push("playbackSave"); return true; }),
    });
    const onSaveSettings = vi.fn(async () => { order.push("saveMidi"); return true; });
    const user = userEvent.setup();
    midiPanel(playback, onSaveSettings);
    await chooseMethod(user, "real");
    fireEvent.change(screen.getByLabelText("MIDI channel"), { target: { value: "5" } });
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(order).toEqual(["selectSource", "saveMidi"]);
  });

  it("order with the method and MIDI edited: selectSource, then the MIDI save; the MIDI payload is byte-identical to the old Save MIDI settings", async () => {
    const order: string[] = [];
    const base = controller();
    const playback = controller({
      status: playbackStatus({ settings: { ...base.status!.settings } }),
      selectSource: vi.fn(async () => { order.push("selectSource"); return true; }),
      save: vi.fn(async () => { order.push("playbackSave"); return true; }),
    });
    const onSaveSettings = vi.fn(async () => { order.push("saveMidi"); return true; });
    const user = userEvent.setup();
    midiPanel(playback, onSaveSettings);
    await chooseMethod(user, "real");
    fireEvent.change(screen.getByLabelText("MIDI channel"), { target: { value: "5" } });
    expect(document.querySelector("[data-save-status]")!.textContent).toBe("2\u00a0changes waiting. Not saved yet.");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(order).toEqual(["selectSource", "saveMidi"]);
    expect(playback.save).not.toHaveBeenCalled();
    expect(JSON.stringify((onSaveSettings.mock.calls as unknown as unknown[][])[0]![0])).toBe(JSON.stringify({ ...validMidi.settings.midi, enabled: true, channel: 5, note: validMidi.settings.midi.note, debounce_ms: validMidi.settings.midi.debounce_ms, mappings: validMidi.settings.midi.mappings }));
  });

  it("stops at the first failure and Try again does not repeat completed steps", async () => {
    const calls: string[] = [];
    const playback = controller({
      selectSource: vi.fn(async () => { calls.push("selectSource"); return true; }),
      save: vi.fn(async () => { calls.push("playbackSave"); return true; }),
    });
    const onSaveSettings = vi.fn().mockImplementationOnce(async () => { calls.push("saveMidi"); return false; }).mockImplementation(async () => { calls.push("saveMidi"); return true; });
    const user = userEvent.setup();
    midiPanel(playback, onSaveSettings);
    await chooseMethod(user, "real");
    fireEvent.change(screen.getByLabelText("MIDI channel"), { target: { value: "5" } });
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(calls).toEqual(["selectSource", "saveMidi"]);
    expect(document.querySelector("[data-save-status]")!.textContent).toBe("Couldn't save");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(calls).toEqual(["selectSource", "saveMidi", "saveMidi"]);
  });

  it("invalid MIDI values: nothing is written and the reason is shown", async () => {
    const playback = controller();
    const onSaveSettings = vi.fn();
    const user = userEvent.setup();
    midiPanel(playback, onSaveSettings);
    await chooseMethod(user, "real");
    fireEvent.change(screen.getByLabelText("MIDI channel"), { target: { value: "99" } });
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(playback.selectSource).not.toHaveBeenCalled();
    expect(onSaveSettings).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(/MIDI settings are not valid/);
  });

  it("opens on the saved alternate method and appends it to the header badge only then", () => {
    const saved = { ...playbackSettings, settings: { ...playbackSettings.settings, integration_modes: { ...playbackSettings.settings.integration_modes, midi_source: "real" as const } } };
    const view = render(<MidiSetupPanel playback={controller({ status: midiStatus() })} midi={null} messages={[]} error={null} message={null} pendingOperation={null} onRefresh={vi.fn()} onSelect={vi.fn()} settings={saved} onSaveSettings={vi.fn()} />);
    expect(chosen()).toHaveAttribute("value", "real");
    expect(screen.getByLabelText("MIDI channel")).toBeVisible();
    expect(view.container.querySelector("[data-setup-panel-header]")).toHaveTextContent("MIDI · Alternate");
    view.unmount();
    panel();
    expect(document.querySelector("[data-setup-panel-header]")).not.toHaveTextContent("alternate");
  });

  it("an install saved with the old simulated source opens on Playback API and offers no Simulation option", () => {
    const sim = { ...playbackSettings, settings: { ...playbackSettings.settings, integration_modes: { ...playbackSettings.settings.integration_modes, midi_source: "simulated" as const } } };
    render(<MidiSetupPanel playback={controller()} midi={null} messages={[]} error={null} message={null} pendingOperation={null} onRefresh={vi.fn()} onSelect={vi.fn()} settings={sim} onSaveSettings={vi.fn()} />);
    expect(chosen()).toHaveAttribute("value", "playback_api");
    expect(screen.queryByText(/Simulation/)).not.toBeInTheDocument();
    expect(within(group()).getAllByRole("radio")).toHaveLength(2);
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "clean");
  });
});

const scenarios = [
  { name: "API only", api: true, midi: false, active: "playback_api" as const, reason: "Connected to Playback on 192.0.2.10." },
  { name: "MIDI only", api: false, midi: true, active: "midi" as const, reason: "Connected through MIDI." },
  { name: "both", api: true, midi: true, active: "playback_api" as const, reason: "Connected to Playback on 192.0.2.10." },
  { name: "neither", api: false, midi: false, active: "none" as const, reason: "Disconnected. Choose Scan to find Playback." },
  { name: "scanning", api: false, midi: false, active: "none" as const, reason: "Looking for Playback…", scanning: true },
];
describe("Banner, panel, dashboard, checklist and Alternate connection: MIDI settings never contradict", () => {
  it.each(scenarios)("$name", async ({ api, midi, active, reason, scanning }) => {
    const connected = api || midi;
    const status = playbackStatus({
      connected, active_source: active, reason, scan: scanOf(scanning ? "scanning" : "idle"),
      sources: { playback_api: { connected: api, reason: api ? "Playback is connected." : "Playback is disconnected." }, midi: { connected: midi, reason: midi ? "MIDI is connected." : "MIDI is disconnected." } },
    });
    panel(controller({ status }));
    // One banner, one sentence: exactly the backend text.
    expect(screen.getByTestId("playback-banner")).toHaveTextContent(reason);
    expect(screen.getAllByText(reason)).toHaveLength(1);
    await userEvent.setup().click(screen.getByRole("radio", { name: /^MIDI/ }));
    // MIDI detail in the alternate connection section never says the opposite of the banner.
    const advanced = document.getElementById("playback-midi-method") as HTMLElement;
    if (midi) expect(advanced).toHaveTextContent(/MIDI is connected/);
    else expect(advanced).not.toHaveTextContent(/MIDI is connected|MIDI input connected/);
    expect(advanced).not.toHaveTextContent(/disconnected for this session/);
    const views = buildConnectionCardViews({ state: playbackState, settings: playbackSettings, playback: status, midi: null, propresenter: null, lights: null });
    expect(views.midi.status).toBe(connected ? "connected" : "disconnected");
    expect(views.midi.detail).toBe(reason);
    const checks = buildReadinessChecks({ state: playbackState, settings: playbackSettings, propresenter: null, live: true, views });
    expect(checks.find((check) => check.id === "midi")).toMatchObject({ passed: connected, detail: reason });
    render(<SetupChecklist state={playbackState} settings={playbackSettings} playback={status} midi={null} propresenter={null} live onOpen={vi.fn()} />);
    // The banner belongs to the Playback API view, which the MIDI method swapped out; the checklist still says the same sentence.
    expect(screen.getAllByText(reason).length).toBeGreaterThanOrEqual(1);
  });

  it.each([{ stale: true, song_order: [101, 202] }, { stale: false, song_order: [] }])("keeps connection green and action readiness separate for invalid order %o", (mapping) => {
    const status = playbackStatus(mapping);
    const views = buildConnectionCardViews({ state: playbackState, settings: playbackSettings, playback: status, midi: null, propresenter: null, lights: null });
    const checks = buildReadinessChecks({ state: playbackState, settings: playbackSettings, live: true, propresenter: null, views });
    expect(views.midi.status).toBe("connected");
    expect(checks.find((check) => check.id === "midi")?.passed).toBe(true);
    expect(checks.find((check) => check.id === "playback-order")?.passed).toBe(false);
  });
});

describe("No dead primary button without a visible fix", () => {
  const states = [
    ["disconnected", none()],
    ["scanning", none({ scan: scanOf("scanning") })],
    ["not found", none({ scan: scanOf("not_found") })],
    ["connected, no order", playbackStatus({ song_order: [], stale: true })],
    ["playing", playbackStatus({ playing: true })],
    ["stale", playbackStatus({ stale: true })],
    ["discovering", playbackStatus({ discovery: "running" })],
  ] as const;
  it.each(states)("%s", (_name, status) => {
    panel(controller({ status }));
    const footer = document.querySelector("[data-save-bar]") as HTMLElement;
    // A switched-off Connect is not dead: it says what to do (the hint on the button). The same for the greyed-out song scan (its reason).
    const disabled = screen.getAllByRole("button").filter((button) => (button as HTMLButtonElement).disabled && !footer.contains(button) && !button.closest('[aria-label="Services song times"]') && !button.closest("[data-part=connect-wrap][title]") && !button.closest("[data-part=song-scan][title]"));
    expect(disabled.map((b) => b.textContent)).toEqual([]);
    expect(screen.getAllByRole("button").some((b) => /^Search$|Searching/.test(b.textContent ?? "") && !(b as HTMLButtonElement).disabled)).toBe(true);
  });
});

// A stateful harness: the controller's status follows save(), like the real hook.
function Harness({ save, onDirty }: { save: PlaybackController["save"]; onDirty?: (dirty: boolean) => void }) {
  const [status, setStatus] = useState(playbackStatus());
  const playback = controller({
    status,
    save: async (input) => {
      const ok = await save(input);
      if (ok) setStatus((current) => ({ ...current, enabled: input.enabled, settings: input }));
      return ok;
    },
  });
  const draft = usePlaybackDraft(playback);
  const bar = usePlaybackSave(draft, playback, noMidi);
  onDirty?.(bar.dirty);
  return <><PlaybackApiPanel playback={playback} draft={draft} /><PlaybackSaveBar save={bar} /></>;
}

describe("Save configuration bar (Playback)", () => {
  afterEach(() => vi.useRealTimers());
  const saveButton = () => screen.getByRole("button", { name: "Save configuration" });
  const state = () => document.querySelector("[data-save-bar]")!.getAttribute("data-state");
  const statusText = () => document.querySelector("[data-save-status]")!.textContent;
  // The one edit left on the API view is the connection method: choosing MIDI holds a change until Save.
  const editMethod = (user: ReturnType<typeof userEvent.setup>) => user.click(screen.getByRole("radio", { name: /^MIDI/ }));

  it("clean: All changes saved, button present but dimmed with the reason beside it, click does nothing", () => {
    const save = vi.fn().mockResolvedValue(true);
    render(<Harness save={save} />);
    expect(state()).toBe("clean");
    expect(statusText()).toBe("All changes saved");
    expect(saveButton()).toHaveAttribute("aria-disabled", "true");
    expect(saveButton()).toHaveAccessibleDescription("No changes to save");
    fireEvent.click(saveButton());
    expect(save).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Discard" })).not.toBeInTheDocument();
  });

  it("there is no Automatically Connect switch: connecting automatically is always on", () => {
    panel();
    expect(screen.queryByLabelText("Automatically Connect")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(/Automatically Connect/);
  });

  it("held: N changes waiting, Discard restores clean and never saves", async () => {
    const user = userEvent.setup();
    const playback = controller();
    panel(playback);
    await editMethod(user);
    expect(state()).toBe("held");
    expect(statusText()).toBe("1\u00a0change waiting. Not saved yet.");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(state()).toBe("clean");
    expect(playback.selectSource).not.toHaveBeenCalled();
    expect(playback.save).not.toHaveBeenCalled();
  });

  it("saving: Saving… while the method is written", async () => {
    let finish!: (ok: boolean) => void;
    const selectSource = vi.fn().mockReturnValue(new Promise<boolean>((resolve) => { finish = resolve; }));
    const user = userEvent.setup();
    panel(controller({ selectSource }));
    await editMethod(user);
    await user.click(saveButton());
    expect(selectSource).toHaveBeenCalledExactlyOnceWith("real");
    expect(state()).toBe("saving");
    expect(statusText()).toBe("Saving…");
    await act(async () => finish(true));
    expect(state()).not.toBe("saving");
  });

  it("error: Couldn't save with Try again, changes kept; Try again repeats the failed step", async () => {
    const selectSource = vi.fn().mockResolvedValueOnce(false).mockResolvedValue(true);
    const user = userEvent.setup();
    panel(controller({ selectSource }));
    await editMethod(user);
    await user.click(saveButton());
    expect(state()).toBe("error");
    expect(statusText()).toBe("Couldn't save");
    expect(screen.getByRole("alert")).toHaveTextContent("not saved");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(selectSource).toHaveBeenCalledTimes(2);
    expect(state()).not.toBe("error");
  });

  it("auto: when Scan or Connect saves the computer by itself the bar says Saved automatically at <time>", async () => {
    const view = render(<AutoHarness host={null} />);
    expect(state()).toBe("clean");
    view.rerender(<AutoHarness host="192.0.2.10" />);
    await act(async () => undefined);
    expect(state()).toBe("auto");
    expect(statusText()).toMatch(/^Saved automatically at\s/);
  });

  it("an install saved with automatic connecting off is turned back on once, with network scan and fast transport on", async () => {
    const save = vi.fn().mockResolvedValue(true);
    const off = playbackStatus({ enabled: false, settings: { enabled: false, host: null, port: 8080, auto_scan: true, fast_transport: true } });
    const Heal = ({ method }: { method: "playback_api" | "real" }) => { usePlaybackDraft(controller({ status: off, save }), method); return null; };
    const view = render(<Heal method="real" />);
    expect(save).not.toHaveBeenCalled(); // MIDI is the saved method: Playback API is not touched
    view.rerender(<Heal method="playback_api" />);
    await act(async () => undefined);
    expect(save).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(save.mock.calls[0]![0])).toBe('{"enabled":true,"host":null,"port":8080,"auto_scan":true,"fast_transport":true}');
    view.rerender(<Heal method="playback_api" />);
    await act(async () => undefined);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("Scan and Connect are never gated by unsaved edits", async () => {
    const playback = controller();
    const Gate = () => { const draft = usePlaybackDraft(playback); const bar = usePlaybackSave(draft, playback, noMidi); return <><PlaybackApiPanel playback={playback} draft={draft} /><PlaybackSaveBar save={bar} /><button onClick={() => draft.setMethod("real")} type="button">edit</button></>; };
    render(<Gate />);
    fireEvent.click(screen.getByRole("button", { name: "edit" }));
    expect(state()).toBe("held");
    expect(screen.getByRole("button", { name: "Search" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(playback.scan).toHaveBeenCalledWith();
  });

  it("counts MIDI field edits in the same bar, with no separate MIDI notice", async () => {
    const user = userEvent.setup();
    panel();
    await user.click(screen.getByRole("radio", { name: /^MIDI/ }));
    expect(document.querySelector("[data-save-status]")!.textContent).toBe("1\u00a0change waiting. Not saved yet.");
    fireEvent.change(screen.getByLabelText("MIDI channel"), { target: { value: "5" } });
    expect(document.querySelector("[data-save-status]")!.textContent).toBe("2\u00a0changes waiting. Not saved yet.");
    expect(screen.getByLabelText("MIDI channel").closest("[data-field]")).toHaveTextContent("Not saved yet");
    expect(document.body).not.toHaveTextContent(/Save MIDI settings/);
    expect(screen.getAllByRole("button", { name: "Save configuration" })).toHaveLength(1);
  });

  it("reports dirtiness upward so leaving can ask first", async () => {
    const user = userEvent.setup();
    const onDirtyChange = vi.fn();
    panel(controller(), { onDirtyChange });
    await editMethod(user);
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  it("the bar is sticky on phones and sits at the end of the panel", () => {
    panel();
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-sticky", "true");
  });
});

function AutoHarness({ host }: { host: string | null }) {
  const status = playbackStatus({ settings: { enabled: true, host, port: 8080, auto_scan: true, fast_transport: true } });
  const playback = controller({ status });
  const draft = usePlaybackDraft(playback);
  const bar = usePlaybackSave(draft, playback, noMidi);
  return <><PlaybackApiPanel playback={playback} draft={draft} /><PlaybackSaveBar save={bar} /></>;
}

describe("Scan diagnostics", () => {
  const scanning = (extra = {}) => none({ scan: scanOf("scanning", { current: 64, total: 254, phase: "Network 192.0.2.0/24", elapsed: 3.4, ...extra }) });

  it("shows phase, real progress and elapsed time while scanning", () => {
    render(<PlaybackApiPanel playback={controller({ status: scanning() })} />);
    const text = screen.getByTestId("scan-progress").textContent ?? "";
    expect(text).toContain("Network 192.0.2.0/24");
    expect(text).toContain("Checked 64 of 254");
    expect(text).toContain("3s");
  });

  it("shows Connecting for a typed address and never the generic message", () => {
    const status = none({ scan: scanOf("scanning", { typed: true, phase: "Connecting to 192.0.2.50…", elapsed: 1 }) });
    render(<PlaybackApiPanel playback={controller({ status })} />);
    expect(screen.getByTestId("scan-progress").textContent).toContain("Connecting to 192.0.2.50");
    expect(screen.queryByText("Make sure:")).toBeNull();
  });

  it("a scanning state is not replaced by not-found before the backend says so", () => {
    const { rerender } = render(<PlaybackApiPanel playback={controller({ status: scanning() })} />);
    expect(screen.queryByText("Make sure:")).toBeNull();
    expect(screen.getByRole("button", { name: /Searching/ })).toBeInTheDocument();
    rerender(<PlaybackApiPanel playback={controller({ status: none({ scan: scanOf("not_found", { reason: "x", hosts_probed: 254, hosts_total: 254, networks: ["192.0.2.0/24"], elapsed: 9 }) }) })} />);
    expect(screen.getByText("Make sure:")).toBeInTheDocument();
    expect(screen.getByText(/Checked 254 of 254 addresses on 1 network \(192.0.2.0\/24\)/)).toBeInTheDocument();
  });

  it("shows the specific cause, next step, settings link and copy button", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const scan = scanOf("not_found", {
      error_class: "permission_denied", reason: "macOS blocked StagePilot. Open System Settings > Privacy & Security > Local Network.",
      settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork", details: "{\"outcome\":\"error\"}",
      hosts_probed: 254, hosts_total: 254, networks: ["192.0.2.0/24"], elapsed: 0.4,
    });
    render(<PlaybackApiPanel playback={controller({ status: none({ scan }) })} />);
    expect(screen.getByTestId("scan-error")).toHaveTextContent("Privacy & Security > Local Network");
    expect(screen.queryByText("Make sure:")).toBeNull();
    expect(screen.getByRole("button", { name: "Open Local Network settings" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Copy diagnostic details" }));
    expect(writeText).toHaveBeenCalledWith("{\"outcome\":\"error\"}");
  });

  it("shows the plain macOS block headline, numbered steps and no generic checklist", async () => {
    for (const typed of [false, true]) {
      const scan = scanOf("not_found", { error_class: "permission_denied", typed, reason: "macOS is blocking StagePilot's local-network access", details: "{}", hosts_probed: 763, hosts_total: 763, elapsed: 0.2 });
      const { unmount } = render(<PlaybackApiPanel playback={controller({ status: none({ scan }) })} />);
      expect(screen.getByText("macOS is blocking StagePilot's local-network access")).toBeInTheDocument();
      expect(screen.getAllByRole("listitem")).toHaveLength(typed ? 4 : 4);
      expect(screen.getByText(/Turn StagePilot off and on again/)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Open Local Network settings" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Copy diagnostic details" })).toBeInTheDocument();
      expect(screen.queryByText("Make sure:")).toBeNull();
      expect(screen.queryByText(/Playback is open/)).toBeNull();
      unmount();
    }
  });

  it("a typed-address failure reports one address and its own error", () => {
    const scan = scanOf("not_found", { error_class: "refused", typed: true, reason: "That computer answered, but Playback isn't accepting connections.", elapsed: 0.2, details: "{}" });
    render(<PlaybackApiPanel playback={controller({ status: none({ scan }) })} />);
    expect(screen.getByText("Couldn't connect")).toBeInTheDocument();
    expect(screen.getByText("Tried one address in 0.2s.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open Local Network settings" })).toBeNull();
  });
});

describe("outdated scan removes the Planning Center update and asks for a new scan", () => {
  const measured = { planning_center_connected: true, captured_at: "2026-10-07T12:00:00Z", song_lengths: [268, 240], discovery: "done" as const };
  const NOTICE_SETLIST = "Your Playback setlist changed. Scan the setlist again before updating Planning Center.";
  const NOTICE_PLAN = "Your Services plan changed since the last scan. Scan the setlist again before updating Planning Center.";
  const updateButton = () => screen.getByRole("button", { name: "Update Services" });

  it("current scan: no notice, update enabled", () => {
    panel(controller({ status: playbackStatus(measured) }));
    expect(screen.queryByText(NOTICE_SETLIST)).not.toBeInTheDocument();
    expect(screen.queryByText(NOTICE_PLAN)).not.toBeInTheDocument();
    expect(updateButton()).toBeEnabled();
  });

  it.each([
    ["heartbeat or event song not in the scan, or Playback setlist version change, or setlist.loaded with another ID (backend reports stale)", { stale: true, song_lengths: [] }, NOTICE_SETLIST],
    ["Planning Center plan or service type changed since the scan", { plan_changed: true, song_lengths: [] }, NOTICE_PLAN],
    ["app restarted with a saved scan, then reconnected to a different setlist (stale on reconnect)", { stale: true, song_lengths: [] }, NOTICE_SETLIST],
  ])("%s", (_name, extra, notice) => {
    const playback = controller({ status: playbackStatus({ ...measured, ...extra }) });
    panel(playback);
    expect(screen.getAllByText(notice).length).toBeGreaterThan(0);
    expect(updateButton()).toBeDisabled();
    fireEvent.click(updateButton());
    expect(screen.queryByRole("dialog", { name: "Update Services?" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Scan Playback API" }).length).toBeGreaterThan(0);
  });

  it("a fresh scan restores the update action without a restart", () => {
    const view = panel(controller({ status: playbackStatus({ ...measured, stale: true, song_lengths: [] }) }));
    expect(updateButton()).toBeDisabled();
    view.rerender(<MidiSetupPanel playback={controller({ status: playbackStatus(measured) })} midi={null} messages={[]} error={null} message={null} pendingOperation={null} onRefresh={vi.fn()} onSelect={vi.fn()} settings={playbackSettings} onSaveSettings={vi.fn()} />);
    expect(screen.queryByText(NOTICE_SETLIST)).not.toBeInTheDocument();
    expect(updateButton()).toBeEnabled();
  });
});

describe("Song order section", () => {
  beforeEach(() => window.localStorage.clear());
  const toggle = () => screen.getByRole("button", { name: /Song order/ });

  it("is expanded by default, with chevron, title, one-line summary and the scan button in the header", () => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus() })} />);
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("song-order-summary")).toHaveTextContent("Saved - 2 songs");
    const header = document.querySelector("[data-section=band] [data-part=band-header]")!;
    expect(within(header as HTMLElement).getByRole("button", { name: "Scan Playback API" })).toBeVisible();
    expect(header.querySelector('[data-part="chevron"]')).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Set up/ })).toBeNull();
  });

  it("folds, keeps the header row (summary and scan button) and remembers the choice on this device", async () => {
    const user = userEvent.setup();
    const view = render(<PlaybackApiPanel playback={controller({ status: playbackStatus() })} />);
    await user.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(window.localStorage.getItem(SONG_ORDER_OPEN_KEY)).toBe("false");
    expect(document.getElementById(toggle().getAttribute("aria-controls")!)!.firstElementChild).toHaveAttribute("inert");
    expect(screen.getByTestId("song-order-summary")).toBeVisible();
    expect(screen.getByRole("button", { name: "Scan Playback API" })).toBeVisible();
    view.unmount();
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus() })} />);
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    await userEvent.setup().click(toggle());
    expect(window.localStorage.getItem(SONG_ORDER_OPEN_KEY)).toBe("true");
  });

  it.each([
    ["saved", {}, "Saved - 2 songs"],
    ["learning", { song_order: [], song_count: 6, plan_song_count: 6, order_mode: "learned" as const, learned_songs: [{ song_id: 7, plan_position: 0, confirmed: true, doubtful: false }, { song_id: 9, plan_position: 1, confirmed: false, doubtful: false }] }, "Learning - 2 of 6 placed"],
    ["none", { song_order: [], song_count: 0, order_mode: "none" as const, learned_songs: [] }, "No order yet"],
    ["stale", { stale: true }, "Needs a new scan"],
  ])("summary when %s", (_name, extra, text) => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus(extra) })} />);
    expect(screen.getByTestId("song-order-summary")).toHaveTextContent(text);
  });

  it.each([
    ["from the Library Link", { order_source: "library" as const, song_order: [1, 2, 3, 4, 5, 6], song_count: 6 }, "From the Library Link (6 songs)"],
    ["from a scan", { order_source: "scan" as const, song_order: [1, 2, 3, 4, 5, 6], song_count: 6 }, "From a scan (6 songs)"],
    ["from a scan, one song", { order_source: "scan" as const, song_order: [1], song_count: 1 }, "From a scan (1 song)"],
    ["learning", { song_order: [], song_count: 6, order_mode: "learned" as const, order_source: "learned" as const, learned_songs: [] }, "Learning as songs play"],
    ["a library order the setlist has left behind", { order_source: "library" as const, stale: true }, "Needs a new scan"],
  ])("summary says where the order came from: %s", (_name, extra, text) => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus(extra) })} />);
    expect(screen.getByTestId("song-order-summary")).toHaveTextContent(text);
    expect(screen.getByTestId("song-order-summary").textContent).toBe(text);
  });

  it("says in one plain line why the Library Link's order is not used, and nothing when it is", () => {
    const line = "Not using the Library Link's order: Faithfulness is not in Playback's setlist.";
    const view = render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ order_source: "none", song_order: [], song_count: 0, order_library_note: line }) })} />);
    expect(screen.getByTestId("order-library-note")).toHaveTextContent(line);
    // Information only: the scan button and the rest of the section stay exactly as they are.
    expect(screen.getByRole("button", { name: "Scan Playback API" })).toBeEnabled();
    view.unmount();
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ order_source: "library", order_library_note: null }) })} />);
    expect(screen.queryByTestId("order-library-note")).toBeNull();
    expect(screen.getByTestId("order-mode")).toHaveTextContent("Order from the Library Link");
  });

  it("uses the Scan Playback API wording in the dialog and the activity line", async () => {
    const user = userEvent.setup();
    const playback = controller({ status: playbackStatus(), events: [{ event: { type: "song.started", timestamp: 1, song_id: 101, position: 0, previous_song_id: null, continues_playing: false, playing: true, pad: false, setlist_version: 1, previous_version: null, section_id: null, active: null, reason: null, message_kind: null }, discovery: true }] });
    render(<PlaybackApiPanel playback={playback} />);
    await user.click(screen.getByRole("button", { name: "Scan Playback API" }));
    const dialog = screen.getByRole("dialog", { name: "Scan Playback API?" });
    expect(within(dialog).getByRole("button", { name: "Scan Playback API" })).toHaveFocus();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(document.body.textContent).not.toMatch(/setting up song order|Set up song order|Set up again/);
  });
});

describe("Learned song order", () => {
  const learned = (extra = {}) => playbackStatus({
    song_order: [], song_count: 6, plan_song_count: 6, order_mode: "learned",
    learned_songs: [{ song_id: 7, plan_position: 0, confirmed: true, doubtful: false }, { song_id: 9, plan_position: 1, confirmed: false, doubtful: false }],
    settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, learn_order: true }, ...extra,
  });

  it("learned: says the order is being learned, with the count, and never says saved", () => {
    render(<PlaybackApiPanel playback={controller({ status: learned() })} />);
    expect(screen.getByTestId("order-mode")).toHaveTextContent("Learning the order as songs play (2 of 6 placed)");
    expect(screen.queryByText(/Song order saved/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Scan Playback API" })).toBeVisible();
  });

  it("scanned: Order scanned", () => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ order_mode: "scanned", learned_songs: [] }) })} />);
    expect(screen.getByTestId("order-mode")).toHaveTextContent("Order scanned");
  });

  it("none: asks for Up next or a scan", () => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ song_order: [], song_count: 0, order_mode: "none", learned_songs: [], settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, learn_order: true } }) })} />);
    expect(screen.getByTestId("order-mode")).toHaveTextContent("No order yet - set Up next or scan");
  });

  it("shows the note and the doubtful-song hint only when they apply", () => {
    const view = render(<PlaybackApiPanel playback={controller({ status: learned({ order_note: "Playback song not placed; set Up next or run a scan" }) })} />);
    expect(screen.getByText("Playback song not placed; set Up next or run a scan")).toBeVisible();
    expect(screen.queryByText(/Check the order/)).not.toBeInTheDocument();
    view.rerender(<PlaybackApiPanel playback={controller({ status: learned({ learned_songs: [{ song_id: 7, plan_position: 0, confirmed: true, doubtful: true }] }) })} />);
    expect(screen.getByText(/Check the order: a song's length differs from the plan\. Run a scan\./)).toBeVisible();
  });

  it("the switch explains itself and reflects the saved setting", () => {
    const view = render(<PlaybackApiPanel playback={controller({ status: learned() })} />);
    expect(screen.getByRole("switch", { name: "Learn the order as songs play" })).toBeChecked();
    expect(screen.getByText("A scan always replaces the learned order.")).toBeVisible();
    view.rerender(<PlaybackApiPanel playback={controller({ status: learned({ order_mode: "none", learned_songs: [], settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, learn_order: false } }) })} />);
    expect(screen.getByRole("switch", { name: "Learn the order as songs play" })).not.toBeChecked();
  });

  it("toggling saves the whole settings with learn_order flipped, and the switch follows the saved status", async () => {
    const save = vi.fn().mockResolvedValue(true);
    const playback = controller({ status: learned(), save });
    const view = render(<PlaybackApiPanel playback={playback} />);
    await userEvent.setup().click(screen.getByRole("switch", { name: "Learn the order as songs play" }));
    expect(save).toHaveBeenCalledWith({ enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, learn_order: false });
    view.rerender(<PlaybackApiPanel playback={controller({ status: learned({ settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, learn_order: false } }), save })} />);
    expect(screen.getByRole("switch", { name: "Learn the order as songs play" })).not.toBeChecked();
  });

  it("a failed save puts the switch back and shows the controller error", async () => {
    const save = vi.fn().mockResolvedValue(false);
    render(<PlaybackApiPanel playback={controller({ status: learned(), save, error: "Couldn't save" })} />);
    const toggle = screen.getByRole("switch", { name: "Learn the order as songs play" });
    await userEvent.setup().click(toggle);
    expect(save).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(toggle).toBeChecked());
    expect(screen.getByRole("alert")).toHaveTextContent("Couldn't save");
  });

  it("the switch is disabled while a save is running", () => {
    render(<PlaybackApiPanel playback={controller({ status: learned(), pending: "save" })} />);
    expect(screen.getByRole("switch", { name: "Learn the order as songs play" })).toBeDisabled();
  });
});
