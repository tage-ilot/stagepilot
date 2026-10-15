import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ScrollRegion } from "./ScrollRegion";

// jsdom has no layout, so give the scroller the sizes a browser would report.
const size = (node: HTMLElement, sizes: { scrollTop?: number; clientHeight: number; scrollHeight: number; offsetWidth?: number; clientWidth?: number }) => {
  for (const [key, value] of Object.entries(sizes)) Object.defineProperty(node, key, { configurable: true, writable: true, value });
};
const fades = (frame: HTMLElement) => Array.from(frame.querySelectorAll<HTMLElement>(".scroll-fade"));
const visible = (frame: HTMLElement) => fades(frame).map((fade) => `${fade.dataset.edge}:${fade.dataset.visible}`);

describe("ScrollRegion", () => {
  const mount = () => {
    render(<ScrollRegion className="overflow-y-auto" data-testid="scroller"><p>row</p></ScrollRegion>);
    const scroller = screen.getByTestId("scroller");
    return { scroller, frame: scroller.parentElement! };
  };

  it("draws a bordered frame with a fade overlay on each end that ignores the pointer and is hidden from assistive tech", () => {
    const { frame, scroller } = mount();
    expect(frame).toHaveClass("scroll-region");
    expect(frame).toHaveAttribute("data-axis", "y");
    expect(fades(frame).map((fade) => fade.dataset.edge)).toEqual(["start", "end"]);
    for (const fade of fades(frame)) {
      expect(fade).toHaveAttribute("aria-hidden", "true");
      expect(scroller.contains(fade)).toBe(false); // an overlay above the scroller, so it covers the inputs inside it
    }
  });

  it("shows only the fade for the edge that has more to see, and none when everything fits", () => {
    const { frame, scroller } = mount();
    size(scroller, { scrollTop: 0, clientHeight: 100, scrollHeight: 100 });
    fireEvent.scroll(scroller);
    expect(visible(frame)).toEqual(["start:false", "end:false"]);

    size(scroller, { scrollTop: 0, clientHeight: 100, scrollHeight: 400 });
    fireEvent.scroll(scroller);
    expect(visible(frame)).toEqual(["start:false", "end:true"]);

    size(scroller, { scrollTop: 150, clientHeight: 100, scrollHeight: 400 });
    fireEvent.scroll(scroller);
    expect(visible(frame)).toEqual(["start:true", "end:true"]);

    size(scroller, { scrollTop: 300, clientHeight: 100, scrollHeight: 400 });
    fireEvent.scroll(scroller);
    expect(visible(frame)).toEqual(["start:true", "end:false"]);
  });

  it("stops the fade short of a scrollbar", () => {
    const { frame, scroller } = mount();
    size(scroller, { scrollTop: 0, clientHeight: 100, scrollHeight: 400, offsetWidth: 309, clientWidth: 300 });
    fireEvent.scroll(scroller);
    expect(frame.style.getPropertyValue("--scroll-bar")).toBe("9px");
  });

  it("draws nothing extra when it is not framed (an empty list)", () => {
    render(<ScrollRegion className="overflow-y-auto" data-testid="scroller" framed={false}><p>none</p></ScrollRegion>);
    const scroller = screen.getByTestId("scroller");
    expect(scroller.parentElement).not.toHaveClass("scroll-region");
    expect(scroller.parentElement?.querySelector(".scroll-fade")).toBeNull();
  });

  it("passes its props to the scroller, not the frame, and still calls the caller's onScroll", () => {
    let calls = 0;
    render(<ScrollRegion aria-label="Things" className="overflow-y-auto" data-testid="scroller" onScroll={() => { calls += 1; }} role="region" tabIndex={0}><p>row</p></ScrollRegion>);
    const scroller = screen.getByTestId("scroller");
    expect(scroller).toHaveAttribute("role", "region");
    expect(scroller).toHaveAttribute("aria-label", "Things");
    expect(scroller.parentElement).not.toHaveAttribute("role");
    fireEvent.scroll(scroller);
    expect(calls).toBe(1);
  });

  it("supports a sideways scroller", () => {
    render(<ScrollRegion axis="x" className="overflow-x-auto" data-testid="scroller"><p>wide</p></ScrollRegion>);
    const frame = screen.getByTestId("scroller").parentElement!;
    expect(frame).toHaveAttribute("data-axis", "x");
  });
});
