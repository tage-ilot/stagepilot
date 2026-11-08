import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";
import { LightingTimeline } from "./LightingTimeline";
import { capturedItemsText, lightkeyHeadline } from "./LightkeyStrip";
import { formatCount, simplifiedNote, startLookNote } from "./lightingLooks";
import type { TimelineMarker } from "./lightingTimelineMath";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getLightkeyStatus: vi.fn(() => Promise.resolve({ mode: "replay", link: "connected", recording: false, replay: true, api_url: "http://127.0.0.1:8100" })),
}));

/**
 * A song at the saved caps (50,000 rows, 600,000 messages): StagePilot saves it (see docs/lights.md), so the Lighting screens
 * must stay usable with it. jsdom is slow and has no layout, so the first assertions are on how much is DRAWN; the time limits are
 * generous guards against a return to "draw every row", not benchmarks (the PR body has real browser numbers).
 */
const ROWS = 50_000;
const MESSAGES_PER_ROW = 12;
const SONG_SECONDS = 1200;
/** The most DOM elements the panel may hold with the big song open (the whole Lighting card, measured with margin). */
const PANEL_NODE_BOUND = 6_000;
/** The most elements the timeline alone may hold. */
const TIMELINE_NODE_BOUND = 2_500;
const RENDER_LIMIT_MS = 30_000;

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Big Song", duration_seconds: SONG_SECONDS, order: 1, is_generic: false };

const bigRows = (rows = ROWS): OscLightingCue[] => Array.from({ length: rows }, (_, index) => ({
  id: `r${index}`,
  at_seconds: Math.round((index / rows) * SONG_SECONDS * 10) / 10,
  address: "",
  args: [],
  label: "",
  source: "captured" as const,
  ...(index % 5 === 0 ? { fade_seconds: 2 } : {}),
  messages: Array.from({ length: MESSAGES_PER_ROW }, (_, message) => ({
    address: `/fixture/MH${(message % 6) + 1}/overrides/${message < 6 ? "dimmer" : "pan"}`,
    args: [{ type: "float" as const, value: ((index * 7 + message) % 100) / 100 }],
  })),
}));

const lightsFor = (method: "osc" | "midi"): LightsStatusResponse => ({
  osc_host: "127.0.0.1", osc_port: 21600, enabled: true, output_name: "Out", channel: 1, pulse_ms: 100,
  method, connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null,
} as unknown as LightsStatusResponse);

const settingsWith = (osc_cues: OscLightingCue[], capture?: object) => ({
  planning_center_secret_saved: true, warning: null, restart_required: false,
  settings: { lights: { ...lightsFor("osc"), pads: [], cue_maps: { "song-1": { song_key: "song-1", song_title: "Big Song", cues: [], osc_cues, ...(capture ? { capture } : {}) } } } },
}) as unknown as SettingsResponse;

const runningState = {
  plan: { id: "p", title: "Sunday", songs: [song] }, current_song: song, playing_song: song,
  timer: { status: "running", duration_seconds: SONG_SECONDS, started_at: new Date().toISOString(), position_seconds: 100 },
} as unknown as ApplicationState;

const mountPanel = (rows: OscLightingCue[], capture?: object, onSaveCues = vi.fn()) => render(
  <LightingSetupPanel error={null} lights={lightsFor("osc")} message={null} onRefresh={vi.fn()} onSaveCues={onSaveCues} onSaveSettings={vi.fn()} onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={settingsWith(rows, capture)} state={runningState} />,
);

const nodeCount = () => document.body.querySelectorAll("*").length;

beforeEach(() => {
  window.localStorage.clear();
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({ width: 1200, height: 100, top: 0, left: 0, right: 1200, bottom: 100, x: 0, y: 0, toJSON: () => ({}) } as DOMRect);
});
afterEach(() => { vi.restoreAllMocks(); window.localStorage.clear(); });

describe("a song at the saved caps", () => {
  it("draws the timeline with a bounded number of elements and in a bounded time", () => {
    const rows = bigRows();
    const markers: TimelineMarker[] = rows.map((cue) => ({ id: cue.id, at: cue.at_seconds, label: "Look - 6 fixtures", color: null, captured: true, type: "look-captured", weight: MESSAGES_PER_ROW }));
    const started = performance.now();
    render(<LightingTimeline markers={markers} onMove={vi.fn()} onRemove={vi.fn()} playhead={null} songLength={SONG_SECONDS} />);
    const elapsed = performance.now() - started;
    const drawn = screen.queryAllByTestId("lighting-marker").length + screen.queryAllByTestId("lighting-cluster").length;
    console.warn(`[big-song] timeline: ${markers.length} rows -> ${drawn} drawn items, ${nodeCount()} elements, ${elapsed.toFixed(0)} ms`);
    expect(drawn).toBeLessThan(1_300);
    expect(nodeCount()).toBeLessThan(TIMELINE_NODE_BOUND);
    expect(elapsed).toBeLessThan(RENDER_LIMIT_MS);
    // The data is not changed: the note says how many cues are drawn and that all are kept.
    expect(screen.getByTestId("lighting-timeline-thinned")).toHaveTextContent(/of 50,000 cues/);
  });

  it("opens the whole Lighting card with a bounded number of elements, lists one page, and re-renders on the follow-along tick fast", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    try {
      const rows = bigRows();
      const started = performance.now();
      mountPanel(rows);
      const elapsed = performance.now() - started;
      console.warn(`[big-song] panel: ${rows.length} rows -> ${nodeCount()} elements, ${elapsed.toFixed(0)} ms`);
      expect(nodeCount()).toBeLessThan(PANEL_NODE_BOUND);
      expect(elapsed).toBeLessThan(RENDER_LIMIT_MS);
      // The cue list shows one page of 40 rows, not 50,000.
      expect(screen.getAllByTestId("look-row").length).toBeLessThanOrEqual(40);
      // Ten ticks of the follow-along (100 ms each) cost far less than ten full renders of the rows.
      const tickStart = performance.now();
      for (let tick = 0; tick < 10; tick++) await act(async () => { vi.advanceTimersByTime(100); });
      const perTick = (performance.now() - tickStart) / 10;
      console.warn(`[big-song] follow-along tick: ${perTick.toFixed(1)} ms each`);
      expect(perTick).toBeLessThan(2_000);
    } finally { vi.useRealTimers(); }
  });

  it("still edits: Remove on a listed row saves the other rows unchanged", async () => {
    const rows = bigRows(3_000); // above the draw-all limit, so the timeline is thinned
    const onSaveCues = vi.fn();
    mountPanel(rows, undefined, onSaveCues);
    const user = userEvent.setup();
    await user.click(screen.getAllByRole("button", { name: /^Remove look at/ })[0]!);
    expect(onSaveCues).toHaveBeenCalledTimes(1);
    const saved = onSaveCues.mock.calls[0]![1] as OscLightingCue[];
    expect(saved).toHaveLength(2_999);
    expect(saved.every((cue) => cue.messages!.length === MESSAGES_PER_ROW)).toBe(true);
  });
});

describe("the simplified note and the numbers", () => {
  it("adds thousands separators from 10,000 and leaves smaller counts alone", () => {
    expect(formatCount(999)).toBe("999");
    expect(formatCount(9_999)).toBe("9,999");
    expect(formatCount(10_000)).toBe("10,000");
    expect(formatCount(1_234_567)).toBe("1,234,567");
    expect(formatCount(552)).toBe("552");
    expect(capturedItemsText(12_000, 30)).toBe("12,030 captured items (12,000 ramps/steps, 30 loops)");
    expect(capturedItemsText(546, 6)).toBe("552 captured items (546 ramps/steps, 6 loops)");
  });

  it("says in plain words that a capture was simplified, and says nothing otherwise", () => {
    expect(simplifiedNote({ kept_changes: 6_200, recorded_changes: 11_400 })).toBe("Simplified: 6,200 of 11,400 changes kept.");
    expect(simplifiedNote({ kept_changes: 968, recorded_changes: 38_916 })).toBe("Simplified: 968 of 38,916 changes kept.");
    expect(simplifiedNote(undefined)).toBeNull();
    expect(simplifiedNote(null)).toBeNull();
    const status = { mode: "manual", link: "connected" } as never;
    expect(lightkeyHeadline(status, { count: 6_200, edited: false, simplified: { kept_changes: 6_200, recorded_changes: 11_400 } }))
      .toContain("6,200 captured items (6,200 ramps/steps, 0 loops) on this song. Simplified: 6,200 of 11,400 changes kept.");
    expect(lightkeyHeadline(status, { count: 40, edited: false })).not.toMatch(/Simplified/);
  });

  it("shows the note beside the capture count on the Lighting card, and the count with separators", async () => {
    const capture = { complete: true, coverage_seconds: 1200, recorded_at: "2026-11-08T00:00:00Z", simplified: { stage: "trimmed", tolerance_scale: 8, thin_seconds: 1, trimmed: 100, kept_changes: 6_200, recorded_changes: 11_400 } };
    mountPanel(bigRows(12_000), capture);
    expect(await screen.findByText(/12,000 captured items \(12,000 ramps\/steps, 0 loops\) on this song\. Simplified: 6,200 of 11,400 changes kept\./)).toBeInTheDocument();
    expect(screen.getByText(/12,000 cues on 1 of 1 song/)).toBeInTheDocument();
  });
});

describe("the starting look on the capture line", () => {
  const status = { mode: "manual", link: "connected" } as never;

  it("says how many lights the starting look holds, with thousands separators", () => {
    expect(startLookNote(66, true)).toBe("Starting look of 66 lights saved.");
    expect(startLookNote(1, true)).toBe("Starting look of 1 light saved.");
    expect(startLookNote(12_000, true)).toBe("Starting look of 12,000 lights saved.");
    expect(lightkeyHeadline(status, { count: 40, edited: false, startLights: 66 }))
      .toContain("40 captured items (40 ramps/steps, 0 loops) on this song. Starting look of 66 lights saved.");
  });

  it("tells the operator an older capture has no house lights and how to fix it", () => {
    const text = "Recorded before the starting lights were saved. Record this song again to include the house lights.";
    expect(startLookNote(null, true)).toBe(text);
    expect(lightkeyHeadline(status, { count: 40, edited: false, startLights: null })).toContain(`on this song. ${text}`);
  });

  it("says plainly when the starting look could not be read, and nothing when nothing is captured or unknown", () => {
    expect(startLookNote(0, true)).toBe("The starting look of the other lights could not be read.");
    expect(startLookNote(null, false)).toBeNull();
    expect(startLookNote(66, false)).toBeNull();
    expect(startLookNote(undefined, true)).toBeNull();
    expect(lightkeyHeadline(status, { count: 40, edited: false })).not.toMatch(/starting|Starting/);
  });

  it("shows the sentence on the Lighting card for a new capture and for an old one", async () => {
    const base = { complete: true, coverage_seconds: 300, recorded_at: "2026-11-08T00:00:00Z" };
    mountPanel(bigRows(40), { ...base, start_lights: 66 });
    expect(await screen.findByText(/40 captured items .* on this song\. Starting look of 66 lights saved\./)).toBeInTheDocument();
  });

  it("shows the old-capture notice on the Lighting card", async () => {
    mountPanel(bigRows(40), { complete: true, coverage_seconds: 300, recorded_at: "2026-11-08T00:00:00Z" });
    expect(await screen.findByText(/Recorded before the starting lights were saved\. Record this song again to include the house lights\./)).toBeInTheDocument();
  });
});
