import { useCallback, useState } from "react";

export type HeightMode = "s" | "m" | "auto";

export const AUTO_MIN_PX = 200;
export const AUTO_MAX_PX = 420;
/** One stacked label row (the label pill and the gap under it). */
export const LABEL_ROW_PX = 22;
const AUTO_BASE_PX = 176;
/** The waveform and playhead always keep at least this much room under the labels. */
const PLOT_MIN_PX = 70;
const AUTO_MAX_ROWS = 12;
const PRESET_PX = { s: 200, m: 320 } as const;

export const HEIGHT_MODE_KEY = "stagepilot.lighting.timeline-height";

/** Pure: the Auto height for the number of label rows the markers stack into at the current zoom and clustering. */
export const autoTimelineHeight = (rows: number): number =>
  Math.min(AUTO_MAX_PX, Math.max(AUTO_MIN_PX, AUTO_BASE_PX + Math.max(0, rows) * LABEL_ROW_PX));

export const heightFor = (mode: HeightMode, rows: number): number => (mode === "auto" ? autoTimelineHeight(rows) : PRESET_PX[mode]);

/** How many label rows may be stacked: a fixed height keeps room for the waveform, Auto grows instead. */
export const labelRowsFor = (mode: HeightMode): number =>
  mode === "auto" ? AUTO_MAX_ROWS : Math.max(1, Math.floor((PRESET_PX[mode] - PLOT_MIN_PX) / LABEL_ROW_PX));

export const readHeightMode = (saved: string | null): HeightMode => (saved === "s" || saved === "m" || saved === "auto" ? saved : "auto");

/** The S / M / Auto choice, kept on this device. */
export function useHeightMode(): [HeightMode, (mode: HeightMode) => void] {
  const [mode, setMode] = useState<HeightMode>(() => {
    try { return readHeightMode(window.localStorage.getItem(HEIGHT_MODE_KEY)); } catch { return "auto"; }
  });
  const set = useCallback((next: HeightMode) => {
    setMode(next);
    try { window.localStorage.setItem(HEIGHT_MODE_KEY, next); } catch { /* the choice still applies for this session */ }
  }, []);
  return [mode, set];
}
