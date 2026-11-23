import type { LibrarySection, SongWaveform } from "../types";
import { formatElapsed } from "./lightingOsc";
import { SECTION_ABBREVIATIONS, sectionColor } from "./sectionColors";

/**
 * The drawing math of the lighting timeline's "song map": the arrangement bar and the waveform (both from the Playback library,
 * both view-only). Pure functions, so the rules can be tested without a browser.
 *
 * Both rows live inside the timeline's own horizontally scrolling content and are placed by the SAME seconds-to-percent mapping the
 * cue markers use (`at / length`), so zoom and scroll apply to them for free. The timeline's length is the countdown length: the
 * arrangement can run a few seconds past it, and what runs past is cut off here (the track is never made longer).
 */

/** The bar's height in px. */
export const BAR_HEIGHT_PX = 26;
/** Space a block keeps around its label, in px (the label must fit the block's width plus this). */
export const LABEL_PADDING_PX = 8;
/** The gap between two blocks, in px. */
export const BLOCK_GAP_PX = 1;

const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

export interface ArrangementBlock {
  /** The place in the arrangement (repeats are separate blocks with the same name, so this is the key). */
  index: number;
  name: string;
  /** Percent of the timeline, 0..100. */
  leftPct: number;
  /** Percent of the timeline, clipped so that left + width never passes 100. */
  widthPct: number;
  color: string;
  /** Playback's short form (V1, C, B1 ...); null for a name the table does not know. */
  abbreviation: string | null;
  /** The hover and long-press text: the full name and the section's own times, `m:ss–m:ss`. */
  title: string;
}

/**
 * One block per section, in playing order. A section that starts at or after the end of the timeline, or has no length, gets no
 * block; one that runs past the end is cut at the end. Seconds that are not usable numbers are skipped, never drawn.
 */
export function arrangementBlocks(sections: readonly LibrarySection[], length: number): ArrangementBlock[] {
  if (!Number.isFinite(length) || length <= 0) return [];
  const blocks: ArrangementBlock[] = [];
  sections.forEach((section, index) => {
    const { start, end } = section;
    if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) return;
    const from = Math.max(0, start);
    const to = Math.min(length, end);
    if (from >= length || to <= from) return;
    blocks.push({
      index,
      name: section.name,
      leftPct: (from / length) * 100,
      widthPct: ((to - from) / length) * 100,
      color: sectionColor(section.name),
      abbreviation: Object.hasOwn(SECTION_ABBREVIATIONS, section.name) ? SECTION_ABBREVIATIONS[section.name]! : null,
      title: `${section.name} · ${clock(start)}–${clock(end)}`,
    });
  });
  return blocks;
}

/**
 * The one rule for the words on a block (as in the approved mockup): Playback's short form when it fits the block, else nothing.
 * It fits when its measured width plus 8 px of padding is at most the block's width. `measure` is injected (a canvas
 * `measureText` in the browser) so the rule can be tested where there is no layout.
 */
export function blockLabel(abbreviation: string | null | undefined, widthPx: number, measure: (text: string) => number): string | null {
  if (!abbreviation || !Number.isFinite(widthPx)) return null;
  return measure(abbreviation) + LABEL_PADDING_PX <= widthPx ? abbreviation : null;
}

/** The path is drawn in a box this many units wide (and `HEIGHT` high); the SVG stretches it to the track. */
const WIDTH = 2400;
const HEIGHT = 100;
const MIDDLE = HEIGHT / 2;
/** Never more than this many columns, however long the song or the list: a column is the loudest of the points it covers. */
export const MAX_WAVEFORM_COLUMNS = 2400;
const num = (value: number) => String(Math.round(value * 10) / 10);

/**
 * The waveform as ONE mirrored SVG path around the center line, in a 2400 x 100 box that spans the timeline's length (the way
 * `loudnessPath` does it), so it follows the timeline's zoom and scroll. Heights are each point's `peak` against the song's own
 * loudest point (drawn as given, normalised; the helper has not said whether its scale is linear or dB, so nothing is labelled).
 * A waveform shorter than the timeline ends where its numbers end; a longer one is cut at the end of the timeline. Null when there
 * is nothing to draw.
 */
export function waveformPath(waveform: SongWaveform, length: number): string | null {
  const { step_s: step, peak } = waveform;
  if (!(length > 0) || !Number.isFinite(length) || !(step > 0) || !Number.isFinite(step) || !peak.length) return null;
  let max = 0;
  for (const value of peak) if (Number.isFinite(value) && value > max) max = value;
  if (max <= 0) return null;
  // Points that start inside the timeline.
  const usable = Math.min(peak.length, Math.ceil(length / step));
  if (usable <= 0) return null;
  const group = Math.ceil(usable / MAX_WAVEFORM_COLUMNS);
  const x = (slot: number) => Math.min(WIDTH, (slot * step * WIDTH) / length);
  const half: number[] = []; // each column's half height
  const mid: number[] = []; // each column's x center
  let last = 0;
  for (let from = 0; from < usable; from += group) {
    const to = Math.min(usable, from + group);
    let loudest = 0;
    for (let slot = from; slot < to; slot += 1) {
      const value = peak[slot]!;
      if (Number.isFinite(value) && value > loudest) loudest = value;
    }
    half.push((loudest / max) * MIDDLE);
    mid.push((x(from) + x(to)) / 2);
    last = x(to);
  }
  const first = x(0);
  const point = (at: number, offset: number, side: 1 | -1) => `${num(at)},${num(MIDDLE - side * offset)}`;
  const top = half.map((h, i) => `L${point(mid[i]!, h, 1)}`);
  const bottom = half.map((h, i) => `L${point(mid[i]!, h, -1)}`).reverse();
  const lastHalf = half[half.length - 1]!;
  const firstHalf = half[0]!;
  return [
    `M${point(first, firstHalf, 1)}`,
    ...top,
    `L${point(last, lastHalf, 1)}`,
    `L${point(last, lastHalf, -1)}`,
    ...bottom,
    `L${point(first, firstHalf, -1)}`,
    "Z",
  ].join(" ");
}

/** The text alternative of the waveform picture. */
export function waveformAltText(waveform: SongWaveform): string {
  let at = -1;
  let loudest = 0;
  waveform.peak.forEach((value, index) => { if (Number.isFinite(value) && value > loudest) { loudest = value; at = index; } });
  return at < 0 ? "Waveform: nothing to show" : `Waveform: loudest at ${formatElapsed(Math.round(at * waveform.step_s * 100) / 100)}`;
}

/** Where the "Song map" switch is remembered (this browser, like the timeline height). */
export const SONG_MAP_KEY = "stagepilot.lighting.songMap";

/**
 * A text-width function for `blockLabel`: the browser's canvas `measureText` with the font the label is drawn in. Where there is no
 * canvas (the unit tests' jsdom) it guesses 6.5 px a character, about right at the label's 11 px.
 */
export function canvasMeasure(font: string): (text: string) => number {
  const context = typeof CanvasRenderingContext2D !== "undefined" && typeof document !== "undefined" ? document.createElement("canvas").getContext("2d") : null;
  if (!context) return (text) => text.length * 6.5;
  context.font = font;
  return (text) => context.measureText(text).width;
}
