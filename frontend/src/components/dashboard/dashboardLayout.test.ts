import { describe, expect, it } from "vitest";

import {
  createDefaultDashboardLayout,
  dashboardModeForWidth,
  migrateDashboardOrder,
  moveDashboardItem,
  orderedLayoutItems,
  parseDashboardLayout,
} from "./dashboardLayout";
import {
  DASHBOARD_COLUMNS,
  DASHBOARD_WIDGET_IDS,
} from "./dashboardLayoutTypes";
import { DASHBOARD_WIDGETS } from "./dashboardWidgetRegistry";

describe("dashboard layout model", () => {
  it("creates valid defaults with every required widget exactly once", () => {
    const layout = createDefaultDashboardLayout();
    expect(parseDashboardLayout(layout)).toEqual(layout);
    for (const mode of ["desktop", "tablet"] as const) {
      expect(layout[mode].map(({ id }) => id).sort()).toEqual([...DASHBOARD_WIDGET_IDS].sort());
      expect(new Set(layout[mode].map(({ id }) => id)).size).toBe(DASHBOARD_WIDGET_IDS.length);
      expect(layout[mode].every((item) =>
        item.x >= 0 && item.y >= 0 && item.w > 0 && item.h > 0
        && item.x + item.w <= DASHBOARD_COLUMNS[mode])).toBe(true);
    }
  });

  it("puts the live state first for new mobile layouts", () => {
    expect(createDefaultDashboardLayout().mobileOrder[0]).toBe("now-playing");
  });

  it("defines usable constraints for every widget", () => {
    for (const definition of Object.values(DASHBOARD_WIDGETS)) {
      for (const mode of ["desktop", "tablet"] as const) {
        const item = definition[mode];
        expect(item.minW).toBeGreaterThan(0);
        expect(item.minH).toBeGreaterThan(0);
        expect(item.w).toBeGreaterThanOrEqual(item.minW!);
        expect(item.h).toBeGreaterThanOrEqual(item.minH!);
      }
    }
  });

  it("fills the default desktop columns to the same total height", () => {
    const desktop = createDefaultDashboardLayout().desktop;
    const servicePlan = desktop.find(({ id }) => id === "service-plan")!;
    const rightColumnBottom = Math.max(
      ...desktop
        .filter(({ x }) => x === 7)
        .map(({ y, h }) => y + h),
    );

    expect(servicePlan.y + servicePlan.h).toBe(rightColumnBottom);
  });

  it("rejects unknown, duplicate, missing, negative, and zero-sized items", () => {
    const valid = createDefaultDashboardLayout();
    expect(parseDashboardLayout({
      ...valid,
      desktop: [...valid.desktop, { ...valid.desktop[0], id: "unknown" }],
    })).toBeNull();
    expect(parseDashboardLayout({
      ...valid,
      desktop: [...valid.desktop, valid.desktop[0]],
    })).toBeNull();
    expect(parseDashboardLayout({
      ...valid,
      desktop: valid.desktop.slice(1),
    })).toBeNull();
    expect(parseDashboardLayout({
      ...valid,
      desktop: valid.desktop.map((item, index) => index ? item : { ...item, x: -1 }),
    })).toBeNull();
    expect(parseDashboardLayout({
      ...valid,
      desktop: valid.desktop.map((item, index) => index ? item : { ...item, h: 0 }),
    })).toBeNull();
  });

  it("constrains oversized widgets to the active column count", () => {
    const valid = createDefaultDashboardLayout();
    const parsed = parseDashboardLayout({
      ...valid,
      tablet: valid.tablet.map((item, index) =>
        index ? item : { ...item, x: 99, w: 99 }),
    });
    expect(parsed?.tablet[0]).toMatchObject({ x: 0, w: 6 });
  });

  it("drops spacers saved by older versions instead of rejecting the whole layout", () => {
    const valid = createDefaultDashboardLayout();
    const spacer = { id: "spacer-old", kind: "spacer", x: 0, y: 40, w: 2, h: 3, minW: 1, minH: 1 };
    const parsed = parseDashboardLayout({
      ...valid,
      desktop: [...valid.desktop, spacer],
      tablet: [...valid.tablet, spacer],
      mobileOrder: [...valid.mobileOrder, "spacer-old"],
      mobileHeights: { ...valid.mobileHeights, "spacer-old": 3 },
    });
    expect(parsed).not.toBeNull();
    expect(parsed!.desktop.map(({ id }) => id).sort()).toEqual(valid.desktop.map(({ id }) => id).sort());
    expect(parsed!.tablet.some(({ kind }) => (kind as string) === "spacer")).toBe(false);
    expect(parsed!.mobileOrder).toEqual(valid.mobileOrder);
    expect(parsed!.mobileHeights).not.toHaveProperty("spacer-old");
  });

  it("moves items deterministically without losing any widget", () => {
    const items = createDefaultDashboardLayout().desktop;
    const moved = moveDashboardItem(items, "service-plan", 1);
    expect(orderedLayoutItems(moved).map(({ id }) => id)).toHaveLength(items.length);
    expect(new Set(moved.map(({ id }) => id))).toEqual(new Set(items.map(({ id }) => id)));
    expect(moveDashboardItem(moved, "missing" as never, 1)).toBe(moved);
  });

  it("selects stable desktop, tablet, and mobile breakpoints", () => {
    expect(dashboardModeForWidth(1200)).toBe("desktop");
    expect(dashboardModeForWidth(800)).toBe("mobile");
    expect(dashboardModeForWidth(500)).toBe("mobile");
  });
});
describe("v1 dashboard order migration", () => {
  const order = [
    "now-playing",
    "service-plan",
    "manual-controls",
    "events",
    "readiness",
  ] as const;

  it("preserves a valid v1 order across the new layouts", () => {
    const migrated = migrateDashboardOrder([...order]);
    const currentOrder = order.filter((id) => id !== "readiness");
    expect(migrated?.mobileOrder).toEqual(currentOrder);
    expect(migrated?.desktop.map(({ id }) => id)).toEqual(currentOrder);
    expect(parseDashboardLayout(migrated)).not.toBeNull();
  });

  it.each([
    ["missing", order.slice(1)],
    ["duplicate", [...order.slice(0, 4), order[0]]],
    ["unknown", [...order.slice(0, 4), "unknown"]],
  ])("rejects %s v1 data", (_case, value) => {
    expect(migrateDashboardOrder(value)).toBeNull();
  });
});
