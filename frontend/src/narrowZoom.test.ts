import { describe, expect, it } from "vitest";

import backendPanel from "./components/BackendSetupPanel.tsx?raw";
import lightingPanel from "./components/LightingSetupPanel.tsx?raw";
import toast from "./components/ui/Toast.tsx?raw";
import lengths from "./components/PlanningCenterLengths.tsx?raw";
// vitest stubs CSS modules, so read the stylesheet from disk (node types are not in the app tsconfig).
// @ts-expect-error node builtin without types
import { readFileSync } from "node:fs";

declare const process: { cwd: () => string };
const css: string = readFileSync(`${process.cwd()}/src/index.css`, "utf8");

// Every font-size declared inside a rule whose selector mentions the status word.
function statusWordSizes(): string[] {
  const sizes: string[] = [];
  for (const match of css.matchAll(/([^{}]*status-card-status-word[^{}]*)\{([^}]*)\}/g)) {
    for (const size of match[2]!.matchAll(/font-size:\s*([^;]+);/g)) sizes.push(size[1]!.trim());
  }
  return sizes;
}

describe("5.6 narrow screens", () => {
  it("compact status word: capitalized, ONE line, sized from the card so NOT CONNECTED fits, never wraps or ellipsizes", () => {
    const compact = [...css.matchAll(/\.status-compact [^{}]*status-card-status-word[^{}]*\{([^}]*)\}/g)].map((m) => m[1]!).join("\n");
    // 2026-10-13 (owner): NOT CONNECTED is never more than one line, even compact, and nothing overlaps. The size follows the
    // card width (room / 8.3, at most 11px); on phones that is small. The floor is only a guard against a zero-width card.
    expect(compact).toContain("font-size: clamp(0.3125rem, calc((100cqw - 0.4rem) / 8.3), 0.6875rem)");
    expect(compact).toContain("white-space: nowrap");
    expect(compact).not.toContain("white-space: normal");
    expect(compact).not.toContain("text-overflow: ellipsis");
    expect(compact).toContain("overflow: hidden");
    for (const size of statusWordSizes()) {
      expect(size).not.toMatch(/clamp\(0\.4rem/);
      expect(size).not.toMatch(/0\.68rem/);
    }
  });

  it("keeps room above the sticky save bar when a field scrolls into view", () => {
    expect(css).toMatch(/max-width:\s*1023\.98px\)\s*\{\s*html\s*\{\s*scroll-padding-bottom:\s*6rem/);
  });

  it("gives the remaining small controls a 44px touch target on coarse pointers", () => {
    expect(backendPanel).toContain("items-start gap-3 coarse:min-h-ds-touch-target");
    expect(lengths).toContain("coarse:min-w-ds-touch-target");
    expect(lightingPanel).toContain("coarse:after:-inset-y-[9px]");
  });

  it("Toast scrolls inside itself and clears the safe area instead of running off a short screen", () => {
    expect(toast).toContain("max-h-[calc(100dvh-3rem-env(safe-area-inset-bottom))]");
    expect(toast).toContain("overflow-y-auto");
    expect(toast).toContain("mb-[env(safe-area-inset-bottom)]");
  });
});
