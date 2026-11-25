import { describe, expect, it, vi } from "vitest";

import { createDefaultDashboardLayout } from "./dashboardLayout";
import {
  DASHBOARD_LAYOUT_INVALID_KEY,
  DASHBOARD_LAYOUT_KEY,
  DASHBOARD_ORDER_V1_KEY,
  loadDashboardLayout,
  saveDashboardLayout,
} from "./dashboardLayoutStorage";

class MemoryStorage {
  values = new Map<string, string>();
  getItem(key: string) {
    return this.values.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.values.set(key, value);
  }
}

describe("dashboard layout storage", () => {
  it("loads valid v2 data without consulting or replacing v1", () => {
    const storage = new MemoryStorage();
    const expected = createDefaultDashboardLayout();
    storage.setItem(DASHBOARD_LAYOUT_KEY, JSON.stringify(expected));
    storage.setItem(DASHBOARD_ORDER_V1_KEY, JSON.stringify([...expected.mobileOrder].reverse()));

    expect(loadDashboardLayout(storage)).toEqual(expected);
    expect(storage.getItem(DASHBOARD_ORDER_V1_KEY)).not.toBeNull();
  });

  it("migrates valid v1 data once and retains the rollback key", () => {
    const storage = new MemoryStorage();
    const order = ["events", "readiness", "manual-controls", "now-playing", "service-plan"];
    storage.setItem(DASHBOARD_ORDER_V1_KEY, JSON.stringify(order));

    const migrated = loadDashboardLayout(storage);

    expect(migrated.mobileOrder).toEqual(order.filter((id) => id !== "readiness"));
    expect(JSON.parse(storage.getItem(DASHBOARD_LAYOUT_KEY)!)).toEqual(migrated);
    expect(storage.getItem(DASHBOARD_ORDER_V1_KEY)).toBe(JSON.stringify(order));
  });

  it("falls back safely and preserves corrupt v2 data for diagnostics", () => {
    const storage = new MemoryStorage();
    storage.setItem(DASHBOARD_LAYOUT_KEY, "{\"broken\":");

    expect(loadDashboardLayout(storage)).toEqual(createDefaultDashboardLayout());
    expect(storage.getItem(DASHBOARD_LAYOUT_INVALID_KEY)).toBe("{\"broken\":");
  });

  it("preserves structurally invalid v2 data for diagnostics", () => {
    const storage = new MemoryStorage();
    storage.setItem(DASHBOARD_LAYOUT_KEY, JSON.stringify({ version: 2, desktop: [] }));

    loadDashboardLayout(storage);

    expect(storage.getItem(DASHBOARD_LAYOUT_INVALID_KEY)).toContain("\"version\":2");
  });

  it("does not crash when storage writes fail", () => {
    const storage = {
      getItem: vi.fn(() => null),
      setItem: vi.fn(() => {
        throw new Error("quota");
      }),
    };
    expect(() => loadDashboardLayout(storage)).not.toThrow();
    expect(saveDashboardLayout(storage, createDefaultDashboardLayout())).toBe(false);
  });
});

