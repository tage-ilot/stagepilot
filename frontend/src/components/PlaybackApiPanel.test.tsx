import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { playbackSettings, playbackState, playbackStatus } from "../test/playbackFixtures";
import { MidiSetupPanel } from "./MidiSetupPanel";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { PlaybackSaveFooter, usePlaybackDraft } from "./PlaybackSaveFooter";
import { validPlaybackHost } from "./playbackStatus";
import { SetupChecklist } from "./SetupChecklist";
import { buildConnectionCardViews, buildReadinessChecks } from "./dashboard/dashboardReadiness";

const scanOf = (state: "idle" | "scanning" | "found" | "not_found", extra = {}) => ({ state, candidates: [], reason: null, current: 0, total: 0, ...extra });
const none = (overrides = {}) => playbackStatus({
  connected: false, active_source: "none", reason: "Not connected. Choose Scan Network to find Playback.",
  sources: { playback_api: { connected: false, reason: "Playback is not connected." }, midi: { connected: false, reason: "MIDI is not connected." } },
  ...overrides,
});
const controller = (overrides: Partial<PlaybackController> = {}): PlaybackController => ({
  status: playbackStatus(), events: [], error: null, message: null, pending: null,
  save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), ...overrides,
});
const panel = (playback = controller(), extra: { onDirtyChange?: (dirty: boolean) => void } = {}) => render(<MidiSetupPanel
  playback={playback} midi={null} messages={[]} error={null} message={null}
  pendingOperation={null} pendingCue={null} onRefresh={vi.fn()} onSelect={vi.fn()} onSimulate={vi.fn()}
  settings={playbackSettings} onSaveSettings={vi.fn()} {...extra}
/>);

describe("Scan Network is always first and never dead", () => {
  it.each([
    ["fresh install, nothing configured", none({ selected: true, settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true } })],
    ["MIDI-selected install", none({ selected: false, enabled: false, active_source: "midi", connected: true, reason: "Connected through MIDI (backup). Find Playback for full control.", settings: { enabled: false, host: null, port: 8080, auto_scan: true, fast_transport: true } })],
    ["Playback switched off", none({ enabled: false, settings: { enabled: false, host: null, port: 8080, auto_scan: true, fast_transport: true } })],
  ])("%s", (_name, status) => {
    const playback = controller({ status });
    panel(playback);
    const scan = screen.getByRole("button", { name: "Scan Network" });
    expect(scan).toBeEnabled();
    const primary = screen.getAllByRole("button").find((b) => !/Close/.test(b.textContent ?? "") && !/Close/.test(b.getAttribute("aria-label") ?? ""));
    expect(primary).toBe(scan);
    fireEvent.click(scan);
    expect(playback.scan).toHaveBeenCalledWith();
  });

  it("shows scanning with progress and a working cancel, then returns to Scan again", () => {
    const playback = controller({ status: none({ scan: scanOf("scanning", { current: 64, total: 254 }), reason: "Looking for Playback…" }), pending: "scan" });
    const view = render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByTestId("playback-banner")).toHaveTextContent("Looking for Playback…");
    expect(screen.getByText(/Checked 64 of 254 addresses/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Scanning… (cancel)" }));
    expect(playback.cancelScan).toHaveBeenCalledOnce();
    view.rerender(<PlaybackApiPanel playback={controller({ status: none({ scan: scanOf("not_found", { reason: "x" }), reason: "Couldn't find Playback." }) })} />);
    expect(screen.queryByText(/Scanning/)).not.toBeInTheDocument();
  });

  it("not found: plain checklist, Scan again and Enter address instead; never a stuck spinner", async () => {
    const playback = controller({ status: none({ scan: scanOf("not_found"), reason: "Couldn't find Playback. Check things." }) });
    render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByText("Couldn't find Playback")).toBeVisible();
    expect(screen.getByText("Remote Connections is turned on in Playback")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Scan again" }));
    expect(playback.scan).toHaveBeenCalledWith();
    await userEvent.setup().click(screen.getByRole("button", { name: "Enter address instead" }));
    expect(screen.getByLabelText("Computer name or address")).toBeVisible();
  });

  it("several found: pick one by computer name", () => {
    const playback = controller({ status: none({ scan: scanOf("found", { candidates: [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }] }) }) });
    render(<PlaybackApiPanel playback={playback} />);
    const group = screen.getByRole("group", { name: "Choose which Playback" });
    fireEvent.click(within(group).getByRole("button", { name: /Other PC/ }));
    expect(playback.scan).toHaveBeenCalledWith("192.0.2.2");
  });
});

describe("Computer select with Other computer…", () => {
  const user = () => userEvent.setup();
  const computer = () => screen.getByLabelText("Computer");
  const withHosts = (host: string | null, candidates: { name: string; host: string }[] = []) => controller({ status: none({
    scan: scanOf(candidates.length ? "found" : "idle", { candidates }),
    settings: { enabled: true, host, port: 8080, auto_scan: true, fast_transport: true },
  }) });

  it("lists scan candidates, the saved host when not among them, and Other computer… last", () => {
    render(<PlaybackApiPanel playback={withHosts("192.0.2.9", [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }])} />);
    expect(computer().tagName).toBe("SELECT");
    expect(within(computer()).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "Example Mac (192.0.2.1)", "Other PC (192.0.2.2)", "192.0.2.9 (saved)", "Other computer…"]);
    expect(computer()).toHaveValue("192.0.2.9");
  });

  it("does not repeat a saved host that is already a candidate", () => {
    render(<PlaybackApiPanel playback={withHosts("192.0.2.2", [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }])} />);
    expect(screen.getAllByRole("option", { name: /192\.0\.2\.2/ })).toHaveLength(1);
    expect(computer()).toHaveValue("192.0.2.2");
  });

  it("empty candidates and no saved host: only Other computer… and nothing selected", () => {
    render(<PlaybackApiPanel playback={withHosts(null)} />);
    expect(within(computer()).getAllByRole("option").map((o) => o.textContent)).toEqual(["Other computer…"]);
    expect(computer()).toHaveValue("");
    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Computer name or address")).not.toBeInTheDocument();
  });

  it("the old expander text is gone", () => {
    render(<PlaybackApiPanel playback={withHosts(null)} />);
    expect(screen.queryByText(/Connect a specific computer/)).not.toBeInTheDocument();
    expect(document.getElementById("playback-specific")).not.toHaveAttribute("aria-expanded");
  });

  it("choosing a listed computer then Connect scans exactly that host; changing the select alone calls nothing", async () => {
    const playback = withHosts(null, [{ name: "Example Mac", host: "192.0.2.1" }, { name: "Other PC", host: "192.0.2.2" }]);
    const u = user();
    render(<PlaybackApiPanel playback={playback} />);
    await u.selectOptions(computer(), "192.0.2.2");
    expect(playback.scan).not.toHaveBeenCalled();
    await u.click(screen.getByRole("button", { name: "Connect" }));
    expect(playback.scan).toHaveBeenCalledExactlyOnceWith("192.0.2.2");
  });

  it("Other computer… shows the address field; Enter connects in one action; blank cannot connect", async () => {
    const playback = withHosts(null);
    const u = user();
    render(<PlaybackApiPanel playback={playback} />);
    await u.selectOptions(computer(), "__other__");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(screen.getByText("Type an address to connect.")).toBeVisible();
    await u.type(screen.getByLabelText("Computer name or address"), "  192.0.2.10  {Enter}");
    expect(playback.scan).toHaveBeenCalledExactlyOnceWith("192.0.2.10");
    expect(screen.queryByText(/Save address|Find \/ Scan|override/)).not.toBeInTheDocument();
  });

  it.each(["http://192.0.2.10", "192.0.2.10:8080", "bad host", "999.0.2.10", "a/b"])("rejects invalid address %s inline", async (value) => {
    const playback = withHosts(null);
    render(<PlaybackApiPanel playback={playback} />);
    await user().selectOptions(computer(), "__other__");
    fireEvent.change(screen.getByLabelText("Computer name or address"), { target: { value } });
    expect(screen.getByRole("alert")).toHaveTextContent("no http://");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    fireEvent.submit(screen.getByLabelText("Computer name or address").closest("form")!);
    expect(playback.scan).not.toHaveBeenCalled();
  });

  it("Enter address instead (not found) selects Other computer… and opens the address field", async () => {
    render(<PlaybackApiPanel playback={controller({ status: none({ scan: scanOf("not_found") }) })} />);
    expect(screen.queryByLabelText("Computer name or address")).not.toBeInTheDocument();
    await user().click(screen.getByRole("button", { name: "Enter address instead" }));
    expect(computer()).toHaveValue("__other__");
    expect(screen.getByLabelText("Computer name or address")).toBeVisible();
  });

  it("Enter address instead in a scan error (refused, permission denied) opens the same field", async () => {
    for (const error_class of ["refused", "permission_denied"]) {
      const scan = scanOf("not_found", { error_class, typed: false, reason: "x", details: "{}" });
      const { unmount } = render(<PlaybackApiPanel playback={controller({ status: none({ scan }) })} />);
      await user().click(screen.getByRole("button", { name: "Enter address instead" }));
      expect(computer()).toHaveValue("__other__");
      expect(screen.getByLabelText("Computer name or address")).toBeVisible();
      unmount();
    }
  });
});

describe("validPlaybackHost", () => {
  it.each(["", "192.0.2.10", "playback.local", "127.0.0.1", "::1"])("accepts bare host %s", (host) => expect(validPlaybackHost(host)).toBe(true));
});

describe("Song order card", () => {
  it("not connected: explanation instead of a dead button", () => {
    render(<PlaybackApiPanel playback={controller({ status: none() })} />);
    expect(screen.getByText("Connect to Playback first.")).toBeVisible();
    expect(screen.queryByRole("button", { name: /song order/i })).not.toBeInTheDocument();
  });

  it("connected without order: one button, confirmation dialog, then discover", async () => {
    const playback = controller({ status: playbackStatus({ song_order: [], stale: true, captured_version: null }) });
    const user = userEvent.setup();
    render(<PlaybackApiPanel playback={playback} />);
    expect(screen.getByText(/steps through your Playback songs once/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Set up song order" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("must stay stopped");
    expect(dialog).toHaveTextContent("read their lengths");
    expect(dialog).toHaveTextContent("Nothing will be played.");
    expect(dialog).not.toHaveTextContent("2 seconds");
    expect(playback.discover).not.toHaveBeenCalled();
    expect(within(dialog).getByRole("button", { name: "Set up song order" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Set up song order" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Set up song order" }));
    expect(playback.discover).toHaveBeenCalledOnce();
  });

  it("works with nothing typed or saved: no address and no save needed", () => {
    render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ song_order: [], stale: true, settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true } }) })} />);
    expect(screen.getByRole("button", { name: "Set up song order" })).toBeEnabled();
  });

  it("playing: replaced by an explanation, not a dead control; an open dialog closes", () => {
    const view = render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ song_order: [], stale: true }) })} />);
    fireEvent.click(screen.getByRole("button", { name: "Set up song order" }));
    view.rerender(<PlaybackApiPanel playback={controller({ status: playbackStatus({ playing: true, song_order: [], stale: true }) })} />);
    expect(screen.getByText("Stop Playback to set up song order.")).toBeVisible();
    expect(screen.queryByRole("button", { name: /song order/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("stale: amber message with the same one button; saved: confirmation count", () => {
    const view = render(<PlaybackApiPanel playback={controller({ status: playbackStatus({ stale: true }) })} />);
    expect(screen.getByText("Your Playback setlist changed. Set up song order again.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Set up song order" })).toBeEnabled();
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
      plan_songs: [{ title: "Plan title", duration_seconds: 180 }],
    }) })} />);
    expect(screen.getAllByText("Playback has 2 songs, this service plan has 1. Playback song 2 is ignored.")).toHaveLength(1);
    expect(screen.getByRole("group", { name: "Song lengths" })).toBeVisible();
    expect(screen.getByText("1. Plan title")).toBeVisible();
  });
});

describe("Recent activity", () => {
  it("is an activity Disclosure that opens and closes from its button", async () => {
    render(<PlaybackApiPanel playback={controller()} />);
    const toggle = screen.getByRole("button", { name: /^Recent activity\s*0 events$/ });
    expect(toggle.closest("[data-variant='activity']")).toHaveAttribute("data-state", "closed");
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Nothing yet.")).toBeVisible();
  });
  it("shows the event count on the activity Disclosure (singular and plural)", () => {
    const ev = { type: "song.started", timestamp: 1, song_id: 202, position: 0, previous_song_id: null, continues_playing: false, playing: true, pad: false, setlist_version: 1, previous_version: null, section_id: null, active: null, reason: null, message_kind: null };
    const { unmount } = render(<PlaybackApiPanel playback={controller({ events: [{ event: ev, discovery: false }] })} />);
    expect(screen.getByRole("button", { name: /^Recent activity\s*1 event$/ })).toBeInTheDocument();
    unmount();
    render(<PlaybackApiPanel playback={controller({ events: [{ event: ev, discovery: false }, { event: ev, discovery: false }] })} />);
    expect(screen.getByRole("button", { name: /^Recent activity\s*2 events$/ })).toBeInTheDocument();
  });
  const event = { type: "song.started", timestamp: 123.4, song_id: 202, position: 0, previous_song_id: null, continues_playing: false, playing: true, pad: false, setlist_version: 1, previous_version: null, section_id: null, active: null, reason: null, message_kind: null };
  it("renders transport rollback as plain activity, without a song number", () => {
    render(<PlaybackApiPanel playback={controller({ events: [{ event: { ...event, type: "transport.reverted", provisional: false }, discovery: false }] })} />);
    expect(screen.getByRole("list", { name: "Recent activity", hidden: true })).toHaveTextContent("Playback did not confirm the command; transport reverted.");
  });
  it("uses plain rows and wall-clock time, no monotonic wording; unknown songs are never numbered", () => {
    render(<PlaybackApiPanel playback={controller({ events: [
      { event, discovery: false, at: "2026-01-01T10:00:00Z" },
      { event: { ...event, type: "song.paused", song_id: 999 }, discovery: false, at: "2026-01-01T10:01:00Z" },
      { event: { ...event, type: "song.stopped" }, discovery: false, at: "2026-01-01T10:02:00Z" },
    ] })} />);
    const list = screen.getByRole("list", { name: "Recent activity", hidden: true });
    expect(list).toHaveTextContent("Song 2 started");
    expect(list).toHaveTextContent("Paused");
    expect(list).toHaveTextContent("Stopped");
    expect(document.body.textContent).not.toMatch(/monotonic|velocit/i);
  });
});

describe("Main view has no jargon", () => {
  it("avoids API, monotonic, velocity, manual address", () => {
    panel(controller());
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/monotonic|manual address|velocities/i);
  });
});

describe("Connection method select (Playback API main / MIDI alternate)", () => {
  const validMidi = { ...playbackSettings, settings: { ...playbackSettings.settings, midi: { ...playbackSettings.settings.midi, mappings: { start_next: 100, restart_current: 101, previous: 102, next: 103, reload_plan: 104, stop_timer: 105 } } } };
  const midiStatus = () => playbackStatus({ sources: { playback_api: { connected: false, reason: "Playback is not connected." }, midi: { connected: true, reason: "MIDI is connected." } } });
  const method = () => screen.getByLabelText("Connection method");

  it("opens on the saved method with one labelled Field + Select and the two options", () => {
    panel();
    expect(method().tagName).toBe("SELECT");
    expect(method()).toHaveValue("playback_api");
    expect(screen.getByRole("option", { name: "Playback API (main)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "MIDI (alternate)" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Alternate connection: MIDI settings/ })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("MIDI channel")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Scan Network" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Connection method" })).not.toBeInTheDocument();
  });

  it("switching the select swaps the fields as a view and calls nothing", async () => {
    const playback = controller();
    const user = userEvent.setup();
    panel(playback);
    await user.selectOptions(method(), "real");
    expect(playback.selectSource).not.toHaveBeenCalled();
    expect(playback.save).not.toHaveBeenCalled();
    expect(screen.getByLabelText("MIDI channel")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Scan Network" })).not.toBeInTheDocument();
    await user.selectOptions(method(), "playback_api");
    expect(screen.getByRole("button", { name: "Scan Network" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save settings" })).toBeDisabled();
  });

  it("holds the method in the draft: unsaved, reported dirty, and discard restores the saved one", async () => {
    const onDirtyChange = vi.fn();
    const user = userEvent.setup();
    panel(controller(), { onDirtyChange });
    await user.selectOptions(method(), "real");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    expect(screen.getByRole("button", { name: "Save settings" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Discard changes" }));
    expect(method()).toHaveValue("playback_api");
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
    await user.selectOptions(method(), "real");
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    expect(order).toEqual(["selectSource"]);
    expect(playback.selectSource).toHaveBeenCalledWith("real");
    expect(playback.save).not.toHaveBeenCalled();
  });

  it("method change plus setting change: selectSource then save, in that order", async () => {
    const order: string[] = [];
    const playback = controller({
      selectSource: vi.fn(async () => { order.push("selectSource"); return true; }),
      save: vi.fn(async () => { order.push("save"); return true; }),
    });
    const Host = () => { const draft = usePlaybackDraft(playback, "real"); return <>
      <button onClick={() => { draft.setMethod("playback_api"); draft.setFastTransport(!draft.fastTransport); }} type="button">edit</button>
      <button onClick={() => void draft.save()} type="button">go</button></>; };
    render(<Host />);
    await userEvent.click(screen.getByRole("button", { name: "edit" }));
    await userEvent.click(screen.getByRole("button", { name: "go" }));
    expect(order).toEqual(["selectSource", "save"]);
    expect(playback.selectSource).toHaveBeenCalledWith("playback_api");
  });

  it("stops at the first failure: save is not called and the draft stays", async () => {
    const playback = controller({ selectSource: vi.fn().mockResolvedValue(false), save: vi.fn().mockResolvedValue(true) });
    const user = userEvent.setup();
    panel(playback);
    await user.selectOptions(method(), "real");
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    expect(playback.selectSource).toHaveBeenCalledTimes(1);
    expect(playback.save).not.toHaveBeenCalled();
    expect(method()).toHaveValue("real");
    expect(screen.getByRole("alert")).toHaveTextContent(/not saved/);
    expect(screen.getByRole("button", { name: "Save settings" })).toBeEnabled();
  });

  it("a failing selectSource in the MIDI view leaves Save MIDI settings uncalled", async () => {
    const playback = controller({ selectSource: vi.fn().mockResolvedValue(false) });
    const onSaveSettings = vi.fn();
    const user = userEvent.setup();
    render(<MidiSetupPanel playback={playback} midi={null} messages={[]} error={null} message={null} pendingOperation={null} pendingCue={null} onRefresh={vi.fn()} onSelect={vi.fn()} onSimulate={vi.fn()} settings={validMidi} onSaveSettings={onSaveSettings} />);
    await user.selectOptions(method(), "real");
    await user.click(screen.getByRole("button", { name: "Save MIDI settings" }));
    expect(playback.selectSource).toHaveBeenCalledWith("real");
    expect(onSaveSettings).not.toHaveBeenCalled();
    expect(method()).toHaveValue("real");
  });

  it("Save MIDI settings applies selectSource first, then the MIDI save", async () => {
    const order: string[] = [];
    const playback = controller({ selectSource: vi.fn(async () => { order.push("selectSource"); return true; }) });
    const onSaveSettings = vi.fn(() => { order.push("saveMidi"); });
    const user = userEvent.setup();
    render(<MidiSetupPanel playback={playback} midi={null} messages={[]} error={null} message={null} pendingOperation={null} pendingCue={null} onRefresh={vi.fn()} onSelect={vi.fn()} onSimulate={vi.fn()} settings={validMidi} onSaveSettings={onSaveSettings} />);
    await user.selectOptions(method(), "real");
    await user.click(screen.getByRole("button", { name: "Save MIDI settings" }));
    expect(order).toEqual(["selectSource", "saveMidi"]);
  });

  it("opens on the saved alternate method and appends it to the header badge only then", () => {
    const saved = { ...playbackSettings, settings: { ...playbackSettings.settings, integration_modes: { ...playbackSettings.settings.integration_modes, midi_source: "real" as const } } };
    const view = render(<MidiSetupPanel playback={controller({ status: midiStatus() })} midi={null} messages={[]} error={null} message={null} pendingOperation={null} pendingCue={null} onRefresh={vi.fn()} onSelect={vi.fn()} onSimulate={vi.fn()} settings={saved} onSaveSettings={vi.fn()} />);
    expect(method()).toHaveValue("real");
    expect(screen.getByLabelText("MIDI channel")).toBeVisible();
    expect(view.container.querySelector("[data-setup-panel-header]")).toHaveTextContent("MIDI (alternate)");
    view.unmount();
    panel();
    expect(document.querySelector("[data-setup-panel-header]")).not.toHaveTextContent("alternate");
  });

  it("shows the legacy simulation source as a disabled option and still opens on it", () => {
    const sim = { ...playbackSettings, settings: { ...playbackSettings.settings, integration_modes: { ...playbackSettings.settings.integration_modes, midi_source: "simulated" as const } } };
    render(<MidiSetupPanel playback={controller()} midi={null} messages={[]} error={null} message={null} pendingOperation={null} pendingCue={null} onRefresh={vi.fn()} onSelect={vi.fn()} onSimulate={vi.fn()} settings={sim} onSaveSettings={vi.fn()} />);
    expect(method()).toHaveValue("simulated");
    expect(screen.getByRole("option", { name: "Simulation (legacy)" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save settings" })).toBeDisabled();
  });
});

const scenarios = [
  { name: "API only", api: true, midi: false, active: "playback_api" as const, reason: "Connected to Playback on 192.0.2.10." },
  { name: "MIDI only", api: false, midi: true, active: "midi" as const, reason: "Connected through MIDI (backup). Find Playback for full control." },
  { name: "both", api: true, midi: true, active: "playback_api" as const, reason: "Connected to Playback on 192.0.2.10." },
  { name: "neither", api: false, midi: false, active: "none" as const, reason: "Not connected. Choose Scan Network to find Playback." },
  { name: "scanning", api: false, midi: false, active: "none" as const, reason: "Looking for Playback…", scanning: true },
];
describe("Banner, panel, dashboard, checklist and Alternate connection: MIDI settings never contradict", () => {
  it.each(scenarios)("$name", async ({ api, midi, active, reason, scanning }) => {
    const connected = api || midi;
    const status = playbackStatus({
      connected, active_source: active, reason, scan: scanOf(scanning ? "scanning" : "idle"),
      sources: { playback_api: { connected: api, reason: api ? "Playback is connected." : "Playback is not connected." }, midi: { connected: midi, reason: midi ? "MIDI is connected." : "MIDI is not connected." } },
    });
    panel(controller({ status }));
    // One banner, one sentence: exactly the backend text.
    expect(screen.getByTestId("playback-banner")).toHaveTextContent(reason);
    expect(screen.getAllByText(reason)).toHaveLength(1);
    await userEvent.setup().selectOptions(screen.getByLabelText("Connection method"), "real");
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
    ["not connected", none()],
    ["scanning", none({ scan: scanOf("scanning") })],
    ["not found", none({ scan: scanOf("not_found") })],
    ["connected, no order", playbackStatus({ song_order: [], stale: true })],
    ["playing", playbackStatus({ playing: true })],
    ["stale", playbackStatus({ stale: true })],
    ["discovering", playbackStatus({ discovery: "running" })],
  ] as const;
  it.each(states)("%s", (_name, status) => {
    panel(controller({ status }));
    const footer = screen.getByTestId("playback-save-footer");
    const disabled = screen.getAllByRole("button").filter((button) => (button as HTMLButtonElement).disabled && !footer.contains(button) && !button.closest('[aria-label="Services song times"]'));
    expect(disabled.map((b) => b.textContent)).toEqual([]);
    expect(screen.getAllByRole("button").some((b) => /Scan Network|Scanning|Scan again/.test(b.textContent ?? "") && !(b as HTMLButtonElement).disabled)).toBe(true);
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
  onDirty?.(draft.dirty);
  return <><PlaybackApiPanel playback={playback} draft={draft} /><PlaybackSaveFooter draft={draft} playback={playback} /></>;
}

describe("Save settings footer", () => {
  afterEach(() => vi.useRealTimers());
  const toggle = () => screen.getByLabelText("Connect to Playback automatically");
  const saveButton = () => screen.getByRole("button", { name: /Save settings|Saving…/ });

  it("clean: disabled with 'No changes to save'", () => {
    render(<Harness save={vi.fn().mockResolvedValue(true)} />);
    expect(saveButton()).toBeDisabled();
    expect(screen.getByText("No changes to save")).toBeVisible();
    expect(screen.queryByText("Unsaved changes")).not.toBeInTheDocument();
  });

  it("dirty: enabled, 'Unsaved changes' hint and a Discard link; Discard restores clean", () => {
    render(<Harness save={vi.fn().mockResolvedValue(true)} />);
    fireEvent.click(toggle());
    expect(saveButton()).toBeEnabled();
    expect(screen.getByText("Unsaved changes")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
    expect(toggle()).toBeChecked();
    expect(saveButton()).toBeDisabled();
  });

  it("saving then saved: sends every visible setting once, shows Saving…, then Saved, then returns to disabled", async () => {
    vi.useFakeTimers();
    let finish!: (ok: boolean) => void;
    const save = vi.fn().mockReturnValue(new Promise<boolean>((resolve) => { finish = resolve; }));
    render(<Harness save={save} />);
    fireEvent.click(toggle());
    fireEvent.click(saveButton());
    expect(save).toHaveBeenCalledWith({ enabled: false, host: null, port: 8080, auto_scan: true, fast_transport: true });
    expect(screen.getByRole("button", { name: "Saving…" })).toBeDisabled();
    expect(screen.queryByText("Unsaved changes")).not.toBeInTheDocument();
    await act(async () => finish(true));
    expect(screen.getByText("Saved")).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(2_100); });
    expect(screen.queryByText("Saved")).not.toBeInTheDocument();
    vi.useRealTimers();
  });

  it("error: plain reason, button stays enabled, changes kept", async () => {
    render(<Harness save={vi.fn().mockResolvedValue(false)} />);
    fireEvent.click(toggle());
    await act(async () => { fireEvent.click(saveButton()); });
    expect(screen.getByRole("alert")).toHaveTextContent("not saved");
    expect(saveButton()).toBeEnabled();
    expect(toggle()).not.toBeChecked();
  });

  it("covers every editable field in the visible panel (auto-scan too)", async () => {
    const save = vi.fn().mockResolvedValue(true);
    render(<Harness save={save} />);
    fireEvent.click(screen.getByLabelText(/Look for Playback on the network/));
    await act(async () => { fireEvent.click(saveButton()); });
    expect(save).toHaveBeenCalledWith({ enabled: true, host: null, port: 8080, auto_scan: false, fast_transport: true });
  });

  it("fast transport saves through the shared footer and stays clean after saving", async () => {
    const save = vi.fn().mockResolvedValue(true);
    render(<Harness save={save} />);
    const fast = screen.getByRole("checkbox", { name: "Respond to Play immediately" });
    expect(fast).toBeChecked();
    expect(fast).toHaveAccessibleDescription("Reacts to the command instead of waiting up to one second for Playback to report it.");
    fireEvent.click(fast);
    expect(saveButton()).toBeEnabled();
    await act(async () => { fireEvent.click(saveButton()); });
    expect(save).toHaveBeenCalledExactlyOnceWith({ enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: false });
    expect(fast).not.toBeChecked();
    expect(saveButton()).toBeDisabled();
    expect(screen.getByText("Saved")).toBeVisible();
  });

  it("discard restores fast transport without saving", () => {
    const save = vi.fn();
    render(<Harness save={save} />);
    fireEvent.click(screen.getByLabelText("Respond to Play immediately"));
    fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
    expect(screen.getByLabelText("Respond to Play immediately")).toBeChecked();
    expect(saveButton()).toBeDisabled();
    expect(save).not.toHaveBeenCalled();
  });

  it("failed fast transport save retains the draft", async () => {
    render(<Harness save={vi.fn().mockResolvedValue(false)} />);
    fireEvent.click(screen.getByLabelText("Respond to Play immediately"));
    await act(async () => { fireEvent.click(saveButton()); });
    expect(screen.getByLabelText("Respond to Play immediately")).not.toBeChecked();
    expect(saveButton()).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent("not saved");
  });

  it("Scan Network and Connect are never gated by unsaved edits", async () => {
    const playback = controller();
    const Gate = () => { const draft = usePlaybackDraft(playback); return <><PlaybackApiPanel playback={playback} draft={draft} /><PlaybackSaveFooter draft={draft} playback={playback} /></>; };
    render(<Gate />);
    fireEvent.click(toggle());
    expect(screen.getByRole("button", { name: "Scan Network" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Scan Network" }));
    expect(playback.scan).toHaveBeenCalledWith();
  });

  it("names dirty MIDI fields instead of ignoring them", async () => {
    const user = userEvent.setup();
    panel();
    await user.selectOptions(screen.getByLabelText("Connection method"), "real");
    fireEvent.change(screen.getByLabelText("MIDI channel"), { target: { value: "5" } });
    expect(screen.getByTestId("playback-save-footer")).toHaveTextContent(/unsaved MIDI changes\. They are saved with Save MIDI settings/);
  });

  it("reports dirtiness upward so leaving can ask first", () => {
    const onDirtyChange = vi.fn();
    panel(controller(), { onDirtyChange });
    fireEvent.click(screen.getByLabelText("Connect to Playback automatically"));
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
  });

  it("footer is sticky at the bottom of the card (visible without scrolling)", () => {
    panel();
    expect(screen.getByTestId("playback-save-footer").className).toMatch(/sticky bottom-0/);
  });
});

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
    expect(screen.queryByText("Couldn't find Playback")).toBeNull();
  });

  it("a scanning state is not replaced by not-found before the backend says so", () => {
    const { rerender } = render(<PlaybackApiPanel playback={controller({ status: scanning() })} />);
    expect(screen.queryByText("Couldn't find Playback")).toBeNull();
    expect(screen.getByRole("button", { name: /Scanning/ })).toBeInTheDocument();
    rerender(<PlaybackApiPanel playback={controller({ status: none({ scan: scanOf("not_found", { reason: "x", hosts_probed: 254, hosts_total: 254, networks: ["192.0.2.0/24"], elapsed: 9 }) }) })} />);
    expect(screen.getByText("Couldn't find Playback")).toBeInTheDocument();
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
    expect(screen.queryByText("Couldn't find Playback")).toBeNull();
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
      expect(screen.queryByText("Couldn't find Playback")).toBeNull();
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
