import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const desktop = vi.hoisted(() => ({
  restartDesktopBackend: vi.fn().mockResolvedValue(true),
  status: {
    state: "failed" as "failed" | "starting",
    message:
      "The StagePilot server stopped unexpectedly.",
    port: 8765,
    managed: true,
    failure_kind: "server_exited" as const,
    log_path: "/tmp/stagepilot-backend.log",
  },
}));

vi.mock("./desktop", async (importOriginal) => {
  const original = await importOriginal<typeof import("./desktop")>();
  return {
    ...original,
    isDesktopShell: () => true,
    desktopBackendStatus: vi.fn(async () => desktop.status),
    listenForDesktopBackend: vi.fn(async () => null),
    restartDesktopBackend: desktop.restartDesktopBackend,
  };
});
vi.mock("./hooks/useStagePilot", () => ({
  useStagePilot: () => ({
    state: null,
    error: "Live connection interrupted; reconnecting.",
    settings: null,
    activateConfiguredServices: vi.fn(),
  }),
}));
vi.mock("./hooks/useUpdater", () => ({ useUpdater: () => ({}) }));
vi.mock("./components/DesktopTitleBar", () => ({ DesktopTitleBar: () => null }));
vi.mock("./components/Dashboard", () => ({ Dashboard: () => null }));

import App from "./App";

describe("backend startup recovery", () => {
  beforeEach(() => {
    desktop.restartDesktopBackend.mockClear();
    desktop.status.state = "failed";
  });

  it("shows only the primary descriptor during a normal backend start", async () => {
    desktop.status.state = "starting";
    render(<App />);

    expect(await screen.findByText("Starting the local backend")).toBeInTheDocument();
    expect(
      screen.queryByText(/server stopped unexpectedly/),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Live connection interrupted; reconnecting."),
    ).not.toBeInTheDocument();
  });

  it("shows the specific server failure instead of a reconnect loop", async () => {
    render(<App />);

    expect(await screen.findByText("The StagePilot server stopped")).toBeInTheDocument();
    expect(screen.getByText(/The StagePilot server stopped unexpectedly/)).toBeInTheDocument();
    expect(screen.queryByText("Live connection interrupted; reconnecting.")).not.toBeInTheDocument();
    const progress = screen.getByRole("progressbar", { name: "StagePilot startup progress" });
    expect(progress).toHaveClass("loading-progress-track--failed");
    expect(progress).toHaveAttribute("aria-valuenow", "1");
    expect(progress).toHaveAttribute(
      "aria-valuetext",
      "Startup stopped because the local backend reported an error.",
    );
    expect(screen.getByRole("button", { name: "Copy Backend Log" })).toBeInTheDocument();
  });

  it("offers a bounded supervisor retry action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Retry Backend" }));
    await waitFor(() => expect(desktop.restartDesktopBackend).toHaveBeenCalledOnce());
  });

  it("renders the failure actions with the Button primitive at touch size", async () => {
    render(<App />);

    const retry = await screen.findByRole("button", { name: "Retry Backend" });
    const copy = screen.getByRole("button", { name: "Copy Backend Log" });
    expect(retry).toHaveAttribute("data-variant", "destructive-soft");
    expect(copy).toHaveAttribute("data-variant", "secondary");
    expect(retry).toHaveAttribute("data-size", "touch");
    expect(copy).toHaveAttribute("data-size", "touch");
  });
});
