import { describe, expect, it } from "vitest";

// vitest stubs CSS modules, so read the stylesheet from disk (node types are not in the app tsconfig).
// @ts-expect-error node builtin without types
import { readFileSync } from "node:fs";

declare const process: { cwd: () => string };
const css: string = readFileSync(`${process.cwd()}/src/index.css`, "utf8");

function rule(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`(?:^|\\n)${escaped}\\s*\\{([^}]*)\\}`));
  if (!match) throw new Error(`rule ${selector} not found`);
  return match[1]!;
}

describe("loading bar leading edge", () => {
  // Regression: .loading-progress-edge is position:absolute with top:0/bottom:0. If the track is not
  // positioned, the edge resolves against the page and draws a full-height line across the screen as the
  // bar fills. The track must be its containing block.
  it("the track is the containing block for the absolutely positioned edge", () => {
    expect(rule(".loading-progress-edge")).toMatch(/position:\s*absolute/);
    expect(rule(".loading-progress-track")).toMatch(/position:\s*relative/);
  });
});
