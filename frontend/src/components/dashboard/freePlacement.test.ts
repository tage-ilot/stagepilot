import { describe, expect, it } from "vitest";

// The project has no @types/node; declare only what this test uses (same approach as tokensOnlyAll.test.ts).
declare const process: { cwd(): string };
const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
const read = (file: string) => readFileSync(`${process.cwd()}/src/components/dashboard/${file}`, "utf8");

describe("widget placement: free columns, upward gravity (owner 2026-10-14)", () => {
  const grid = read("DashboardGrid.tsx");

  it("keeps upward gravity on in every layout, so no empty space opens above a widget", () => {
    expect(grid).toContain("float: false,");
    expect(grid).toContain("grid.float(false);");
    expect(grid).not.toContain("float: modeRef.current");
    expect(grid).toMatch(/const settle = useCallback\(\(target: GridStack\) => \{\s*if \(modeRef\.current === "mobile"\) target\.compact\("compact"\);/);
  });

  it("never repacks desktop widgets in reading order after a drag or resize (that would shuffle them sideways)", () => {
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
