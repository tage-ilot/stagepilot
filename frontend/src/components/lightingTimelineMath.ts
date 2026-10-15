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

/** Longest a marker label is drawn, in px, and the least it needs to be readable. */
export const MARKER_LABEL_MAX_PX = 112;
const MARKER_LABEL_CHAR_PX = 7;
const MARKER_LABEL_PAD_PX = 14;
/** Clear space kept between the end of a label and the next marker line. */
const MARKER_LABEL_GAP_PX = 10;
/** Rows a label can sit on. More cues than this that all overlap share the last row and are shortened. */
export const MARKER_LABEL_ROWS = 3;

/** How wide a label is drawn: its text (about 7 px a character), capped. */
export const markerLabelWidth = (label: string): number =>
  Math.min(MARKER_LABEL_MAX_PX, Math.max(0, label.length) * MARKER_LABEL_CHAR_PX + MARKER_LABEL_PAD_PX);

export interface MarkerLabelPlacement {
  /** 0 is the top row; each row below sits one label height lower. */
  row: number;
  /** Width the label may use before it would run into the next label on its row, in px. */
  width: number;
}

/**
 * Stacks marker labels so close cues stay readable (the owner's capture flow drops cues a few seconds apart on a four-minute
 * song). Markers are taken left to right; each label goes on the first row where the label before it has already ended, and
 * when every row is busy it shares the last row and is shortened to the gap. Returns a placement per marker id.
 */
export const placeMarkerLabels = (
  markers: { id: string; label: string; at: number }[],
  trackWidth: number,
  songLength: number,
): Map<string, MarkerLabelPlacement> => {
  const placements = new Map<string, MarkerLabelPlacement>();
  if (trackWidth <= 0 || songLength <= 0) {
    for (const marker of markers) placements.set(marker.id, { row: 0, width: markerLabelWidth(marker.label) });
    return placements;
  }
  // Labels start 8 px to the right of the marker line (see the component), so the line's x is the start of the claim.
  const sorted = [...markers].sort((left, right) => left.at - right.at || left.id.localeCompare(right.id));
  const rowEnds: number[] = Array.from({ length: MARKER_LABEL_ROWS }, () => Number.NEGATIVE_INFINITY);
  const lastOnRow: (string | null)[] = Array.from({ length: MARKER_LABEL_ROWS }, () => null);
  for (const marker of sorted) {
    const x = (marker.at / songLength) * trackWidth;
    const wanted = markerLabelWidth(marker.label);
    let row = rowEnds.findIndex((end) => end <= x);
    if (row === -1) {
      // Every row is busy: share the one whose label ends soonest, and cut that earlier label short at this marker.
      row = rowEnds.indexOf(Math.min(...rowEnds));
      const earlier = lastOnRow[row];
      const earlierPlacement = earlier ? placements.get(earlier) : undefined;
      if (earlier && earlierPlacement) {
        const earlierX = (sorted.find((item) => item.id === earlier)!.at / songLength) * trackWidth;
        placements.set(earlier, { row: earlierPlacement.row, width: Math.max(0, Math.min(earlierPlacement.width, x - earlierX - 4)) });
      }
    }
    placements.set(marker.id, { row, width: wanted });
    rowEnds[row] = x + wanted;
    lastOnRow[row] = marker.id;
  }
  // A marker's line runs from its own label row down, so it crosses the labels on that row and below it. Stop every label short of
  // the next later marker whose line would cross it (a marker on a row further down starts below the label and crosses nothing).
  for (const marker of sorted) {
    const mine = placements.get(marker.id)!;
    const x = (marker.at / songLength) * trackWidth;
    for (const other of sorted) {
      if (other.at <= marker.at) continue;
      const theirs = placements.get(other.id)!;
      if (theirs.row > mine.row) continue;
      const gap = (other.at / songLength) * trackWidth - x - MARKER_LABEL_GAP_PX;
      if (gap < mine.width) mine.width = Math.max(0, gap);
      break;
    }
  }
  return placements;
};
