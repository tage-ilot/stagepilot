import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const startup = vi.hoisted(() => ({
  complete: false,
  applicationState: null as object | null,
  activateConfiguredServices: vi.fn().mockResolvedValue(undefined),
  updater: vi.fn(() => ({})),
}));

vi.mock("./startup/useStartupProgress", () => ({
  useStartupProgress: () => ({
    value: startup.complete ? 100 : 98,
    phase: startup.complete ? "complete" : "preparing-dashboard",
    tone: startup.complete ? "complete" : "normal",
    headline: startup.complete ? "StagePilot is ready" : "Preparing the dashboard",
    valueText: startup.complete ? "Startup complete." : "Dashboard data is ready.",
    moving: !startup.complete,
    complete: startup.complete,
  }),
}));
vi.mock("./hooks/useStagePilot", () => ({
  useStagePilot: () => ({
    state: startup.applicationState,
    health: {},
    live: true,
    error: null,
    settings: { settings: {} },
    activateConfiguredServices: startup.activateConfiguredServices,
  }),
}));
vi.mock("./hooks/useUpdater", () => ({ useUpdater: startup.updater }));
vi.mock("./desktop", async (importOriginal) => {
  const original = await importOriginal<typeof import("./desktop")>();
  return {
    ...original,
    isDesktopShell: () => true,
    desktopBackendStatus: vi.fn(async () => ({
      state: "ready",
      message: "ready",
      port: 8765,
      managed: true,
    })),
    listenForDesktopBackend: vi.fn(async () => null),
  };
});
vi.mock("./components/DesktopTitleBar", () => ({ DesktopTitleBar: () => null }));
vi.mock("./components/Dashboard", () => ({
  Dashboard: () => <div data-testid="dashboard">Dashboard</div>,
}));

import App from "./App";

describe("startup completion integration", () => {
  afterEach(() => {
    startup.complete = false;
    startup.applicationState = null;
    startup.activateConfiguredServices.mockClear();
    startup.updater.mockClear();
    vi.useRealTimers();
  });

  it("reveals the dashboard only after progress completes and preserves activation timing", () => {
    vi.useFakeTimers();
    startup.applicationState = { revision: 1 };
    const { rerender } = render(<App />);

    expect(screen.queryByTestId("dashboard")).not.toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "98");

    startup.complete = true;
    rerender(<App />);
    act(() => vi.advanceTimersByTime(50));
    expect(screen.getByTestId("dashboard")).toBeInTheDocument();
    expect(startup.updater).toHaveBeenLastCalledWith({ ready: true });

    act(() => vi.advanceTimersByTime(1_000));
    expect(startup.activateConfiguredServices).toHaveBeenCalledOnce();
  });
});
