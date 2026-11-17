import { useEffect, useState } from "react";

import { getPlaybackLibraryWaveform } from "../api";
import type { SongWaveform } from "../types";

/** What the backend promises; anything else is treated as "no waveform" (the arrangement bar then shows alone). */
export function readWaveform(value: unknown): SongWaveform | null {
  if (!value || typeof value !== "object") return null;
  const { step_s: step, peak } = value as { step_s?: unknown; peak?: unknown };
  if (typeof step !== "number" || !Number.isFinite(step) || step <= 0 || !Array.isArray(peak) || !peak.length) return null;
  if (!peak.every((point) => typeof point === "number" && Number.isFinite(point) && point >= 0)) return null;
  return { step_s: step, peak: peak as number[] };
}

/**
 * The waveform of one Playback library song for the lighting timeline. Asks the backend (never the helper) only while `songId` is a
 * number, i.e. while that song's timeline is on screen, the song is paired and the song map is switched on. Shows nothing while it
 * loads and ignores a failure silently: the arrangement bar then shows alone. Read-only; nothing is stored here.
 */
export function useSongWaveform(songId: number | null, refreshKey = ""): SongWaveform | null {
  const [loaded, setLoaded] = useState<{ id: number; key: string; waveform: SongWaveform } | null>(null);
  useEffect(() => {
    if (songId === null) return;
    let cancelled = false;
    getPlaybackLibraryWaveform(songId).then(
      (answer) => {
        const waveform = readWaveform(answer);
        if (!cancelled && waveform) setLoaded({ id: songId, key: refreshKey, waveform });
      },
      () => { /* No waveform: the bar stands alone. */ },
    );
    return () => { cancelled = true; };
  }, [songId, refreshKey]);
  // Another song's numbers are never drawn under this song's bar.
  return songId !== null && loaded?.id === songId ? loaded.waveform : null;
}
