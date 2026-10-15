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

describe("Now playing rail (owner 2026-10-14)", () => {
  it("rests on the Previous blue; the live colour sweeps down from the top when the timer runs, a lighter band keeps travelling, and the blue sweeps back down on stop", () => {
    expect(css).toMatch(/\.now-playing-panel\.now-playing-panel::before \{\s*background-color: var\(--ds-live-back\)/);
    // Every front is a registered percentage animated top to bottom, so no change fades the whole bar at once.
    for (const prop of ["--rail-rise", "--rail-fall", "--rail-wave"]) {
      expect(css).toMatch(new RegExp(`@property ${prop} \\{\\s*syntax: "<percentage>"`));
    }
    expect(css).toMatch(/\[data-timer="running"\]::before \{\s*animation:\s*now-playing-rise [\d.]+ms ease-out both,\s*now-playing-wave [\d.]+s linear [\d.]+ms infinite/);
    expect(css).toMatch(/\[data-settling="true"\]::before \{\s*--rail-rise: 120%;\s*animation: now-playing-fall/);
    expect(css.match(/@keyframes now-playing-rise \{[\s\S]*?\n\}\n\n\}?/)![0]).toContain("--rail-rise");
    expect(css).toContain("linear-gradient(180deg, var(--ds-primary) calc(var(--rail-rise)");
    expect(css).toContain("var(--ds-primary-focus)");
    expect(css).not.toMatch(/@keyframes now-playing-(rise|settle|pulse) \{\s*(from|0%)[^}]*background-color/);
    // Reduced motion: a running timer shows the live colour without sweeping.
    expect(css).toMatch(/prefers-reduced-motion: reduce\) \{\s*\.now-playing-panel\.now-playing-panel\[data-timer="running"\]::before \{\s*animation: none;\s*background-image: none;\s*background-color: var\(--ds-primary\)/);
  });

  it("on stop the label, elapsed time and glow wait for the blue sweep to start (delayed), on start they change straight away", () => {
    expect(css).toMatch(/\.now-playing-panel \.now-playing-tint \{\s*color: var\(--ds-live-back-text\);\s*transition: color 600ms ease-in-out 380ms/);
    expect(css).toMatch(/\[data-timer="running"\] \.now-playing-tint \{\s*color: var\(--ds-primary-text\);\s*transition: color 700ms ease-out 0ms/);
    expect(css).toMatch(/@property --rail-glow \{\s*syntax: "<color>"/);
    expect(css).toMatch(/transition: --rail-glow 600ms ease-in-out 380ms/);
    const src = readFileSync(`${process.cwd()}/src/components/Dashboard.tsx`, "utf8");
    // The settle starts in the same frame as the stop, so the bar sweep never trails the text.
    expect(src).toMatch(/const previousTimerStatus = useRef\(state\.timer\.status\);\s*useLayoutEffect\(/);
  });

  it("every font-mono number uses the bundled Geist Mono (variable weight), not a CDN or the system monospace", () => {
    expect(css).toMatch(/font-family: "Geist Mono";\s*src: url\("\.\/assets\/fonts\/GeistMono-latin-wght\.woff2"\) format\("woff2"\);[^}]*font-weight: 100 900/);
    expect(css).toMatch(/\.font-mono \{\s*font-family: "Geist Mono", ui-monospace/);
    expect(css).not.toMatch(/https?:\/\/[^)]*GeistMono/i);
  });

  it("the countdown is 4px bigger than before and a hovered song row takes the live red-orange tint", () => {
    const src = readFileSync(`${process.cwd()}/src/components/Dashboard.tsx`, "utf8");
    expect(src).toContain("text-[clamp(calc(3rem+4px),calc(5vw+4px),calc(5.4rem+4px))]");
    expect(src).toContain("can-hover:bg-primary/[0.08]");
    expect(src).toContain("group-hover:bg-primary/20 group-hover:text-primary-text");
    // Generic (non-song) rows keep their own darker hover.
    expect(src).toContain("can-hover:bg-surface-950/40");
  });

  it("the label and elapsed time follow the rail (blue at rest, the live tint while running), and the plan title is 18px", () => {
    const src = readFileSync(`${process.cwd()}/src/components/Dashboard.tsx`, "utf8");
    expect(src).toContain('now-playing-tint text-live-back-text">Now playing</p>');
    expect(src).toMatch(/now-playing-tint text-live-back-text">\{formatDuration\(timerElapsed\)\}/);
    expect(css).toMatch(/\[data-timer="running"\] \.now-playing-tint \{\s*color: var\(--ds-primary-text\)/);
    expect(src).toContain('font-pixel mt-1 break-words !text-[1.125rem]');
  });
});

describe("Manual controls grid (owner 2026-10-14)", () => {
  const columnsAt = (minWidth: string) => {
    const block = css.match(new RegExp(`@container \\(min-width: ${minWidth}\\) \\{\\s*\\.manual-controls-grid \\{\\s*grid-template-columns: repeat\\((\\d), minmax\\(0, 1fr\\)\\)`));
    return block ? Number(block[1]) : null;
  };

  it("is 2 columns by default and only 3 or 6 columns above: always even rows, never 4 + 2 or one column", () => {
    expect(css).toMatch(/\.manual-controls-grid \{\s*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\)/);
    expect(columnsAt("37rem")).toBe(3);
    expect(columnsAt("74.5rem")).toBe(6);
    expect(css).not.toMatch(/\.manual-controls-grid \{[^}]*repeat\((1|4|5)\b/);
  });

  it("adds a column only when each label still fits on one line (12rem columns for the 186px longest label)", () => {
    // 3 x 12rem + 2 x 0.5rem gap = 37rem; 6 x 12rem + 5 x 0.5rem gap = 74.5rem.
    expect(3 * 12 + 2 * 0.5).toBe(37);
    expect(6 * 12 + 5 * 0.5).toBe(74.5);
    expect(12 * 16).toBeGreaterThanOrEqual(186 + 6);
  });
});

describe("status card hover and Disconnected colour (owner 2026-10-14)", () => {
  it("title and description lighten when the whole card is hovered, pressed, focused or open - not only the text", () => {
    expect(css).toMatch(/@media \(hover: hover\) \{\s*\.status-card\.status-card:hover \.status-card-title\.status-card-title,\s*\.status-card\.status-card:hover \.status-card-detail\.status-card-detail \{\s*color: var\(--ds-ink-body\)/);
    for (const trigger of [":active", ":focus-visible", '[aria-expanded="true"]']) {
      expect(css).toContain(`.status-card.status-card${trigger} .status-card-title.status-card-title`);
      expect(css).toContain(`.status-card.status-card${trigger} .status-card-detail.status-card-detail`);
    }
  });

  it("the card no longer relies on the text-only group-hover classes for the title and description", () => {
    const src = readFileSync(`${process.cwd()}/src/components/StatusCard.tsx`, "utf8");
    expect(src).not.toMatch(/status-card-(title|detail)[^"]*group-hover/);
    expect(src).toContain('disconnected: "text-ink"');
  });
});

describe("status card title (owner 2026-10-14)", () => {
  it("is 13px, 2px larger than the 11px micro size, and is not offset from its natural position", () => {
    const rule = css.match(/\.status-card-title \{[^}]*\}/)?.[0] ?? "";
    expect(rule).toMatch(/font-size: clamp\(0\.6875rem, calc\(\(100cqw - [\d.]+rem\) \/ [\d.]+\), 0\.8125rem\)/);
    expect(rule).toContain("line-height: 1.03125rem");
    expect(rule).not.toContain("top:");
    expect(rule).not.toContain("position: relative");
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
