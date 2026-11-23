import { useCallback, useState } from "react";

/** S, M and Auto, or a height in pixels set with the − and + buttons. */
export type HeightMode = "s" | "m" | "auto" | number;

/** What the − and + buttons change the height by, and how far they go. */
export const STEP_PX = 40;
export const MANUAL_MIN_PX = 160;
export const MANUAL_MAX_PX = 800;
export const clampManual = (px: number): number => Math.min(MANUAL_MAX_PX, Math.max(MANUAL_MIN_PX, Math.round(px)));

export const AUTO_MIN_PX = 200;
export const AUTO_MAX_PX = 420;
/** One stacked label row (the label pill and the gap under it). */
export const LABEL_ROW_PX = 22;
const AUTO_BASE_PX = 176;
/** The waveform and playhead always keep at least this much room under the labels. */
const PLOT_MIN_PX = 70;
const AUTO_MAX_ROWS = 12;
const PRESET_PX = { s: 200, m: 320 } as const;

/** `.v2`: the choice kept before the − and + buttons is not read, so every device starts on Auto. */
export const HEIGHT_MODE_KEY = "stagepilot.lighting.timeline-height.v2";

/** Pure: the Auto height for the number of label rows the markers stack into at the current zoom and clustering. */
export const autoTimelineHeight = (rows: number): number =>
  Math.min(AUTO_MAX_PX, Math.max(AUTO_MIN_PX, AUTO_BASE_PX + Math.max(0, rows) * LABEL_ROW_PX));

export const heightFor = (mode: HeightMode, rows: number): number =>
  typeof mode === "number" ? clampManual(mode) : mode === "auto" ? autoTimelineHeight(rows) : PRESET_PX[mode];

/** How many label rows may be stacked: a fixed height keeps room for the waveform, Auto grows instead. */
export const labelRowsFor = (mode: HeightMode): number =>
  mode === "auto" ? AUTO_MAX_ROWS : Math.max(1, Math.floor((heightFor(mode, 0) - PLOT_MIN_PX) / LABEL_ROW_PX));

export const readHeightMode = (saved: string | null): HeightMode => {
  if (saved === "s" || saved === "m" || saved === "auto") return saved;
  return saved !== null && /^\d{3}$/.test(saved) ? clampManual(Number(saved)) : "auto";
};

/** The S / M / Auto choice or the height set by hand, kept on this device. */
export function useHeightMode(): [HeightMode, (mode: HeightMode) => void] {
  const [mode, setMode] = useState<HeightMode>(() => {
    try { return readHeightMode(window.localStorage.getItem(HEIGHT_MODE_KEY)); } catch { return "auto"; }
  });
  const set = useCallback((next: HeightMode) => {
    setMode(next);
    try { window.localStorage.setItem(HEIGHT_MODE_KEY, String(next)); } catch { /* the choice still applies for this session */ }
  }, []);
  return [mode, set];
}
