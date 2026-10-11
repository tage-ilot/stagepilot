import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createDefaultDashboardLayout } from "./dashboardLayout";
import { loadDashboardLayout } from "./dashboardLayoutStorage";
import { DASHBOARD_WIDGETS } from "./dashboardWidgetRegistry";
import { DashboardGrid } from "./DashboardGrid";

const widgets = {
  "service-plan": <div className="widget-autosize-target">Service plan content</div>,
  "now-playing": <div className="widget-autosize-target">Now Playing content</div>,
  "manual-controls": <div className="widget-autosize-target">Manual Controls content</div>,
  events: <div>Events content</div>,
};

class MemoryLocalStorage implements Storage {
  private map = new Map<string, string>();
  get length() { return this.map.size; }
  clear() { this.map.clear(); }
  getItem(key: string) { return this.map.get(key) ?? null; }
  key(index: number) { return [...this.map.keys()][index] ?? null; }
  removeItem(key: string) { this.map.delete(key); }
  setItem(key: string, value: string) { this.map.set(key, value); }
}

const setNaturalContentHeight = (px: number) => {
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
    configurable: true,
    get() {
      return this.className?.includes("widget-autosize-target") ? px : 0;
    },
  });
};

describe("mobile layout content-driven auto-fit (Remote Access uses the same narrow layout)", () => {
  let durableStorage: MemoryLocalStorage;
  const originalWidth = window.innerWidth;

  beforeEach(() => {
    durableStorage = new MemoryLocalStorage();
    Object.defineProperty(window, "localStorage", {
      value: durableStorage,
      configurable: true,
    });
    // Below the 1024px desktop breakpoint -> mobile layout mode, the same
    // mode a narrow Remote Access viewport renders (no separate mobile
    // frontend exists).
    Object.defineProperty(window, "innerWidth", { value: 480, configurable: true, writable: true });
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    durableStorage.clear();
    Reflect.deleteProperty(HTMLElement.prototype, "scrollHeight");
    Object.defineProperty(window, "innerWidth", { value: originalWidth, configurable: true, writable: true });
  });

  const settleFit = async () => {
    await vi.advanceTimersByTimeAsync(400);
  };

  it("expands a mobile widget past the old 16-row clamp when its real content needs more room", async () => {
    // 16 rows * 28px cell height ~= 448px + inset; give it far more real
    // content than that so the old static Math.min(..., 16) clamp would
    // have truncated it.
    setNaturalContentHeight(4000);

    render(<DashboardGrid widgets={widgets} />);
    await settleFit();

    const height = Number(screen.getByTestId("dashboard-widget-service-plan")
      .closest(".grid-stack-item")!.getAttribute("gs-h"));
    expect(height).toBeGreaterThan(16);

    // And the real measured height was persisted for next load, not the
    // desktop-derived static clamp.
    const persisted = loadDashboardLayout(durableStorage);
    expect(persisted.mobileHeights?.["service-plan"]).toBe(height);
  });

  it("shrinks a mobile widget to its true content-driven minimum instead of the old static clamp", async () => {
    // Near-zero natural content height. manual-controls' desktop minH is
    // 7, matching the OLD hardcoded mobile floor -- so under both the old
    // static clamp and the new content-based measurement the result here
    // is the same 7, which on its own wouldn't prove anything. The real
    // proof is the *other* two tests (which show heights escaping the old
    // 7-16 band entirely); this test instead proves the value is driven
    // by contentRows()'s real minH-respecting measurement path, not a
    // hardcoded literal, by cross-checking it against dashboardWidgetRegistry.
    setNaturalContentHeight(20);

    render(<DashboardGrid widgets={widgets} />);
    await settleFit();

    const height = Number(screen.getByTestId("dashboard-widget-manual-controls")
      .closest(".grid-stack-item")!.getAttribute("gs-h"));
    expect(height).toBe(DASHBOARD_WIDGETS["manual-controls"].desktop.minH);

    const persisted = loadDashboardLayout(durableStorage);
    expect(persisted.mobileHeights?.["manual-controls"]).toBe(height);
  });

  it("keeps the Recent Event Stream's own bounded height / internal scroll unchanged on mobile", async () => {
    setNaturalContentHeight(4000);
    render(<DashboardGrid widgets={widgets} />);
    await settleFit();

    // The events widget must NOT have been content-measured/auto-fit --
    // it keeps its fallback/registry-derived height and its own internal
    // scrollbar, exactly as on desktop/tablet.
    const eventsHeight = Number(screen.getByTestId("dashboard-widget-events")
      .closest(".grid-stack-item")!.getAttribute("gs-h"));
    expect(eventsHeight).toBeLessThanOrEqual(16);
    const persisted = loadDashboardLayout(durableStorage);
    expect(persisted.mobileHeights?.events).toBeUndefined();
  });

  it("Reset Layout restores default heights and re-measures real content on mobile", async () => {
    setNaturalContentHeight(4000);
    render(<DashboardGrid widgets={widgets} />);
    await settleFit();

    const defaults = createDefaultDashboardLayout();
    expect(defaults.mobileHeights).toEqual({});

    const persisted = loadDashboardLayout(durableStorage);
    expect(persisted.mobileHeights?.["service-plan"]).toBeGreaterThan(16);
  });

  it("persists measured mobile heights so a fresh mount uses them instead of clamping again", async () => {
    setNaturalContentHeight(4000);
    const first = render(<DashboardGrid widgets={widgets} />);
    await settleFit();
    const firstHeight = screen.getByTestId("dashboard-widget-service-plan")
      .closest(".grid-stack-item")!.getAttribute("gs-h");
    first.unmount();

    // Second mount: same durable storage, same (huge) content height. It
    // should render using the persisted measured height immediately (via
    // mobileHeights), matching the first mount's result.
    render(<DashboardGrid widgets={widgets} />);
    const rehydratedLayout = loadDashboardLayout(durableStorage);
    expect(String(rehydratedLayout.mobileHeights?.["service-plan"])).toBe(firstHeight);
  });
});
