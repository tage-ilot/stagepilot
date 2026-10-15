import type { LightingMethod, LightingPad, PadColor } from "../types";

/** Cue times are kept to 0.1 s everywhere (capture, drag, nudge, the cue rows). */
export const snapTenths = (seconds: number): number => Math.round(seconds * 10) / 10;

/** Snaps to 0.1 s and clamps to the song. A song with no known length only clamps at 0. */
export const clampCueTime = (seconds: number, songLength: number | null): number => {
  const snapped = snapTenths(Number.isFinite(seconds) ? seconds : 0);
  const upper = songLength === null ? Number.POSITIVE_INFINITY : snapTenths(songLength);
  return Math.min(Math.max(0, snapped), upper);
};

/** Arrow-key nudge: 0.1 s, or 1 s with Shift. Result is snapped and clamped. */
export const nudgeCueTime = (at: number, direction: -1 | 1, shift: boolean, songLength: number | null): number =>
  clampCueTime(at + direction * (shift ? 1 : 0.1), songLength);

/** Time of a pointer on the track: `offsetX` is measured from the track's left edge. */
export const timeFromTrackOffset = (offsetX: number, trackWidth: number, songLength: number): number =>
  trackWidth <= 0 ? 0 : clampCueTime((offsetX / trackWidth) * songLength, songLength);

/** Where a running song is, in seconds: the position it (re)started from plus time since. */
export const elapsedSince = (startedAt: string | null, startPosition: number | null | undefined, now: number): number | null => {
  if (!startedAt) return null;
  const started = Date.parse(startedAt);
  if (Number.isNaN(started)) return null;
  return Math.max(0, (startPosition ?? 0) + (now - started) / 1000);
};

export interface TimelineMarker {
  id: string;
  at: number;
  label: string;
  color: PadColor | null;
}

/** Ruler tick spacing in seconds for a song of this length. */
export const rulerStep = (songLength: number): number => {
  if (songLength <= 60) return 10;
  if (songLength <= 180) return 15;
  if (songLength <= 480) return 30;
  return 60;
};

/** A pad can fire only through the saved method, and only while that method is the one on screen. */
export const padIsActive = (pad: LightingPad, viewMethod: LightingMethod, savedMethod: LightingMethod): boolean =>
  pad.method === viewMethod && pad.method === savedMethod;

/** Pads for the method on screen are kept; the others are dimmed, not hidden or deleted. */
export const padIsDimmed = (pad: LightingPad, viewMethod: LightingMethod): boolean => pad.method !== viewMethod;

/** Class strings are written out in full so Tailwind can see them. */
export const padColorClasses: Record<PadColor, { pad: string; marker: string; swatch: string; label: string }> = {
  rose: { label: "Rose", pad: "border-integration-lighting-soft/60 bg-integration-lighting-soft/15", marker: "bg-integration-lighting-soft", swatch: "bg-integration-lighting-soft" },
  teal: { label: "Teal", pad: "border-integration-lighting-osc-soft/60 bg-integration-lighting-osc-soft/15", marker: "bg-integration-lighting-osc-soft", swatch: "bg-integration-lighting-osc-soft" },
  amber: { label: "Orange", pad: "border-integration-presentation-soft/60 bg-integration-presentation-soft/15", marker: "bg-integration-presentation-soft", swatch: "bg-integration-presentation-soft" },
  blue: { label: "Blue", pad: "border-integration-playback-soft/60 bg-integration-playback-soft/15", marker: "bg-integration-playback-soft", swatch: "bg-integration-playback-soft" },
  violet: { label: "Violet", pad: "border-integration-playback-midi-soft/60 bg-integration-playback-midi-soft/15", marker: "bg-integration-playback-midi-soft", swatch: "bg-integration-playback-midi-soft" },
  green: { label: "Green", pad: "border-integration-services-soft/60 bg-integration-services-soft/15", marker: "bg-integration-services-soft", swatch: "bg-integration-services-soft" },
};
export const padColors = Object.keys(padColorClasses) as PadColor[];
export const neutralPad = { pad: "border-edge-medium bg-surface-950", marker: "bg-ink-muted" };

/** Reorders by one place. Out-of-range moves return the list unchanged. */
export const movePad = <T,>(items: T[], index: number, delta: -1 | 1): T[] => {
  const target = index + delta;
  if (index < 0 || index >= items.length || target < 0 || target >= items.length) return items;
  const next = [...items];
  [next[index], next[target]] = [next[target]!, next[index]!];
  return next;
};

/** Keyboard shortcut 1-9 maps to the Nth pad of the method on screen (in board order). */
export const padForShortcut = (pads: LightingPad[], key: string): LightingPad | null => {
  if (!/^[1-9]$/.test(key)) return null;
  return pads[Number(key) - 1] ?? null;
};

/** Describes a pad's payload in one short line, for the pad tooltip and the editor list. */
export const describePad = (pad: LightingPad) =>
  pad.method === "osc" ? (pad.address ?? "") : `MIDI note ${pad.note ?? 0}, velocity ${pad.velocity}`;
