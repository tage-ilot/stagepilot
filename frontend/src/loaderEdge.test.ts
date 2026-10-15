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

describe("loading bar leading edge (removed, owner 2026-10-14)", () => {
  // The white and cyan blocks that led the fill are gone. The track stays positioned for the striped scan overlay.
  it("has no leading edge element or style any more", () => {
    expect(css).not.toContain("loading-progress-edge");
    expect(rule(".loading-progress-track")).toMatch(/position:\s*relative/);
  });
});
