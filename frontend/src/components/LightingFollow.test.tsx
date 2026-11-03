import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightsStatusResponse, SettingsResponse, Song } from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";

const mk = (n: number): Song => ({ id: `item-${n}`, source_song_id: `song-${n}`, title: `Song ${n}`, duration_seconds: 200, order: n, is_generic: false });
const [a, b, c] = [mk(1), mk(2), mk(3)];
const lights = {
  enabled: true, method: "midi", osc_host: "127.0.0.1", osc_port: 21600, output_name: "Out", channel: 1, pulse_ms: 100,
  connection_status: "connected", detail: "ok", outputs: [], last_cue: null, last_cue_at: null,
} as unknown as LightsStatusResponse;
const base = {
  revision: 1, updated_at: "2026-07-15T19:00:00Z", application_status: "running",
  plan: { id: "p", title: "Sunday", date: "2026-07-19", service_type: "W", service_type_id: "s", service_times: [], duration_source: "x", songs: [a, b, c] },
  current_song: null, next_song: null, current_song_index: null, playing_song: null, playing_song_index: null, up_next_song: null,
  timer: null, plugins: {}, recent_events: [], recent_errors: [],
} as unknown as ApplicationState;
const withLive = (live: Song | null, over: Partial<ApplicationState> = {}): ApplicationState =>
  ({ ...base, current_song: live, playing_song: live, ...over }) as ApplicationState;

function ui(state: ApplicationState) {
  return (
    <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={null} state={state} />
  );
}
const select = () => screen.getByLabelText("Song") as HTMLSelectElement;

const oscSettings = (revision = 1) => ({
  settings: { lights: { enabled: true, method: "osc", osc_host: "127.0.0.1", osc_port: 21600, output_name: null, channel: 1, pulse_ms: 100, pads: [], cue_maps: {}, cue_revisions: { "song-1": revision } } },
}) as unknown as SettingsResponse;

function uiWith(state: ApplicationState, settings: SettingsResponse) {
  return (
    <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settings} state={state} />
  );
}

describe("Lighting draft rows (F-03)", () => {
  it("keeps an unsaved new row when the saved settings refresh and when the follow moves and comes back", () => {
    const { rerender } = render(uiWith(withLive(a), oscSettings()));
    const rows = () => screen.queryAllByLabelText("OSC address").length - 1; // minus the test-send field
    expect(rows()).toBe(0);
    fireEvent.click(screen.getByRole("button", { name: "Add cue" }));
    expect(rows()).toBe(1);
    // The settings refresh (a recording finished elsewhere): a new object, the draft stays.
    rerender(uiWith(withLive(a), oscSettings(2)));
    expect(rows()).toBe(1);
    // Auto-follow moves to the next song and the draft is not shown there, then is back.
    rerender(uiWith(withLive(b), oscSettings(2)));
    expect(select().value).toBe("song-2");
    expect(rows()).toBe(0);
    fireEvent.change(select(), { target: { value: "song-1" } });
    expect(rows()).toBe(1);
  });
});

describe("Lighting timeline follows the live song", () => {
  it("falls back to the first song when nothing has played, with no indicator", () => {
    render(ui(base));
    expect(select().value).toBe("song-1");
    expect(screen.queryByTestId("lighting-follow-status")).toBeNull();
  });

  it("switches when the live song changes and says it is following", () => {
    const { rerender } = render(ui(withLive(a)));
    expect(select().value).toBe("song-1");
    rerender(ui(withLive(b)));
    expect(select().value).toBe("song-2");
    expect(screen.getByTestId("lighting-follow-status")).toHaveTextContent("Following live song");
    expect(screen.queryByRole("button", { name: "Follow live" })).toBeNull();
  });

  it("keeps a manual pick while the same song plays, then follows the next song start", () => {
    const { rerender } = render(ui(withLive(a)));
    fireEvent.change(select(), { target: { value: "song-3" } });
    expect(select().value).toBe("song-3");
    expect(screen.getByTestId("lighting-follow-status")).toHaveTextContent("Showing Song 3");
    rerender(ui(withLive(a, { revision: 2 })));
    expect(select().value).toBe("song-3");
    rerender(ui(withLive(b)));
    expect(select().value).toBe("song-2");
  });

  it("a song ending does not drop the hand pick when current_song moves on to the next song (F-01)", () => {
    const { rerender } = render(ui(withLive(a)));
    fireEvent.change(select(), { target: { value: "song-3" } });
    // Song A ends: nothing is playing, and the plan's current song advances to B.
    rerender(ui({ ...withLive(null), current_song: b }));
    expect(select().value).toBe("song-3");
    expect(screen.getByTestId("lighting-follow-status")).toHaveTextContent("Showing Song 3");
  });

  it("Follow live goes back to the live song in one click", () => {
    render(ui(withLive(b)));
    fireEvent.change(select(), { target: { value: "song-1" } });
    fireEvent.click(screen.getByRole("button", { name: "Follow live" }));
    expect(select().value).toBe("song-2");
    expect(screen.getByTestId("lighting-follow-status")).toHaveTextContent("Following live song");
  });

  it("keeps the last played song after it stops", () => {
    const { rerender } = render(ui(withLive(b)));
    rerender(ui(withLive(null)));
    expect(select().value).toBe("song-2");
  });

  it("resets to the first song when the followed song leaves the plan", () => {
    const { rerender } = render(ui(withLive(b)));
    rerender(ui({ ...withLive(null), plan: { ...base.plan!, songs: [a, c] } }));
    expect(select().value).toBe("song-1");
  });
});
