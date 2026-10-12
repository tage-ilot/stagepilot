import { describe, expect, it } from "vitest";

// The project has no @types/node; declare only what this test uses.
declare const process: { cwd(): string };
// Tokens-only guard over every non-test .ts/.tsx/.css file under src/.
// Wider sibling of components/ui/tokensOnly.test.ts (which keeps its extra ui/ rules).
const forbidden: Array<[string, RegExp]> = [
  ["raw hex", /#[0-9a-f]{3,8}\b/i],
  ["rgb()/rgba() literal", /\brgba?\s*\(/i],
  ["hsl()/hsla() literal", /\bhsla?\s*\(/i],
  [
    "stock palette class",
    /\b(?:slate|gray|zinc|neutral|stone|red|rose|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink)-\d{2,3}\b/,
  ],
  ["white/black utility", /\b(?:text|bg)-(?:white|black)\b/],
  ["transition-all", /\btransition-all\b/],
];

// Allow-list: path suffixes exempt from the guard. Each entry needs a justification.
const allowList: string[] = [
  // Generated from frontend/scripts/design-tokens.mjs; it is where the literals live.
  "/design-tokens.css",
  // TODO remove after lockin/dead-styles (PR #139): index.css still holds the wordmark outline #000 and scrollbar hex on main.
  // Phase 6 PRs are independent and based on main, so this card cannot rebase on 6.1; delete this entry once #139 merges.
  "/index.css",
];

function violations(source: string) {
  return forbidden.filter(([, pattern]) => pattern.test(source)).map(([name]) => name);
}

// Read from disk (not import.meta.glob ?raw): vitest blanks CSS modules, which would hide index.css.
type Entry = { name: string; isDirectory(): boolean };
const { readdirSync, readFileSync } = (await import("node:fs" as string)) as {
  readdirSync(dir: string, opts: { withFileTypes: true }): Entry[];
  readFileSync(file: string, enc: "utf8"): string;
};
const { join, relative, sep } = (await import("node:path" as string)) as {
  join(...parts: string[]): string;
  relative(from: string, to: string): string;
  sep: string;
};
const srcRoot = join(process.cwd(), "src");
function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry: Entry) => {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "test" ? [] : walk(full);
    return /\.(ts|tsx|css)$/.test(entry.name) && !/\.(test|spec)\./.test(entry.name) ? [full] : [];
  });
}
const sources: Record<string, string> = Object.fromEntries(
  walk(srcRoot).map((file) => ["./" + relative(srcRoot, file).split(sep).join("/"), readFileSync(file, "utf8")]),
);

describe("tokens only across all of src", () => {
  it.each([
    ["#fff", "raw hex"],
    ["bg-[#123456]", "raw hex"],
    ["color: rgb(0 0 0)", "rgb()/rgba() literal"],
    ["rgba(1,2,3,.5)", "rgb()/rgba() literal"],
    ["hsl(10 20% 30%)", "hsl()/hsla() literal"],
    ["text-slate-400", "stock palette class"],
    ["bg-emerald-500", "stock palette class"],
    ["text-white", "white/black utility"],
    ["bg-black", "white/black utility"],
    ["transition-all", "transition-all"],
  ])("rejects fixture %s", (source, kind) => {
    expect(violations(source)).toContain(kind);
  });

  it("permits token classes and opacity modifiers", () => {
    expect(violations("bg-primary/10 text-ink rounded-ds-lg border-line transition-colors")).toEqual([]);
  });

  it("scans ts, tsx and css files, skipping tests and generated tokens", () => {
    const files = Object.keys(sources);
    expect(files.some((f) => f.endsWith("/index.css"))).toBe(true);
    expect(files.some((f) => f.endsWith("/App.tsx"))).toBe(true);
    expect(files.some((f) => f.endsWith(".ts"))).toBe(true);expect(sources["./index.css"]?.length).toBeGreaterThan(100);
    expect(files.some((f) => /\.(test|spec)\./.test(f))).toBe(false);
  });

  it("has no violations", () => {
    for (const [file, source] of Object.entries(sources)) {
      if (allowList.some((suffix) => file.endsWith(suffix))) continue;
      expect(violations(source), file).toEqual([]);
    }
  });
});
