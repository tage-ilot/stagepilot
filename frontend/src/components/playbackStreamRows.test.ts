import { describe, expect, it } from "vitest";

import type { PlaybackMonitorEntry } from "../types";
import { playbackStreamRows } from "./playbackStatus";

const base = { timestamp: 1, song_id: 202 as number | null, position: 0, previous_song_id: null, continues_playing: false, playing: true, pad: false, setlist_version: 1, previous_version: null, section_id: null, active: null, message_kind: null, reason: null, provisional: false };
const entry = (type: string, at: string | undefined, over: Record<string, unknown> = {}, discovery = false): PlaybackMonitorEntry => ({ event: { ...base, type, ...over }, discovery, at } as PlaybackMonitorEntry);
const status = { song_order: [101, 202], stale: false };

describe("playbackStreamRows (Playback's events in the Event stream)", () => {
  it("numbers a known song from the saved order, and never numbers an unknown one or one from an old scan", () => {
    const rows = playbackStreamRows([entry("song.started", "2026-01-01T10:00:00Z"), entry("song.paused", "2026-01-01T10:01:00Z", { song_id: 999 }), entry("song.stopped", "2026-01-01T10:02:00Z"), entry("song.ended", "2026-01-01T10:03:00Z", { song_id: 101 })], status);
    expect(rows.map((row) => row.source)).toEqual(["Song 2 started", "Paused", "Stopped", "Song 1 ended"]);
    expect(playbackStreamRows([entry("song.started", "2026-01-01T10:00:00Z")], { ...status, stale: true })[0]!.source).toBe("A song started");
    expect(playbackStreamRows([entry("song.started", "2026-01-01T10:00:00Z")], null)[0]!.source).toBe("A song started");
  });

  it("names a rollback in plain words, tags events seen while scanning, and uses the dotted type like the other rows", () => {
    const [rollback, scanning] = playbackStreamRows([entry("transport.reverted", "2026-01-01T10:00:00Z"), entry("song.started", "2026-01-01T10:01:00Z", {}, true)], status);
    expect(rollback).toMatchObject({ type: "playback.transport.reverted", source: "Playback did not confirm the command; transport reverted.", timestamp: "2026-01-01T10:00:00.000Z" });
    expect(scanning!.source).toBe("Song 2 started (while scanning Playback songs)");
    expect(scanning!.id).not.toBe(rollback!.id);
  });

  it("leaves out reconnect snapshots, unknown types and events with no time", () => {
    expect(playbackStreamRows([entry("state.snapshot", "2026-01-01T10:00:00Z"), entry("something.else", "2026-01-01T10:00:00Z"), entry("song.started", undefined)], status)).toEqual([]);
    expect(playbackStreamRows(undefined, undefined)).toEqual([]);
  });
});
