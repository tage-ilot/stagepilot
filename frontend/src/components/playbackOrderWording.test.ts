import { describe, expect, it } from "vitest";

import { playbackStatus } from "../test/playbackFixtures";
import { playbackOrderModeText, playbackOrderNotice } from "./playbackStatus";

const on = { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, learn_order: true };
const learned = playbackStatus({ song_order: [], plan_song_count: 6, order_mode: "learned", settings: on,
  learned_songs: [{ song_id: 1, plan_position: 0, confirmed: true, doubtful: false }] });

describe("order wording is shared by panel, status card and tooltips", () => {
  it("mode text", () => {
    expect(playbackOrderModeText(learned)).toBe("Learning the order as songs play (1 of 6 placed)");
    expect(playbackOrderModeText(playbackStatus({ order_mode: "scanned" }))).toBe("Order scanned");
    expect(playbackOrderModeText(playbackStatus({ song_order: [], order_mode: "none", settings: on }))).toBe("No order yet - set Up next or scan");
    expect(playbackOrderModeText(playbackStatus({ song_order: [], order_mode: "none", settings: { ...on, learn_order: false } }))).toBe("No order yet - scan to set it up");
  });

  it("older backends without order_mode keep the scanned wording", () => {
    expect(playbackOrderModeText(playbackStatus())).toBe("Order scanned");
    expect(playbackOrderNotice(playbackStatus({ song_order: [] }))).toBe("Song order isn't set up yet.");
  });

  it("a learned order never raises the scan-required notice and never says scanned or saved", () => {
    expect(playbackOrderNotice(learned)).toBeNull();
    expect(playbackOrderModeText(learned)).not.toMatch(/scanned|saved/i);
  });

  it("learning with nothing placed yet does not block the dashboard; learning off still does", () => {
    expect(playbackOrderNotice(playbackStatus({ song_order: [], order_mode: "none", settings: on }))).toBeNull();
    expect(playbackOrderNotice(playbackStatus({ song_order: [], order_mode: "none", settings: { ...on, learn_order: false } }))).toBe("Song order isn't set up yet.");
  });

  it("a stale scanned order still asks for a new scan", () => {
    expect(playbackOrderNotice(playbackStatus({ order_mode: "scanned", stale: true }))).toMatch(/Scan the setlist again/);
  });
});
