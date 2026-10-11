import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ScanProgress } from "./ScanProgress";

const amount = () => Number(screen.getByRole("progressbar").getAttribute("data-progress"));
const tick = (ms: number) => act(() => vi.advanceTimersByTime(ms));
beforeEach(() => { vi.useFakeTimers(); vi.stubGlobal("requestAnimationFrame", (fn: FrameRequestCallback) => setTimeout(() => fn(Date.now()), 16)); vi.stubGlobal("cancelAnimationFrame", clearTimeout); });
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("continuous song scan progress", () => {
  it("creeps between irregular events, is monotonic with late count, and never snaps or exceeds 90%", () => {
    const view = render(<ScanProgress state="running" steps={0} total={0} />);
    let previous = 0;
    const bar = screen.getByRole("progressbar");
    for (let frame = 0; frame < 12000; frame++) {
      if (frame === 70) view.rerender(<ScanProgress state="running" steps={1} total={0} />);
      if (frame === 170) view.rerender(<ScanProgress state="running" steps={1} total={20} />);
      if (frame === 1000) view.rerender(<ScanProgress state="running" steps={19} total={20} />);
      tick(16);
      const next = Number(bar.getAttribute("data-progress"));
      expect(next).toBeGreaterThanOrEqual(previous);
      if (frame > 2) expect(next).toBeGreaterThan(previous);
      expect(next).toBeLessThanOrEqual(0.9);
      expect(next - previous).toBeLessThan(0.1);
      previous = next;
    }
    const a = amount(); tick(1000); expect(amount()).toBeGreaterThan(a);
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuemin", "0");
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuemax", "100");
    expect(screen.queryByText(/Checking song|step \d/)).not.toBeInTheDocument();
  }, 15000);
  it("unknown count omits determinate attributes, completion glides then holds before showing the result", () => {
    const view = render(<ScanProgress state="running" steps={0} total={0}>Saved</ScanProgress>);
    tick(2000);
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
    const before = amount();
    view.rerender(<ScanProgress state="done" steps={5} total={5}>Saved</ScanProgress>);
    expect(amount()).toBe(before);
    tick(160); expect(amount()).toBeGreaterThan(before); expect(amount()).toBeLessThan(1);
    tick(160); expect(amount()).toBe(1);
    expect(screen.queryByText("Saved")).not.toBeInTheDocument();
    tick(400); expect(screen.queryByRole("progressbar")).not.toBeInTheDocument(); expect(screen.getByText("Saved")).toBeVisible();
  });
  it.each(["failed", "idle"] as const)("%s stops without fake completion", state => {
    const view = render(<ScanProgress state="running" steps={0} total={0} />); tick(1000);
    expect(amount()).toBeLessThan(1);
    view.rerender(<ScanProgress state={state} steps={0} total={0}>Cancelled</ScanProgress>);
    tick(1000); expect(screen.queryByRole("progressbar")).not.toBeInTheDocument(); expect(screen.getByText("Cancelled")).toBeVisible();
  });
  it("reduced motion uses small plain increments and still completes", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
    const view = render(<ScanProgress state="running" steps={5} total={5} />);
    tick(1000); const before = amount(); tick(16);
    expect(amount() - before).toBeLessThanOrEqual(0.001);
    expect(screen.getByRole("progressbar")).toHaveAttribute("data-reduced-motion", "true");
    view.rerender(<ScanProgress state="done" steps={5} total={5} />); tick(320); expect(amount()).toBe(1);
  });
  it("draws the fill with transform, not width", () => {
    render(<ScanProgress state="running" steps={2} total={5} />);
    tick(1000);
    const fill = screen.getByRole("progressbar").firstElementChild as HTMLElement;
    expect(fill.style.transform).toMatch(/^scaleX\(/);
    expect(fill.style.width).toBe("");
  });
});
