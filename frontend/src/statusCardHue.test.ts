import { describe, expect, it } from "vitest";

// vitest stubs CSS modules, so read the stylesheet from disk (node types are not in the app tsconfig).
// @ts-expect-error node builtin without types
import { readFileSync } from "node:fs";

declare const process: { cwd: () => string };
const css: string = readFileSync(`${process.cwd()}/src/index.css`, "utf8");

// DESIGN.md Connection colours: the OPEN status card wears its own connection colour, not primary.
describe("open status card hue", () => {
  it.each(["services", "presentation", "playback", "lighting", "backend"])(
    "%s sets its own hue",
    (tone) => {
      expect(css).toContain(`.status-card[data-tone="${tone}"] { --status-card-hue: var(--ds-integration-${tone}); }`);
    },
  );

  it("draws the open card border, wash and bar from the hue, not primary", () => {
    const rule = css.match(/\.status-card\[aria-expanded="true"\] \.status-card-surface \{[^}]*\}/)?.[0] ?? "";
    expect(rule).toContain("var(--status-card-hue)");
    expect(rule).not.toContain("--ds-primary");
  });

  it("falls back to primary only when a card has no tone", () => {
    expect(css).toContain(".status-card { --status-card-hue: var(--ds-primary); }");
  });

  it("keeps the StagePilot card neutral with a 1px backend border (same hex as the live red)", () => {
    const rule = css.match(/\.status-card\[data-tone="backend"\]\[aria-expanded="true"\] \.status-card-surface \{[^}]*\}/)?.[0] ?? "";
    expect(rule).toContain("border-color: var(--ds-integration-backend-soft)");
    expect(rule).not.toContain("linear-gradient");
  });
});
