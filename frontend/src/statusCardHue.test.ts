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

  it("the StagePilot open card never looks like hover: card colour, backend border, 3px bar, no wash, same on hover", () => {
    const rule = css.match(/\.status-card\[data-tone="backend"\]\[aria-expanded="true"\] \.status-card-surface,\s*\.status-card\[data-tone="backend"\]\[aria-expanded="true"\]:hover \.status-card-surface \{[^}]*\}/)?.[0] ?? "";
    expect(rule).not.toBe("");
    expect(rule).toContain("border-color: var(--ds-integration-backend-soft)");
    expect(rule).toContain("background: var(--ds-card)");
    expect(rule).toContain("inset 3px 0 0 var(--ds-integration-backend-soft)");
    expect(rule).not.toContain("linear-gradient");
    // hover paints panel-strong; the open rule must not use it, or the card would look hovered while open
    expect(rule).not.toContain("surface-panel-strong");
  });
});

describe("status word (owner 2026-10-13: no dot, capitalized, larger)", () => {
  it("is uppercase and sized from the card width, capped at 18px", () => {
    const rule = css.match(/\.status-card-status-word \{[^}]*\}/)?.[0] ?? "";
    expect(rule).toContain("text-transform: uppercase");
    expect(rule).toContain("white-space: nowrap");
    expect(rule).toMatch(/font-size: clamp\(0\.6875rem, calc\(\(100cqw - [\d.]+rem\) \/ [\d.]+\), 1\.125rem\)/);
  });

  it("the compact word steps from 11px down to 9px and NOT CONNECTED may wrap (one line below 360px)", () => {
    expect(css).toContain("font-size: clamp(0.5625rem, calc((100cqw - 0.5rem) / 6.6), 0.6875rem)");
    expect(css).toMatch(/\.status-compact \.status-card-status-word \{[^}]*white-space: normal/);
    expect(css).toMatch(/max-width: 359\.98px\) \{\s*\.connections-status\.status-compact \.status-card-status-word \{\s*white-space: nowrap/);
  });
});
