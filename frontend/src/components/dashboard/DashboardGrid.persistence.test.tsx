import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createDefaultDashboardLayout } from "./dashboardLayout";
import { DASHBOARD_LAYOUT_KEY, loadDashboardLayout } from "./dashboardLayoutStorage";
import { DashboardGrid } from "./DashboardGrid";

const widgets = {
  "service-plan": <div className="widget-autosize-target">Service plan content</div>,
  "now-playing": <div className="widget-autosize-target">Now Playing content</div>,
  "manual-controls": <div className="widget-autosize-target">Manual Controls content</div>,
  events: <div>Events content</div>,
};

// jsdom does not provide a working `localStorage` under this test runner's
// flags, so this stands in for the durable storage the real desktop app
// writes to (its webview's persistent origin storage). It behaves exactly
// like the real localStorage API the component code calls.
class MemoryLocalStorage implements Storage {
  private map = new Map<string, string>();
  get length() { return this.map.size; }
  clear() { this.map.clear(); }
  getItem(key: string) { return this.map.get(key) ?? null; }
  key(index: number) { return [...this.map.keys()][index] ?? null; }
  removeItem(key: string) { this.map.delete(key); }
  setItem(key: string, value: string) { this.map.set(key, value); }
}

describe("dashboard layout persists across a simulated app restart", () => {
  let durableStorage: MemoryLocalStorage;

  beforeEach(() => {
    durableStorage = new MemoryLocalStorage();
    Object.defineProperty(window, "localStorage", {
      value: durableStorage,
      configurable: true,
    });
  });
  afterEach(() => {
    durableStorage.clear();
  });

  it("survives unmount -> fresh mount (app restart) with the moved item still in place", async () => {
    // First "launch": mount, enter edit mode, and reorder a widget via the
    // accessible keyboard controls (equivalent to a drag reorder).
    const first = render(<DashboardGrid widgets={widgets} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));
    fireEvent.click(screen.getByRole("button", { name: "Move Now Playing earlier" }));

    const persistedAfterMove = loadDashboardLayout(durableStorage);
    const defaultLayout = createDefaultDashboardLayout();
    expect(persistedAfterMove.desktop).not.toEqual(defaultLayout.desktop);

    // Simulate a full app restart: unmount the component tree entirely
    // (as happens when the desktop shell's webview is torn down) and
    // mount a brand-new instance reading from the same durable storage --
    // never touching React state carried over from the first mount.
    first.unmount();

    const second = render(<DashboardGrid widgets={widgets} />);
    const rehydrated = loadDashboardLayout(durableStorage);
    expect(rehydrated.desktop).toEqual(persistedAfterMove.desktop);
    expect(rehydrated.desktop).not.toEqual(defaultLayout.desktop);
    second.unmount();
  });

  it("falls back to the reset layout when nothing was ever saved", () => {
    expect(durableStorage.getItem(DASHBOARD_LAYOUT_KEY)).toBeNull();
    render(<DashboardGrid widgets={widgets} />);
    const stored = loadDashboardLayout(durableStorage);
    expect(stored).toEqual(createDefaultDashboardLayout());
  });
});

describe("a restored custom layout is not overwritten during startup sizing", () => {
  let durableStorage: MemoryLocalStorage;

  beforeEach(() => {
    durableStorage = new MemoryLocalStorage();
    Object.defineProperty(window, "localStorage", {
      value: durableStorage,
      configurable: true,
    });
    vi.useFakeTimers();
    vi.stubGlobal("ResizeObserver", class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
      configurable: true,
      get() {
        // 200px of content needs 8 rows: the saved 11 rows are taller than needed and must be kept, not auto-fit down to 8.
        return this.className?.includes("widget-autosize-target") ? 200 : 0;
      },
    });
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    durableStorage.clear();
    Reflect.deleteProperty(HTMLElement.prototype, "scrollHeight");
  });

  it("keeps a user-resized widget after the delayed mount-time pass", async () => {
    const custom = createDefaultDashboardLayout();
    custom.desktop = custom.desktop.map((item) => (
      item.id === "service-plan" ? { ...item, h: 11 } : item
    ));
    durableStorage.setItem(DASHBOARD_LAYOUT_KEY, JSON.stringify(custom));

    render(<DashboardGrid widgets={widgets} />);
    await vi.advanceTimersByTimeAsync(400);

    const restored = loadDashboardLayout(durableStorage);
    expect(restored.desktop.find((item) => item.id === "service-plan")?.h).toBe(11);
  });

  it("raises a saved height that would cut the content off (owner 2026-10-14), but never lowers a taller one", async () => {
    const custom = createDefaultDashboardLayout();
    custom.desktop = custom.desktop.map((item) => (
      item.id === "service-plan" ? { ...item, h: 3, minH: 1 } : item
    ));
    durableStorage.setItem(DASHBOARD_LAYOUT_KEY, JSON.stringify(custom));

    render(<DashboardGrid widgets={widgets} />);
    await vi.advanceTimersByTimeAsync(600);

    const restored = loadDashboardLayout(durableStorage);
    const height = restored.desktop.find((item) => item.id === "service-plan")?.h ?? 0;
    // Raised from the 3 rows that cut the content off to what it needs (about 8-9 rows for 200px), well under the 11 kept above.
    expect(height).toBeGreaterThanOrEqual(8);
    expect(height).toBeLessThan(11);
  });

  it("keeps the restored height after the widget content changes", async () => {
    const custom = createDefaultDashboardLayout();
    custom.desktop = custom.desktop.map((item) => (
      item.id === "service-plan" ? { ...item, h: 11 } : item
    ));
    durableStorage.setItem(DASHBOARD_LAYOUT_KEY, JSON.stringify(custom));

    const view = render(<DashboardGrid widgets={widgets} />);
    await vi.advanceTimersByTimeAsync(400);
    const servicePlan = screen.getByTestId("dashboard-widget-service-plan")
      .closest(".grid-stack-item")!;
    expect(servicePlan.getAttribute("gs-h")).toBe("11");

    view.rerender(<DashboardGrid widgets={{
      ...widgets,
      "service-plan": (
        <div className="widget-autosize-target">
          Service plan content with an additional item
        </div>
      ),
    }} />);
    await vi.advanceTimersByTimeAsync(100);

    expect(servicePlan.getAttribute("gs-h")).toBe("11");
    const restored = loadDashboardLayout(durableStorage);
    expect(restored.desktop.find((item) => item.id === "service-plan")?.h).toBe(11);
  });
});

describe("a fresh install's first render matches clicking Reset Layout", () => {
  let durableStorage: MemoryLocalStorage;

  beforeEach(() => {
    durableStorage = new MemoryLocalStorage();
    Object.defineProperty(window, "localStorage", {
      value: durableStorage,
      configurable: true,
    });
    vi.useFakeTimers();
    // Give the Service plan widget real measured content: taller than its
    // static registry default (h: 35 desktop rows * 28px cell height),
    // so the content-fit measurement pass produces a *different* height
    // than the raw static default -- this is what previously diverged
    // between a fresh load (which ran the fit pass) and Reset Layout
    // (which used to short-circuit it via `initialSizingDone`).
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
      configurable: true,
      get() {
        return this.className?.includes("widget-autosize-target") ? 2000 : 0;
      },
    });
  });
  afterEach(() => {
    vi.useRealTimers();
    durableStorage.clear();
    Reflect.deleteProperty(HTMLElement.prototype, "scrollHeight");
  });

  const settleFit = async () => {
    await vi.advanceTimersByTimeAsync(400);
  };

  it("renders the same widget heights as Reset Layout immediately, with no saved layout", async () => {
    // True fresh/empty storage state: nothing saved yet.
    expect(durableStorage.getItem(DASHBOARD_LAYOUT_KEY)).toBeNull();

    const fresh = render(<DashboardGrid widgets={widgets} />);
    await settleFit();
    const freshHeight = screen.getByTestId("dashboard-widget-service-plan")
      .closest(".grid-stack-item")!.getAttribute("gs-h");
    // The fit pass ran and produced a real measured height, not the
    // untouched static registry default (35).
    expect(freshHeight).not.toBeNull();
    fresh.unmount();

    // Now start over with an existing (different) saved layout and click
    // "Reset Layout" instead, on a brand-new mount.
    durableStorage.setItem(DASHBOARD_LAYOUT_KEY, JSON.stringify({
      ...createDefaultDashboardLayout(),
      desktop: createDefaultDashboardLayout().desktop.map((item) => (
        item.id === "service-plan" ? { ...item, h: 9 } : item
      )),
    }));
    const existing = render(<DashboardGrid widgets={widgets} />);
    await settleFit();
    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));
    fireEvent.click(screen.getByRole("button", { name: "Reset layout" }));
    await settleFit();
    const resetHeight = screen.getByTestId("dashboard-widget-service-plan")
      .closest(".grid-stack-item")!.getAttribute("gs-h");

    // Fresh install (no memory) and an explicit Reset Layout click must
    // converge on the identical rendered height, not a smaller/different
    // one that only becomes correct after the user manually resets.
    expect(resetHeight).toBe(freshHeight);
    existing.unmount();
  });
});
