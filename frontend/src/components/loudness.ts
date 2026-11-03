import type { SongLoudness } from "../types";
import { formatElapsed } from "./lightingOsc";

/** The stored byte is 0 for "no data" and 1..255 for -60..0 dBFS (a digital level, not sound pressure). */
export const loudnessDb = (byte: number): number | null => (byte === 0 ? null : -60 + ((byte - 1) / 254) * 60);

/** The loudest slot, its start in seconds; the first one when several tie. */
export function loudnessPeak(loudness: SongLoudness): { at: number; byte: number } | null {
  let best = -1;
  loudness.values.forEach((value, index) => {
    if (value !== 0 && (best < 0 || value > loudness.values[best]!)) best = index;
  });
  return best < 0 ? null : { at: Math.round(best * loudness.step_seconds * 100) / 100, byte: loudness.values[best]! };
}

/** The text alternative of the picture. */
export function loudnessAltText(loudness: SongLoudness): string {
  const peak = loudnessPeak(loudness);
  return peak ? `Loudness: peak at ${formatElapsed(peak.at)}` : "Loudness: nothing recorded";
}

const WIDTH = 1000;
const HEIGHT = 100;
const num = (value: number) => String(Math.round(value * 100) / 100);

/**
 * The envelope as one SVG path in a 1000 x 100 box that spans the song's length, so it follows the
 * timeline's zoom and scroll for free. Height is each slot against the song's own loudest slot.
 * Runs of slots are filled shapes; a slot with no data leaves a gap.
 */
export function loudnessPath(loudness: SongLoudness, songLength: number): string | null {
  const max = Math.max(0, ...loudness.values);
  if (max === 0 || songLength <= 0 || loudness.step_seconds <= 0) return null;
  const x = (slot: number) => Math.min(WIDTH, (slot * loudness.step_seconds * WIDTH) / songLength);
  const parts: string[] = [];
  let run: string[] = [];
  let runEnd = 0;
  const close = () => {
    if (run.length) parts.push(`${run.join(" ")} L${num(runEnd)},${HEIGHT} Z`);
    run = [];
  };
  for (let slot = 0; slot < loudness.values.length; slot += 1) {
    const value = loudness.values[slot]!;
    const left = x(slot);
    if (value === 0 || left >= WIDTH) {
      close();
      if (left >= WIDTH) break;
      continue;
    }
    const top = HEIGHT - (value / max) * HEIGHT;
    const right = x(slot + 1);
    if (!run.length) run.push(`M${num(left)},${HEIGHT}`);
    run.push(`L${num(left)},${num(top)} L${num(right)},${num(top)}`);
    runEnd = right;
  }
  close();
  return parts.length ? parts.join(" ") : null;
}

/** Meter fill, 0 to 100 %, from the stored 0..255 level. */
export const meterPercent = (value: number | undefined): number => Math.round((Math.min(255, Math.max(0, value ?? 0)) / 255) * 100);
