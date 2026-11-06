import type { LightingEffect, OscLightingCue } from "../types";
import { fixtureOf, rowMessages } from "./lightingLooks";

/**
 * Pure helpers for the smooth-capture data: ramps (a row with `fade_seconds`) drawn as bars, effect loops drawn as bands, the
 * checks a ramp edit goes through, and the rules for which rows may be merged. No React, no clock.
 */

/** A bar is never drawn narrower than this, so a 0.05 s ramp on a long song can still be seen and grabbed. */
export const MIN_BAR_PX = 4;
/** The longest ramp the capture makes; the editor holds hand edits to the same limit. */
export const MAX_RAMP_SECONDS = 60;
/** At most this many lanes of loops are drawn under the timeline; more loops share the last lane. */
export const MAX_EFFECT_LANES = 3;
/** Rows that start within this (the capture's own grouping slot) belong together. */
const SAME_START_SECONDS = 0.02;
const SAME_FADE_SECONDS = 0.001;

export const fadeOf = (cue: Pick<OscLightingCue, "fade_seconds">): number => {
  const fade = cue.fade_seconds ?? 0;
  return Number.isFinite(fade) && fade > 0 ? fade : 0;
};
export const isRamp = (cue: Pick<OscLightingCue, "fade_seconds">): boolean => fadeOf(cue) > 0;

/** `+3.2 s`, the duration column of a ramp. */
export const formatFade = (seconds: number): string => `+${seconds.toFixed(1)} s`;

export interface BarGeometry { left: number; width: number }

/** Where a span of the song sits on a track `trackWidth` px wide: its left edge and width, never narrower than MIN_BAR_PX and never past the end. */
export const spanGeometry = (start: number, end: number, songLength: number, trackWidth: number): BarGeometry => {
  if (songLength <= 0 || trackWidth <= 0) return { left: 0, width: MIN_BAR_PX };
  const left = Math.min(Math.max(0, (start / songLength) * trackWidth), Math.max(0, trackWidth - MIN_BAR_PX));
  const natural = (Math.max(end, start) - start) / songLength * trackWidth;
  const width = Math.max(MIN_BAR_PX, Math.min(natural, trackWidth - left));
  return { left, width };
};

/** The bar of a ramp: from its start to start + fade. */
export const rampGeometry = (at: number, fade: number, songLength: number, trackWidth: number): BarGeometry =>
  spanGeometry(at, at + fade, songLength, trackWidth);

/** The band of an effect loop: from `start_seconds` to `end_seconds`. */
export const effectGeometry = (effect: Pick<LightingEffect, "start_seconds" | "end_seconds">, songLength: number, trackWidth: number): BarGeometry =>
  spanGeometry(effect.start_seconds, effect.end_seconds, songLength, trackWidth);

/**
 * Lane for each effect, first come first served: an effect goes on the first lane whose last effect has ended. When all
 * `maxLanes` are busy the effect shares the lane that frees up soonest (the band is drawn over it), so loops never add more
 * than `maxLanes` lines under the timeline.
 */
export const assignEffectLanes = (effects: Pick<LightingEffect, "id" | "start_seconds" | "end_seconds">[], maxLanes = MAX_EFFECT_LANES): Map<string, number> => {
  const lanes = new Map<string, number>();
  const ends: number[] = [];
  const sorted = [...effects].sort((a, b) => a.start_seconds - b.start_seconds || a.end_seconds - b.end_seconds || a.id.localeCompare(b.id));
  for (const effect of sorted) {
    let lane = ends.findIndex((end) => end <= effect.start_seconds);
    if (lane === -1) {
      if (ends.length < maxLanes) { lane = ends.length; ends.push(effect.end_seconds); }
      else { lane = ends.indexOf(Math.min(...ends)); ends[lane] = Math.max(ends[lane]!, effect.end_seconds); }
    } else ends[lane] = effect.end_seconds;
    lanes.set(effect.id, lane);
  }
  return lanes;
};

export const effectFixtureCount = (effect: Pick<LightingEffect, "members">): number => effect.members.length;

/** `4.0 s loop x 16 fixtures`. */
export const effectLabel = (effect: Pick<LightingEffect, "period_seconds" | "members">): string => {
  const count = effectFixtureCount(effect);
  return `${effect.period_seconds.toFixed(1)} s loop x ${count} fixture${count === 1 ? "" : "s"}`;
};

/** The distinct properties an effect drives, in the order its cycle names them. */
export const effectProperties = (effect: Pick<LightingEffect, "cycle">): string[] => [...new Set(effect.cycle.map((step) => step.property))];

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));

/**
 * One property of an effect across one cycle, as `count` samples. The value ramps from the previous step of the same property
 * (the first step starts from the last one, so the loop closes) to its own over its `fade_seconds`. Vector values use their first
 * number. Returns an empty list for a property the cycle does not name.
 */
export const effectCycleSamples = (effect: Pick<LightingEffect, "cycle" | "period_seconds">, property: string, count = 48): number[] => {
  const steps = effect.cycle.filter((step) => step.property === property).sort((a, b) => a.at_seconds - b.at_seconds);
  const period = effect.period_seconds;
  if (!steps.length || !(period > 0) || count < 2) return [];
  const samples: number[] = [];
  for (let index = 0; index < count; index++) {
    const t = (index / (count - 1)) * period;
    let current = -1;
    steps.forEach((step, position) => { if (step.at_seconds <= t + 1e-9) current = position; });
    const step = current === -1 ? steps[steps.length - 1]! : steps[current]!;
    const previous = current <= 0 ? steps[steps.length - 1]! : steps[current - 1]!;
    const from = previous.value[0] ?? 0;
    const to = step.value[0] ?? 0;
    const fade = step.fade_seconds ?? 0;
    const since = current === -1 ? Number.POSITIVE_INFINITY : t - step.at_seconds;
    samples.push(fade > 0 && since < fade ? from + (to - from) * clamp01(since / fade) : to);
  }
  return samples;
};

/** SVG path points (`x,y x,y ...`) of the samples scaled to a `width` x `height` box, flat lines in the middle. */
export const sparklinePoints = (samples: number[], width: number, height: number): string => {
  if (samples.length < 2) return "";
  const low = Math.min(...samples);
  const high = Math.max(...samples);
  const span = high - low;
  return samples
    .map((value, index) => `${((index / (samples.length - 1)) * width).toFixed(1)},${(span < 1e-9 ? height / 2 : height - ((value - low) / span) * height).toFixed(1)}`)
    .join(" ");
};

const trimNumber = (value: number) => String(Math.round(value * 100) / 100);

/** `0.5` or `0.2 0.8 1` for a message's numbers; options and text are shown as they are. */
export const formatArgs = (args: { type: string; value: number | string | boolean }[]): string =>
  args.map((arg) => (typeof arg.value === "number" ? trimNumber(arg.value) : String(arg.value))).join(" ");

export interface RampLine { fixture: string; property: string; from: string | null; to: string; ramps: boolean }

/** The value an address had just before `cue`: the end value of the latest earlier row that sets it, or null when none does. */
const valueBefore = (cues: OscLightingCue[], cue: OscLightingCue, address: string): string | null => {
  let found: { at: number; text: string } | null = null;
  for (const other of cues) {
    if (other.id === cue.id) continue;
    // Anything that starts at or after this row does not count; rows at the same time apply instant first, then ramps.
    if (other.at_seconds > cue.at_seconds || (other.at_seconds === cue.at_seconds && (isRamp(other) || !isRamp(cue)))) continue;
    for (const message of rowMessages(other)) {
      if (message.address === address && (!found || other.at_seconds >= found.at)) found = { at: other.at_seconds, text: formatArgs(message.args) };
    }
  }
  return found?.text ?? null;
};

/** What a row does, one line per message: the fixture, the property, from -> to. Numeric messages of a ramp are the ones that move. */
export const rowSummary = (cue: OscLightingCue, cues: OscLightingCue[]): RampLine[] =>
  rowMessages(cue).map((message) => {
    const property = /\/overrides\/(.+)$/.exec(message.address)?.[1] ?? message.address;
    const numeric = message.args.length > 0 && message.args.every((arg) => typeof arg.value === "number");
    return {
      fixture: fixtureOf(message.address),
      property,
      from: isRamp(cue) && numeric ? valueBefore(cues, cue, message.address) : null,
      to: formatArgs(message.args),
      ramps: isRamp(cue) && numeric,
    };
  });

/** Plain-words summary of a ramp for a tooltip and the detail strip. */
export const rampDescription = (cue: OscLightingCue, cues: OscLightingCue[]): string => {
  const lines = rowSummary(cue, cues);
  const fixtures = [...new Set(lines.map((line) => line.fixture))];
  const shownFixtures = fixtures.length > 3 ? `${fixtures.slice(0, 3).join(", ")} and ${fixtures.length - 3} more` : fixtures.join(", ");
  const moves = lines.slice(0, 4).map((line) => `${line.property} ${line.from !== null ? `${line.from} → ` : "→ "}${line.to}`).join("; ");
  return `${shownFixtures}: ${moves}${lines.length > 4 ? `; ${lines.length - 4} more` : ""} over ${fadeOf(cue).toFixed(1)} s`;
};

/** Parses what is typed in the duration box: "3.2", "3,2" or "3.2 s". Null when it is not a number. */
export const parseFadeInput = (text: string): number | null => {
  const cleaned = text.trim().replace(/\s*s(ec(onds)?)?$/i, "").replace(",", ".");
  if (cleaned === "" || !/^\d*\.?\d+$|^\d+\.?$/.test(cleaned)) return null;
  const value = Number(cleaned);
  return Number.isFinite(value) ? Math.round(value * 1000) / 1000 : null;
};

/**
 * What is wrong with a ramp's duration, or null. The rules are the capture's: 0 (a step) up to 60 s, and the ramp has to end inside the
 * song (start + duration <= the song length, when the length is known).
 */
export const fadeError = (fade: number | null, at: number, songLength: number | null): string | null => {
  if (fade === null) return "Use a number of seconds, like 3.2.";
  if (fade < 0) return "Duration cannot be negative.";
  if (fade > MAX_RAMP_SECONDS) return `Longest ramp is ${MAX_RAMP_SECONDS} s.`;
  if (songLength !== null && at + fade > songLength + 1e-9) return "This ramp would run past the end of the song.";
  return null;
};

/** The latest start a ramp can have and still end inside the song. */
export const clampRampStart = (at: number, fade: number, songLength: number | null): number =>
  songLength === null ? Math.max(0, at) : Math.min(Math.max(0, at), Math.max(0, songLength - fade));

const sameStart = (a: OscLightingCue, b: OscLightingCue) => Math.abs(a.at_seconds - b.at_seconds) <= SAME_START_SECONDS;
const sameFade = (a: OscLightingCue, b: OscLightingCue) => Math.abs(fadeOf(a) - fadeOf(b)) <= SAME_FADE_SECONDS;

/**
 * Why the chosen rows cannot be merged, or null. Merging puts every message into one row at the earliest time, so it keeps its meaning
 * only for rows that mean the same kind of thing: instant rows merge with instant rows; ramps merge only with ramps of the same
 * duration that start together (the capture's own grouping) and move different properties. A ramp never merges with an instant row.
 * Loops are not rows: they cannot be picked, and a merge never changes them.
 */
export const mergeBlockReason = (cues: OscLightingCue[], ids: string[]): string | null => {
  const chosen = cues.filter((cue) => ids.includes(cue.id));
  if (chosen.length < 2) return null;
  const ramps = chosen.filter(isRamp);
  if (ramps.length === 0) return null;
  if (ramps.length !== chosen.length) return "A ramp cannot be merged with an instant row: the instant row would start fading. Pick rows of one kind.";
  const first = chosen[0]!;
  if (!chosen.every((cue) => sameFade(cue, first))) return "These ramps have different durations. Only ramps of the same duration can be merged.";
  if (!chosen.every((cue) => sameStart(cue, first))) return "These ramps start at different times. Only ramps that start together can be merged.";
  const seen = new Set<string>();
  for (const cue of chosen) {
    for (const message of rowMessages(cue)) {
      if (seen.has(message.address)) return "Two of these ramps move the same property. Merging would drop one of them.";
      seen.add(message.address);
    }
  }
  return null;
};
