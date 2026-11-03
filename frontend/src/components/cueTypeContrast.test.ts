import { describe, expect, it } from "vitest";

// The project has no @types/node; declare only what this test uses.
declare const process: { cwd(): string };
const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
const { join } = (await import("node:path" as string)) as { join(...parts: string[]): string };
const css = readFileSync(join(process.cwd(), "src/design-tokens.css"), "utf8");
const token = (name: string) => /#[0-9a-f]{6}/i.exec(new RegExp(`--ds-${name}:\\s*(#[0-9a-f]{6})`, "i").exec(css)?.[0] ?? "")?.[0] ?? (() => { throw new Error(`missing token ${name}`); })();

const rgb = (hex: string): [number, number, number] => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255) as [number, number, number];
const lin = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const luminance = (c: [number, number, number]) => 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
const contrast = (a: [number, number, number], b: [number, number, number]) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
};

// Machado et al. 2009, severity 1.0, applied to linear RGB.
const SIM: Record<string, number[][]> = {
  protanopia: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deuteranopia: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]],
  tritanopia: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.303900]],
};
const simulate = (c: [number, number, number], kind: string): [number, number, number] => {
  const l = c.map(lin);
  return SIM[kind]!.map((row) => Math.min(1, Math.max(0, row[0]! * l[0]! + row[1]! * l[1]! + row[2]! * l[2]!))) as [number, number, number];
};
const toLab = (linear: [number, number, number]): [number, number, number] => {
  const [r, g, b] = linear;
  const x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
  const y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
  const fn = (t: number) => (t > 0.008856 ? Math.cbrt(t) : 7.787 * t + 16 / 116);
  return [116 * fn(y) - 16, 500 * (fn(x) - fn(y)), 200 * (fn(y) - fn(z))];
};
const deltaE = (a: [number, number, number], b: [number, number, number], kind: string | null) => {
  const la = toLab(kind ? simulate(a, kind) : (a.map(lin) as [number, number, number]));
  const lb = toLab(kind ? simulate(b, kind) : (b.map(lin) as [number, number, number]));
  return Math.hypot(la[0] - lb[0], la[1] - lb[1], la[2] - lb[2]);
};

// CIE76 distance between any two colours after simulation. Around 2.3 is one just-noticeable difference; 7 is clearly apart.
const MIN_DELTA_E = 7;
const MIN_DELTA_E_NOTES = 7;
const OSC = ["cue-look-captured", "cue-look-edited", "cue-look-manual", "cue-live", "cue-fixture", "cue-other"];
const NOTES = ["cue-note-c", "cue-note-d", "cue-note-e", "cue-note-f", "cue-note-g", "cue-note-a", "cue-note-b"];
const TRACKS = ["surface-950", "surface-900", "surface-panel", "surface-panel-strong"];

describe("cue type colours", () => {
  it.each([...OSC, ...NOTES])("%s is at least 3:1 against every track background", (name) => {
    for (const track of TRACKS) expect(contrast(rgb(token(name)), rgb(token(track)))).toBeGreaterThanOrEqual(3);
  });

  // Colour-blind simulation (protan, deutan, tritan) of the token values. Glyph and bar style are the fallback, so the
  // bar is only required to keep a minimum difference between every pair that sits side by side in a legend group.
  const pairs = (names: string[]) => names.flatMap((a, i) => names.slice(i + 1).map((b) => [a, b] as const));
  it.each(["protanopia", "deuteranopia", "tritanopia", null])("OSC type colours stay apart (%s)", (kind) => {
    for (const [a, b] of pairs(OSC)) expect(deltaE(rgb(token(a)), rgb(token(b)), kind), `${a} vs ${b}`).toBeGreaterThan(MIN_DELTA_E);
  });
  it.each(["protanopia", "deuteranopia", "tritanopia", null])("note letter colours stay apart (%s)", (kind) => {
    for (const [a, b] of pairs(NOTES)) expect(deltaE(rgb(token(a)), rgb(token(b)), kind), `${a} vs ${b}`).toBeGreaterThan(MIN_DELTA_E_NOTES);
  });
});
