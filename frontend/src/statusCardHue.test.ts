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

describe("status card title (owner 2026-10-14)", () => {
  it("sits 2px higher than its natural spot, by a relative offset so nothing below it moves", () => {
    const rule = css.match(/\.status-card-title \{[^}]*\}/)?.[0] ?? "";
    expect(rule).toContain("position: relative");
    expect(rule).toContain("top: -2px");
  });
});

describe("status word (owner 2026-10-13: no dot, capitalized, larger)", () => {
  it("is uppercase and sized from the card width, capped at 22px", () => {
    const rule = css.match(/\.status-card-status-word \{[^}]*\}/)?.[0] ?? "";
    expect(rule).toContain("text-transform: uppercase");
    expect(rule).toContain("white-space: nowrap");
    expect(rule).toMatch(/font-size: clamp\(0\.75rem, calc\(\(100cqw - [\d.]+rem\) \/ [\d.]+\), 1\.375rem\)/);
    // Geist Pixel Square, bold by outline (owner, 2026-10-14): one weight in the file, so the heavier look is a stroke.
    expect(rule).toContain('font-family: "Geist Pixel Square"');
    expect(rule).toContain("-webkit-text-stroke: 0.07em currentColor");
    expect(rule).toContain("font-synthesis: none");
  });

  it("the compact word is one line: nowrap, sized from the card (room / 7.95, max 13px), no wrap rule at any width", () => {
    expect(css).toContain("font-size: clamp(0.3125rem, calc((100cqw - 0.4rem) / 7.95), 0.8125rem)");
    expect(css).toMatch(/\.status-compact \.status-card-status-word \{[^}]*white-space: nowrap/);
    expect(css).not.toMatch(/\.status-compact \.status-card-status-word \{[^}]*white-space: normal/);
    expect(css).not.toContain("max-width: 359.98px");
  });
});
