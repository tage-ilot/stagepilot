import { describe, expect, it } from "vitest";

// The project has no @types/node; declare only what this test uses.
declare const process: { cwd(): string };
type Entry = { name: string; isDirectory(): boolean };
const { readdirSync, readFileSync, existsSync } = (await import("node:fs" as string)) as {
  readdirSync(dir: string, opts: { withFileTypes: true }): Entry[];
  readFileSync(file: string, enc: "utf8"): string;
  existsSync(file: string): boolean;
};

/**
 * D2 (design audit): an operator never sees the word "backend" (DESIGN.md section 10): the app is "StagePilot", the part that runs on the
 * computer is "the engine". Code names, routes, test ids, tone keys, log file names and comments keep it. This reads every non-test source
 * file of the frontend and of the desktop shell and fails on a string literal or JSX text that contains the word.
 */
const ALLOWED: Array<{ file: string; text: string; why: string }> = [
  { file: "components/Dashboard.tsx", text: "Backend", why: "Hunter's decision (2026-11-18): the collapsed connection card keeps the title Backend; the open panel is titled StagePilot." },
];

const word = /(?<![\w./-])[Bb]ackend(?![\w/-])/;
const literal = /(["'`])((?:\\.|(?!\1)[^\\\n])*)\1/g;
const jsxText = />([^<>{}\n]*[Bb]ackend[^<>{}\n]*)</g;

function walk(dir: string, into: string[], extensions: string[]) {
  if (!existsSync(dir)) return;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = `${dir}/${entry.name}`;
    if (entry.isDirectory()) { if (entry.name !== "node_modules" && entry.name !== "target" && entry.name !== "gallery") walk(path, into, extensions); continue; }
    if (extensions.some((ext) => entry.name.endsWith(ext)) && !/\.test\.|\.d\.ts$/.test(entry.name)) into.push(path);
  }
}

/** Visible-text candidates of one file: [text, line]. Comment lines, panic messages and logs are not operator words. */
function candidates(source: string, rust: boolean): Array<[string, number]> {
  const found: Array<[string, number]> = [];
  // Log macros that span lines are for developers.
  if (rust) source = source.replace(/e?println!\([^;]*\);/g, (all) => all.replace(/[^\n]/g, " "));
  source.split("\n").forEach((line, index) => {
    const trimmed = line.trim();
    if (/^(\/\/|\*|\/\*|\{\/\*)/.test(trimmed)) return;
    // Panic messages and log lines are for developers.
    if (/console\.\w+\(|expect\(|eprintln!|println!|log::|tracing::/.test(line)) return;
    const code = line.replace(/\s\/\/ .*$/, "");
    for (const match of code.matchAll(literal)) {
      const text = match[2] ?? "";
      if (text === "backend") continue; // a tone / key / id, never shown
      if (word.test(text)) found.push([text, index + 1]);
    }
    if (!rust) for (const match of code.matchAll(jsxText)) if (word.test(match[1] ?? "")) found.push([(match[1] ?? "").trim(), index + 1]);
  });
  return found;
}

describe("operators never see the word backend", () => {
  const root = process.cwd();
  const files: Array<{ path: string; rust: boolean }> = [];
  const frontend: string[] = [];
  walk(`${root}/src`, frontend, [".ts", ".tsx"]);
  frontend.forEach((path) => files.push({ path, rust: false }));
  const desktop: string[] = [];
  walk(`${root}/../desktop/src-tauri/src`, desktop, [".rs"]);
  desktop.forEach((path) => files.push({ path, rust: true }));

  it("reads the frontend and the desktop shell", () => {
    expect(frontend.length).toBeGreaterThan(100);
    expect(desktop.length).toBeGreaterThan(0);
  });

  it("finds no visible text with backend in it, except the allowed card title", () => {
    const hits: string[] = [];
    for (const { path, rust } of files) {
      if (rust && path.endsWith("log_prune.rs")) continue; // test fixtures with log file names
      const relative = path.replace(`${root}/src/`, "").replace(`${root}/../desktop/src-tauri/src/`, "desktop/");
      for (const [text, line] of candidates(readFileSync(path, "utf8"), rust)) {
        if (ALLOWED.some((entry) => entry.file === relative && entry.text === text)) continue;
        hits.push(`${relative}:${line}: ${text}`);
      }
    }
    expect(hits).toEqual([]);
  });

  it("keeps the allow list to the one card title", () => {
    expect(ALLOWED).toHaveLength(1);
  });

  it("the detector itself sees backend in a message and ignores identifiers", () => {
    expect(candidates(`const a = "The local backend stopped";`, false)).toHaveLength(1);
    expect(candidates("<p>Backend log</p>", false)).toHaveLength(1);
    expect(candidates(`tone="backend" id="backend-configuration" const f = "stagepilot-backend.log"; const g = "/api/backend/x"`, false)).toHaveLength(0);
    expect(candidates(`    // The Backend is a comment`, false)).toHaveLength(0);
  });
});
