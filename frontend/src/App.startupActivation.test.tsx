import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Regression for Greptile P1 (PR #41): startup activation must not be a
// one-shot check that fires before Planning Center OAuth status has
// loaded. An OAuth-only account (no PAT) has nothing to pass the
// readiness check until `planningCenterStatus` resolves, so activation
// must wait for it instead of skipping Planning Center forever.
const startup = vi.hoisted(() => ({
  activateConfiguredServices: vi.fn().mockResolvedValue(undefined),
  planningCenterStatus: null as object | null,
  settings: {
    settings: {
      integration_modes: { service_source: "planning_center" },
    },
  } as object | null,
}));

vi.mock("./startup/useStartupProgress", () => ({
  useStartupProgress: () => ({
    value: 100,
    phase: "complete",
    tone: "complete",
    headline: "StagePilot is ready",
    valueText: "Startup complete.",
    moving: false,
    complete: true,
  }),
}));
vi.mock("./hooks/useStagePilot", () => ({
  useStagePilot: () => ({
    state: { revision: 1 },
    health: {},
    live: true,
    error: null,
    settings: startup.settings,
    planningCenterStatus: startup.planningCenterStatus,
    activateConfiguredServices: startup.activateConfiguredServices,
  }),
}));
vi.mock("./hooks/useUpdater", () => ({ useUpdater: () => ({}) }));
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

describe("startup Planning Center activation readiness", () => {
  afterEach(() => {
    startup.activateConfiguredServices.mockClear();
    startup.planningCenterStatus = null;
    vi.useRealTimers();
  });

  it("does not activate while OAuth-only Planning Center status is still unresolved", () => {
    vi.useFakeTimers();
    startup.planningCenterStatus = null;
    const { rerender } = render(<App />);
    rerender(<App />);

    act(() => vi.advanceTimersByTime(5_000));

    expect(startup.activateConfiguredServices).not.toHaveBeenCalled();
  });

  it("activates once Planning Center OAuth status resolves, without a restart", () => {
    vi.useFakeTimers();
    startup.planningCenterStatus = null;
    const { rerender } = render(<App />);
    rerender(<App />);
    act(() => vi.advanceTimersByTime(5_000));
    expect(startup.activateConfiguredServices).not.toHaveBeenCalled();

    startup.planningCenterStatus = {
      connection_method: "oauth",
      oauth_connected: true,
      oauth_needs_reconnect: false,
    };
    rerender(<App />);
    act(() => vi.advanceTimersByTime(1_000));

    expect(startup.activateConfiguredServices).toHaveBeenCalledOnce();
  });

  it("activates immediately when Planning Center is not the configured service source", () => {
    vi.useFakeTimers();
    startup.settings = {
      settings: {
        integration_modes: { service_source: "demo" },
      },
    };
    startup.planningCenterStatus = null;
    render(<App />);
    act(() => vi.advanceTimersByTime(50));
    act(() => vi.advanceTimersByTime(1_500));

    expect(startup.activateConfiguredServices).toHaveBeenCalledOnce();
    startup.settings = {
      settings: {
        integration_modes: { service_source: "planning_center" },
      },
    };
  });
});
