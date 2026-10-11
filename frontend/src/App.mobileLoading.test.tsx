import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./desktop", () => ({
  isDesktopShell: () => true,
  backendStartupTitle: () => "Starting StagePilot",
  copyBackendLog: vi.fn(),
  desktopBackendStatus: vi.fn().mockResolvedValue(null),
  listenForDesktopBackend: vi.fn().mockResolvedValue(null),
  listenForDesktopBackendCrashLoop: vi.fn().mockResolvedValue(null),
  restartDesktopBackend: vi.fn(),
  signInWithPlanningCenter: vi.fn(),
}));

vi.mock("./hooks/useStagePilot", () => ({
  useStagePilot: () => ({
    state: null,
    health: null,
    live: false,
    settings: null,
    activateConfiguredServices: vi.fn(),
  }),
}));

vi.mock("./hooks/useUpdater", () => ({
  useUpdater: () => ({}),
}));

vi.mock("./startup/useStartupProgress", () => ({
  useStartupProgress: () => ({
    value: 10,
    tone: "normal",
    phase: "starting",
    moving: true,
    headline: "Starting StagePilot…",
    valueText: "10%",
    complete: false,
  }),
}));

// App/StagePilotApp are only reached once DashboardAccessGate resolves
// access; force desktop shell mode so no network call is required.
vi.mock("./access/AccessContext", async () => {
  const actual = await vi.importActual<typeof import("./access/AccessContext")>(
    "./access/AccessContext",
  );
  return actual;
});

import App from "./App";

describe("StagePilot pre-dashboard loading screen fits narrow (mobile/tablet) viewports", () => {
  const originalWidth = window.innerWidth;

  beforeEach(() => {
    // Narrow phone viewport -- the same width class the Remote Access web
    // dashboard renders at on a phone (see DashboardGrid.mobileFit.test.tsx
    // for the equivalent 1024px desktop/mobile breakpoint precedent).
    Object.defineProperty(window, "innerWidth", { value: 375, configurable: true, writable: true });
  });

  afterEach(() => {
    Object.defineProperty(window, "innerWidth", { value: originalWidth, configurable: true, writable: true });
  });

  it("renders the wordmark with a responsive (non-fixed-180px) size class instead of forcing horizontal overflow", async () => {
    render(<App />);

    const heading = await screen.findByRole("heading", { name: "StagePilot" });
    // The old bug: a hardcoded text-[11.25rem] (180px) with no responsive
    // variant, far wider than a 375px viewport. The fix must scale down at
    // narrow widths and only reach the large desktop size at the lg: breakpoint.
    expect(heading.className.split(/\s+/)).not.toContain("text-[11.25rem]");
    expect(heading.className).toMatch(/\btext-6xl\b/);
    expect(heading.className).toMatch(/\blg:text-\[11\.25rem\]/);
  });

  it("renders the mobile-only circular spinner and marks the bar for CSS hiding on a narrow viewport", async () => {
    render(<App />);

    const bar = await screen.findByRole("progressbar", { name: "StagePilot startup progress" });
    // The real hiding is the max-width: 1023px rule on this class (proved in screenshots).
    expect(bar).toHaveClass("loading-progress-track--desktop-only");
    const fill = bar.querySelector(".loading-progress-fill") as HTMLElement;
    expect(fill.style.transform).toMatch(/^scaleX\(/);
    expect(fill.style.width).toBe("");
    const spinner = document.querySelector(".loading-spinner-circular");
    expect(spinner).not.toBeNull();
    expect(spinner?.className).toContain("loading-spinner-circular--mobile-only");
  });
});
