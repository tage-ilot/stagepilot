import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

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
// flags -- see DashboardGrid.persistence.test.tsx for the same rationale.
class MemoryLocalStorage implements Storage {
  private map = new Map<string, string>();
  get length() { return this.map.size; }
  clear() { this.map.clear(); }
  getItem(key: string) { return this.map.get(key) ?? null; }
  key(index: number) { return [...this.map.keys()][index] ?? null; }
  removeItem(key: string) { this.map.delete(key); }
  setItem(key: string, value: string) { this.map.set(key, value); }
}

describe("Recent Event Stream hide/show in edit mode", () => {
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

  it("does not expose a hide/show toggle outside of edit mode", () => {
    render(<DashboardGrid widgets={widgets} />);
    expect(screen.queryByRole("button", { name: /event stream/i })).toBeNull();
  });

  it("hides the widget from normal (non-editing) view once toggled, and shows it again", () => {
    render(<DashboardGrid widgets={widgets} />);
    expect(screen.getByTestId("dashboard-widget-events")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));
    fireEvent.click(screen.getByRole("button", { name: "Hide event stream" }));

    // Still visible while editing (so the user can find it again to unhide).
    expect(screen.getByTestId("dashboard-widget-events")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByTestId("dashboard-widget-events")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));
    fireEvent.click(screen.getByRole("button", { name: "Show event stream" }));
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.getByTestId("dashboard-widget-events")).toBeInTheDocument();
  });

  it("persists hidden state across a simulated app restart", () => {
    const first = render(<DashboardGrid widgets={widgets} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));
    fireEvent.click(screen.getByRole("button", { name: "Hide event stream" }));
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    first.unmount();

    const persisted = loadDashboardLayout(durableStorage);
    expect(persisted.eventsHidden).toBe(true);

    render(<DashboardGrid widgets={widgets} />);
    expect(screen.queryByTestId("dashboard-widget-events")).toBeNull();
  });

  it("resetLayout() restores the events widget to visible", () => {
    durableStorage.setItem(DASHBOARD_LAYOUT_KEY, JSON.stringify({
      ...createDefaultDashboardLayout(),
      eventsHidden: true,
    }));
    render(<DashboardGrid widgets={widgets} />);
    expect(screen.queryByTestId("dashboard-widget-events")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));
    fireEvent.click(screen.getByRole("button", { name: "Reset layout" }));

    expect(screen.getByTestId("dashboard-widget-events")).toBeInTheDocument();
    expect(loadDashboardLayout(durableStorage).eventsHidden).toBe(false);
  });
});
