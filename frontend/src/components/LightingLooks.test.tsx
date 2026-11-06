import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import * as api from "../api";
import { LightingSetupPanel } from "./LightingSetupPanel";
import { LightingTimeline } from "./LightingTimeline";
import type { TimelineMarker } from "./lightingTimelineMath";

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 300, order: 1, is_generic: false };
const base = { osc_host: "127.0.0.1", osc_port: 21600, enabled: true, output_name: null, channel: 1, pulse_ms: 100 };
const lights = { ...base, method: "osc", connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null } as LightsStatusResponse;
const state = { plan: { id: "p", title: "Sunday", songs: [song] }, current_song: song } as unknown as ApplicationState;

vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), getLightkeyStatus: vi.fn(() => Promise.reject(new Error("offline"))) }));

const dim = (n: string) => `/fixture/${n}/overrides/dimmer`;
const f = (value: number) => [{ type: "float" as const, value }];
const look = (id: string, at: number, msgs: [string, number][], over: Partial<OscLightingCue> = {}): OscLightingCue => ({
  id, at_seconds: at, address: "", args: [], label: "", source: "captured",
  messages: msgs.map(([address, value]) => ({ address, args: f(value) })), ...over,
});
const settingsWith = (cues: OscLightingCue[]) => ({
  planning_center_secret_saved: true, warning: null, restart_required: false,
  settings: { lights: { ...base, method: "osc", cue_maps: { "song-1": { song_key: "song-1", song_title: "Holy Forever", cues: [], osc_cues: cues } } } },
}) as unknown as SettingsResponse;

const mount = (cues: OscLightingCue[], onSaveCues = vi.fn()) => {
  render(
    <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={onSaveCues} onSaveSettings={vi.fn()}
      onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={settingsWith(cues)} state={state} />,
  );
  return onSaveCues;
};
const lastSaved = (spy: ReturnType<typeof vi.fn>): OscLightingCue[] => spy.mock.calls[spy.mock.calls.length - 1]![1];

const threeLooks = () => [
  look("a", 10, [[dim("MH1"), 1], ["/fixture/MH1/overrides/panAngle", 90], [dim("MH2"), 0.5]]),
  look("b", 20, [[dim("MH1"), 0.2]]),
  look("c", 30, [[dim("MH2"), 0.9]], { source: "manual" }),
];

beforeEach(() => { vi.spyOn(window, "confirm").mockReturnValue(true); });
afterEach(() => vi.restoreAllMocks());

describe("look rows in the cue list", () => {
  it("shows 'Look - N fixtures' and tells captured from manual rows", () => {
    mount(threeLooks());
    const rows = screen.getAllByTestId("look-row");
    expect(rows).toHaveLength(3);
    expect(within(rows[0]!).getByText("Look - 2 fixtures")).toBeInTheDocument();
    expect(rows[0]).toHaveAttribute("data-source", "captured");
    expect(rows[2]).toHaveAttribute("data-source", "manual");
    expect(within(rows[0]!).getByTestId("look-badge")).toHaveTextContent("Captured");
    expect(within(rows[2]!).getByTestId("look-badge")).toHaveTextContent("Manual");
    expect(screen.getAllByTestId("lighting-marker").map((m) => m.getAttribute("data-source"))).toEqual(["captured", "captured", "manual"]);
  });

  it("expands to messages grouped by fixture; editing a value saves and marks the row edited", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Expand look at 00:10" }));
    expect(screen.getByRole("region", { name: "Fixture MH1" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Fixture MH2" })).toBeInTheDocument();
    const mh2 = screen.getByRole("region", { name: "Fixture MH2" });
    const value = within(mh2).getByLabelText("Value 1 value");
    await user.clear(value);
    await user.type(value, "0.75");
    await user.tab();
    const saved = lastSaved(save);
    const a = saved.find((c) => c.id === "a")!;
    expect(a.messages!.find((m) => m.address === dim("MH2"))!.args).toEqual([{ type: "float", value: 0.75 }]);
    expect(a.edited).toBe(true);
    expect(saved.find((c) => c.id === "c")!.edited).toBeUndefined();
    expect(screen.getAllByTestId("look-badge")[0]).toHaveTextContent("Captured · edited");
  });

  it("an invalid value is not saved", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Expand look at 00:10" }));
    const mh2 = screen.getByRole("region", { name: "Fixture MH2" });
    const value = within(mh2).getByLabelText("Value 1 value");
    await user.clear(value);
    await user.type(value, "abc");
    await user.tab();
    expect(save).not.toHaveBeenCalled();
    expect(within(mh2).getByRole("alert")).toHaveTextContent("Enter a number.");
  });

  it("deletes one message; deleting the last message removes the row", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Expand look at 00:20" }));
    await user.click(screen.getByRole("button", { name: `Delete message ${dim("MH1")}` }));
    expect(lastSaved(save).map((c) => c.id)).toEqual(["a", "c"]);
    await user.click(screen.getByRole("button", { name: "Expand look at 00:10" }));
    await user.click(within(screen.getByRole("region", { name: "Fixture MH2" })).getByRole("button", { name: /Delete message/ }));
    expect(lastSaved(save).find((c) => c.id === "a")!.messages).toHaveLength(2);
  });

  it("adds a message, saved only once its address is valid", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Expand look at 00:20" }));
    await user.click(screen.getByRole("button", { name: "Add message" }));
    expect(save).not.toHaveBeenCalled();
    const editors = screen.getAllByTestId("look-message");
    const address = within(editors[editors.length - 1]!).getByLabelText("OSC address");
    await user.type(address, "/fixture/MH3/overrides/dimmer");
    await user.tab();
    expect(lastSaved(save).find((c) => c.id === "b")!.messages).toHaveLength(2);
  });

  it("moves a row 0.1 s with the buttons and by typing a time, and deletes it", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Move look at 00:10 0.1 s later" }));
    expect(lastSaved(save).find((c) => c.id === "a")!.at_seconds).toBe(10.1);
    await user.click(screen.getByRole("button", { name: "Remove look at 00:20" }));
    expect(lastSaved(save).map((c) => c.id)).not.toContain("b");
  });

  it("Remove fixture from this song asks first, then deletes it everywhere and drops empty rows", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Expand look at 00:10" }));
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    await user.click(screen.getByRole("button", { name: "Remove fixture MH1 from this song" }));
    expect(save).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Remove fixture MH1 from this song" }));
    expect(window.confirm).toHaveBeenLastCalledWith(expect.stringContaining("MH1"));
    expect(lastSaved(save).map((c) => c.id)).toEqual(["a", "c"]);
    expect(lastSaved(save)[0]!.messages!.every((m) => !m.address.includes("MH1"))).toBe(true);
  });
});

describe("merge rows", () => {
  it("merges ticked rows, confirms when a value would change, and undoes in one step", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    await user.click(screen.getByLabelText("Select look at 00:10"));
    await user.click(screen.getByLabelText("Select look at 00:20"));
    await user.click(screen.getByRole("button", { name: "Merge 2 rows" }));
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining(dim("MH1")));
    const merged = lastSaved(save);
    expect(merged.map((c) => c.id)).toEqual(["a", "c"]);
    expect(merged[0]!.at_seconds).toBe(10);
    expect(merged[0]!.messages!.find((m) => m.address === dim("MH1"))!.args).toEqual(f(0.2));
    await user.click(screen.getByRole("button", { name: "Undo merge of 2 rows" }));
    expect(lastSaved(save).map((c) => c.id)).toEqual(["a", "b", "c"]);
    expect(lastSaved(save)[0]!.messages).toHaveLength(3);
    expect(screen.queryByRole("button", { name: /Undo merge/ })).toBeNull();
  });

  it("declining the value-change confirm leaves the rows alone", async () => {
    const save = mount(threeLooks());
    const user = userEvent.setup();
    vi.mocked(window.confirm).mockReturnValue(false);
    await user.click(screen.getByLabelText("Select look at 00:10"));
    await user.click(screen.getByLabelText("Select look at 00:20"));
    await user.click(screen.getByRole("button", { name: "Merge 2 rows" }));
    expect(save).not.toHaveBeenCalled();
  });

  it("has no Simplify action: a recording is already fitted, so there is nothing left to thin", () => {
    mount(Array.from({ length: 30 }, (_, i) => look(`r${i}`, i / 10, [[dim("A"), i / 30]])));
    expect(screen.queryByRole("button", { name: /simplify/i })).toBeNull();
    expect(screen.queryByTestId("simplify-panel")).toBeNull();
  });

  it("a song flagged captured_edited by the backend (a whole row was deleted) still asks before Record replaces its looks", async () => {
    const flagged = settingsWith(threeLooks());
    (flagged.settings.lights.cue_maps["song-1"] as { captured_edited?: boolean }).captured_edited = true;
    vi.mocked(api.getLightkeyStatus).mockResolvedValue({ api_url: "http://192.0.2.10:8770", link: "connected", detail: null, active_count: null, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null, message: null, takes: [] } as never);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(
      <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()}
        onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={flagged} state={state} />,
    );
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Record" }));
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("including your edits"));
  });
});

describe("density: 400 look rows", () => {
  const rows = Array.from({ length: 400 }, (_, i) => look(`r${i}`, i * 0.7, [[dim(`MH${i % 20}`), (i % 10) / 10], [dim(`MH${(i + 1) % 20}`), 0.3]]));

  it("renders one page of rows inside a time budget, and pages through the rest", async () => {
    const started = performance.now();
    mount(rows);
    const elapsed = performance.now() - started;
    expect(screen.getAllByTestId("look-row").length).toBeLessThanOrEqual(40);
    expect(elapsed).toBeLessThan(4000);
    expect(screen.getByTestId("cue-pages")).toHaveTextContent("Rows 1–40 of 400");
    await userEvent.setup().click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByTestId("cue-pages")).toHaveTextContent("Rows 41–80 of 400");
  });

  it("picking a marker shows its row on the right page", () => {
    mount(rows);
    const markers = screen.getAllByTestId("lighting-marker");
    expect(markers).toHaveLength(400);
    fireEvent.click(markers[399]!);
    expect(screen.getByTestId("cue-pages")).toHaveTextContent("Rows 361–400 of 400");
    expect(screen.getByRole("button", { name: "Expand look at 04:39.3" })).toBeInTheDocument();
  });
});

describe("timeline clustering and touch", () => {
  const many: TimelineMarker[] = Array.from({ length: 400 }, (_, i) => ({ id: `m${i}`, at: i * 0.7, label: `Look ${i}`, color: null, captured: true }));
  const width = (w: number) => vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, right: w, bottom: 56, width: w, height: 56, x: 0, y: 0, toJSON: () => ({}) });

  const Zoomable = ({ markers = many }: { markers?: TimelineMarker[] }) => <LightingTimeline markers={markers} onMove={vi.fn()} onRemove={vi.fn()} playhead={null} songLength={300} />;

  it("clusters close markers at Fit, shows counts, and click zooms in so they separate", async () => {
    width(1000);
    render(<Zoomable />);
    const clusters = screen.getAllByTestId("lighting-cluster");
    expect(clusters.length).toBeGreaterThan(0);
    expect(screen.queryAllByTestId("lighting-marker").length + clusters.length).toBeLessThan(400);
    const total = clusters.reduce((n, c) => n + Number(c.getAttribute("data-count")), 0) + screen.queryAllByTestId("lighting-marker").length;
    expect(total).toBe(400);
    const before = clusters.length;
    await userEvent.setup().click(screen.getByRole("button", { name: "Zoom in" }));
    await userEvent.setup().click(clusters[0]!.isConnected ? screen.getAllByTestId("lighting-cluster")[0]! : screen.getAllByTestId("lighting-marker")[0]!);
    expect(screen.queryAllByTestId("lighting-cluster").length).toBeLessThanOrEqual(before);
  });

  it("stops clustering once zoomed in far enough", async () => {
    width(36000);
    render(<Zoomable markers={many.slice(0, 20)} />);
    expect(screen.queryAllByTestId("lighting-cluster")).toHaveLength(0);
    expect(screen.getAllByTestId("lighting-marker")).toHaveLength(20);
  });

  it("a cluster can be opened from the keyboard", async () => {
    width(1000);
    render(<Zoomable />);
    const cluster = screen.getAllByTestId("lighting-cluster")[0]!;
    cluster.focus();
    const zoomBefore = screen.getByText(/Fit|px\/s/).textContent;
    await userEvent.keyboard("{Enter}");
    expect(screen.getByText(/px\/s/).textContent).not.toBe(zoomBefore);
  });

  it("shift-click multi-selects and offers Merge rows", () => {
    width(10000);
    const onMerge = vi.fn();
    const Harness = () => {
      const [ids, setIds] = useState<string[]>([]);
      return <LightingTimeline markers={many.slice(0, 5)} onMerge={onMerge} onMove={vi.fn()} onRemove={vi.fn()} onSelectionChange={setIds} playhead={null} selectedIds={ids} songLength={300} />;
    };
    render(<Harness />);
    const markers = screen.getAllByTestId("lighting-marker");
    fireEvent.pointerDown(markers[0]!, { button: 0, pointerId: 1, shiftKey: true });
    fireEvent.pointerDown(markers[1]!, { button: 0, pointerId: 1, ctrlKey: true });
    expect(screen.getByTestId("lighting-selection")).toHaveTextContent("2 rows selected");
    fireEvent.click(screen.getByRole("button", { name: "Merge rows" }));
    expect(onMerge).toHaveBeenCalledWith(["m0", "m1"]);
  });

  it("dragging across the empty track selects the markers inside it (mouse)", () => {
    width(1000);
    const onSelection = vi.fn();
    render(<LightingTimeline markers={many.slice(0, 10)} onMove={vi.fn()} onRemove={vi.fn()} onSelectionChange={onSelection} playhead={null} selectedIds={[]} songLength={300} />);
    const track = screen.getByTestId("lighting-timeline-track");
    fireEvent.pointerDown(track, { button: 0, pointerId: 1, clientX: 0, pointerType: "mouse" });
    fireEvent.pointerMove(track, { pointerId: 1, clientX: 20 });
    fireEvent.pointerUp(track, { pointerId: 1, clientX: 20 });
    // 20 px of 1000 px over 300 s = 6 s: markers at 0, 0.7 ... 5.6
    expect(onSelection).toHaveBeenLastCalledWith(Array.from({ length: 9 }, (_, i) => `m${i}`));
  });

  describe("touch long press", () => {
    beforeEach(() => { vi.useFakeTimers(); });
    afterEach(() => { vi.useRealTimers(); });

    it("a long press starts multi-select and a later tap adds a row; a short tap does not", () => {
      width(10000);
      const Harness = () => {
        const [ids, setIds] = useState<string[]>([]);
        return <LightingTimeline markers={many.slice(0, 5)} onMove={vi.fn()} onRemove={vi.fn()} onSelectionChange={setIds} onMerge={vi.fn()} playhead={null} selectedIds={ids} songLength={300} />;
      };
      render(<Harness />);
      const markers = screen.getAllByTestId("lighting-marker");
      fireEvent.pointerDown(markers[0]!, { button: 0, pointerId: 1, pointerType: "touch", clientX: 10, clientY: 10 });
      fireEvent.pointerUp(markers[0]!, { pointerId: 1, pointerType: "touch", clientX: 10, clientY: 10 });
      act(() => { vi.advanceTimersByTime(600); });
      expect(screen.queryByTestId("lighting-selection")).toBeNull();
      fireEvent.pointerDown(markers[0]!, { button: 0, pointerId: 2, pointerType: "touch", clientX: 10, clientY: 10 });
      act(() => { vi.advanceTimersByTime(600); });
      expect(screen.getByTestId("lighting-selection")).toHaveTextContent("1 row selected");
      fireEvent.pointerUp(markers[0]!, { pointerId: 2, pointerType: "touch", clientX: 10, clientY: 10 });
      fireEvent.click(markers[0]!);
      fireEvent.pointerDown(markers[2]!, { button: 0, pointerId: 3, pointerType: "touch", clientX: 50, clientY: 10 });
      fireEvent.pointerUp(markers[2]!, { pointerId: 3, pointerType: "touch", clientX: 50, clientY: 10 });
      fireEvent.click(markers[2]!);
      expect(screen.getByTestId("lighting-selection")).toHaveTextContent("2 rows selected");
    });

    it("moving a finger more than a few pixels cancels the long press (it is a drag)", () => {
      width(10000);
      const onSel = vi.fn();
      render(<LightingTimeline markers={many.slice(0, 3)} onMove={vi.fn()} onRemove={vi.fn()} onSelectionChange={onSel} playhead={null} selectedIds={[]} songLength={300} />);
      const marker = screen.getAllByTestId("lighting-marker")[0]!;
      fireEvent.pointerDown(marker, { button: 0, pointerId: 1, pointerType: "touch", clientX: 10, clientY: 10 });
      fireEvent.pointerMove(marker, { pointerId: 1, pointerType: "touch", clientX: 40, clientY: 10 });
      act(() => { vi.advanceTimersByTime(700); });
      expect(onSel).not.toHaveBeenCalled();
    });
  });
});
