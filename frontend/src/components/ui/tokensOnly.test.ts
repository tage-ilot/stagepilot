import { describe, expect, it } from "vitest";

const forbidden = [
  /#[0-9a-f]{3,8}\b/i,
  /\brgba?\s*\(/i,
  /\b(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}\b/,
  /\b(?:bg|text|border|ring|outline|divide|fill|stroke|from|via|to|shadow|decoration|placeholder|caret|accent)-(?:white|black)\b/,
  /\btransition-all\b/,
  /\boutline-none\b/,
  /--sp-/,
  /\bstage[.-]\d/,
];
function violations(source: string) {
  return forbidden.filter((pattern) => pattern.test(source));
}
const sources = import.meta.glob<string>(["./**/*.tsx", "!./**/*.test.tsx", "!./**/*.spec.tsx"], {
  query: "?raw", import: "default", eager: true,
});

describe("UI tokens only (including gallery)", () => {
  it.each(["#fff", "bg-[#123456]", "text-slate-400", "rgb(", "transition-all", "outline-none", "var(--sp-text)", "bg-stage-950"])("rejects fixture %s", (source) => {
    expect(violations(source).length).toBeGreaterThan(0);
  });
  it("permits normative tokens and token opacity modifiers", () => {
    expect(violations("bg-primary/10 text-ink rounded-ds-lg can-hover:bg-primary-hover coarse:min-h-ds-touch-target")).toEqual([]);
  });
  it("scans every non-test TSX file recursively", () => {
    const files = Object.keys(sources);
    expect(files.some((path) => path.endsWith("gallery/Gallery.tsx"))).toBe(true);
    for (const [file, source] of Object.entries(sources)) expect(violations(source), file).toEqual([]);
  });
});
