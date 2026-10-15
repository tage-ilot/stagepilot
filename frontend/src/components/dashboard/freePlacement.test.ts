import { describe, expect, it } from "vitest";

// The project has no @types/node; declare only what this test uses (same approach as tokensOnlyAll.test.ts).
declare const process: { cwd(): string };
const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
const read = (file: string) => readFileSync(`${process.cwd()}/src/components/dashboard/${file}`, "utf8");

describe("free widget placement (owner 2026-10-14)", () => {
  const grid = read("DashboardGrid.tsx");

  it("turns gravity off on desktop so a widget stays where it is dropped, and keeps one-column mobile packed", () => {
    expect(grid).toContain('float: modeRef.current !== "mobile"');
    expect(grid).toContain('grid.float(mode !== "mobile")');
    expect(grid).toMatch(/const settle = useCallback\(\(target: GridStack\) => \{\s*if \(modeRef\.current === "mobile"\) target\.compact\("compact"\);/);
  });

  it("only pulls widgets upward when asked (Compact layout) or during the initial fit and Reset", () => {
    const direct = [...grid.matchAll(/\.compact\("compact"\)/g)].length;
    // compactGrid (button), the mobile settle helper and the one-off startup/Reset fit; never after a drag or resize on desktop.
    expect(direct).toBe(3);
    expect(grid).not.toMatch(/initializedGrid\.compact\(/);
  });

  it("the spacer tool is gone", () => {
    expect(grid).not.toMatch(/spacer/i);
    expect(read("DashboardLayoutToolbar.tsx")).not.toMatch(/spacer/i);
    expect(read("dashboardLayoutTypes.ts")).not.toMatch(/Spacer/);
  });
});
