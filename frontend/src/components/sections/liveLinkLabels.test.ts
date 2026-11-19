import { describe, expect, it } from "vitest";

/**
 * Every Live Link block ends its connection row with the same two buttons, Scan and Connect. The four old words are gone from the three
 * blocks: no component draws them, and no test asserts them as something that appears (a test may only say they do NOT appear).
 */
const sources = import.meta.glob("/src/components/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const OLD = /Test connection|Check again|Scan Network|Scan again/;
// Not one of the three Live Link blocks, or not a label: the generic Button demo in the UI gallery; the Services page (its own Test
// connection action); the one place that rewrites the backend's old sentence "Choose Scan Network to find Playback"; this file.
const EXEMPT = [/\/ui\/gallery\//, /\/PlanningCenterSetupPanel/, /\/playbackStatus\.ts$/, /\/sections\/liveLinkLabels\.test\.ts$/, /\/sections\/ActionRow(\.test)?\.tsx?$/];
const SAYS_ABSENT = /queryBy|queryAllBy|toBeNull|\.not\.|not\.toBe|never |no button|nothing is called|Choose Scan Network|Test connection and Refresh/;

describe("the old Live Link button labels are gone", () => {
  const files = Object.entries(sources).filter(([file]) => !EXEMPT.some((pattern) => pattern.test(file)));

  it("sees the components", () => {
    expect(files.length).toBeGreaterThan(100);
  });

  it("no component or test draws or expects 'Test connection', 'Check again', 'Scan Network' or 'Scan again'", () => {
    const offenders = files.flatMap(([file, text]) => text.split("\n")
      .map((line, index) => ({ file, line: index + 1, text: line }))
      .filter((entry) => OLD.test(entry.text) && !SAYS_ABSENT.test(entry.text))
      .map((entry) => `${entry.file}:${entry.line}: ${entry.text.trim().slice(0, 120)}`));
    expect(offenders).toEqual([]);
  });
});
