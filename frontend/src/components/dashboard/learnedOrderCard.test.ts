import { describe, expect, it } from "vitest";

import { playbackSettings, playbackState, playbackStatus } from "../../test/playbackFixtures";
import { buildConnectionCardViews, buildReadinessChecks } from "./dashboardReadiness";

const on = { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, learn_order: true };
const build = (playback: ReturnType<typeof playbackStatus>) => {
  const views = buildConnectionCardViews({ state: playbackState, settings: playbackSettings, playback, midi: null, propresenter: null, lights: null });
  return { views, checks: buildReadinessChecks({ state: playbackState, settings: playbackSettings, live: true, propresenter: null, views }) };
};

describe("dashboard card wording for the learned order", () => {
  const learned = playbackStatus({ song_order: [], stale: true, plan_song_count: 6, order_mode: "learned", settings: on,
    learned_songs: [{ song_id: 1, plan_position: 0, confirmed: true, doubtful: false }, { song_id: 2, plan_position: 1, confirmed: false, doubtful: false }] });
  it("says learning, never scanned or saved, and does not block", () => {
    const { views, checks } = build(learned);
    expect(views.midi.detail).toContain("Learning the order as songs play (2 of 6 placed)");
    expect(views.midi.detail).not.toMatch(/scanned|saved/i);
    expect(checks.find((c) => c.id === "playback-order")).toBeUndefined();
  });
  it("a scanned order keeps the old card text", () => {
    const status = playbackStatus({ order_mode: "scanned" });
    expect(build(status).views.midi.detail).toBe(status.reason);
  });
});
