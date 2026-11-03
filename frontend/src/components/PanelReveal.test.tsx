import { act, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePanelPresence } from "../hooks/usePanelPresence";
import { genieDurationMs } from "../lib/genieDuration";
import { genieGeometry } from "../lib/genieGeometry";
import { PanelReveal } from "./PanelReveal";

function Harness() {
  const [on, setOn] = useState(false);
  const { shown, phase } = usePanelPresence(on ? "x" : null, 300);
  return (
    <>
      <button aria-controls="x-configuration" onClick={() => setOn(!on)}>card</button>
      {shown && (
        <PanelReveal phase={phase} durationMs={300} trigger={() => document.querySelector<HTMLElement>("[aria-controls='x-configuration']")}>
          <section id="x-configuration"><input aria-label="field" /></section>
        </PanelReveal>
      )}
    </>
  );
}

describe("PanelReveal", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("stays mounted (inert, aria-hidden) while closing, unmounts after, and returns focus to the card", () => {
    const click = () => act(() => { screen.getByText("card").click(); });
    render(<Harness />);
    click();
    const wrapper = document.querySelector(".panel-reveal")!;
    expect(wrapper.getAttribute("data-phase")).toBe("opening");
    expect(wrapper.hasAttribute("inert")).toBe(false);
    act(() => { vi.advanceTimersByTime(300); });
    expect(wrapper.getAttribute("data-phase")).toBe("open");
    screen.getByLabelText("field").focus();
    click();
    expect(document.querySelector("#x-configuration")).not.toBeNull();
    expect(wrapper.getAttribute("data-phase")).toBe("closing");
    expect(wrapper.hasAttribute("inert")).toBe(true);
    expect(wrapper.getAttribute("aria-hidden")).toBe("true");
    act(() => { vi.advanceTimersByTime(299); });
    expect(document.querySelector("#x-configuration")).not.toBeNull();
    act(() => { vi.advanceTimersByTime(1); });
    expect(document.querySelector("#x-configuration")).toBeNull();
  });

  it("moves focus back to the card when closed from inside the panel", () => {
    render(<Harness />);
    act(() => { screen.getByText("card").click(); });
    act(() => { vi.advanceTimersByTime(300); });
    screen.getByLabelText("field").focus();
    act(() => { screen.getByText("card").click(); });
    expect(document.activeElement).toBe(screen.getByText("card"));
  });
});

describe("genieGeometry", () => {
  const host = { left: 100, width: 1000, top: 222 };
  it("turns a card rect into edge/centre/width percentages and the gap", () => {
    expect(genieGeometry({ left: 600, width: 200, bottom: 200 }, host)).toEqual({ x: 60, w: 20, l: 50, r: 70, gap: 22 });
  });
  it("clamps at the first and last card", () => {
    expect(genieGeometry({ left: 80, width: 200, bottom: 222 }, host)).toMatchObject({ l: 0, r: 18, x: 9, w: 18, gap: 0 });
    expect(genieGeometry({ left: 1000, width: 200, bottom: 230 }, host)).toMatchObject({ l: 90, r: 100, w: 10, gap: 0 });
  });
  it("returns null for a zero-width row", () => {
    expect(genieGeometry({ left: 0, width: 10, bottom: 0 }, { left: 0, width: 0, top: 0 })).toBeNull();
  });
});

describe("genieDurationMs", () => {
  afterEach(() => { window.localStorage.removeItem("spGenieMs"); });
  it("defaults, honours the dev override and ignores nonsense", () => {
    expect(genieDurationMs(300)).toBe(300);
    window.localStorage.setItem("spGenieMs", "3000");
    expect(genieDurationMs(300)).toBe(3000);
    window.localStorage.setItem("spGenieMs", "abc");
    expect(genieDurationMs(300)).toBe(300);
  });
});
