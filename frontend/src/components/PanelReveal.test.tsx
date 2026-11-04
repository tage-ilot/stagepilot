import { act, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePanelPresence } from "../hooks/usePanelPresence";
import { genieDurationMs } from "../lib/genieDuration";
import { PanelReveal } from "./PanelReveal";

/** A just-enough Web Animations stand-in: records every animate() call and lets a test finish or inspect each one. */
class FakeAnimation {
  playState: AnimationPlayState = "running";
  onfinish: (() => void) | null = null;
  reversed = 0;
  cancelled = false;
  effect = { setKeyframes: vi.fn() };
  constructor(readonly target: HTMLElement, readonly frames: Keyframe[], readonly options: KeyframeAnimationOptions) {}
  reverse() { this.reversed += 1; this.playState = "running"; }
  cancel() { this.cancelled = true; this.playState = "idle"; }
  finish() { this.playState = "finished"; this.onfinish?.(); }
}

let log: FakeAnimation[] = [];
const props = (animation: FakeAnimation) => new Set(animation.frames.flatMap((frame) => Object.keys(frame)));
const live = () => log.filter((animation) => !animation.cancelled);
const finishAll = () => act(() => { for (const animation of live()) if (animation.playState === "running") animation.finish(); });
const slides = () => log.filter((animation) => animation.options.duration === 280);

// Geometry: a 1000 px wide panel at x=100 (section top 254), cards 180 px wide, bottom edge at 234.
const rects = new Map<Element, DOMRect>();
const box = (left: number, top: number, width: number, height: number) => ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }) as DOMRect;
const CARD_LEFT = { a: 100, b: 560, c: 920 } as Record<string, number>; // first, middle, last
const placeCard = (id: string) => rects.set(document.querySelector(`[data-card="${id}"]`)!, box(CARD_LEFT[id]!, 142, 180, 92));

function Harness({ initialKey = "a", reduced = false }: { initialKey?: string; reduced?: boolean }) {
  void reduced;
  const [on, setOn] = useState(false);
  const [key, setKey] = useState(initialKey);
  const { shown, phase } = usePanelPresence(on ? key : null, 500, null, { swap: true });
  return (
    <>
      {["a", "b", "c"].map((id) => (
        <button key={id} data-card={id} aria-controls={`${id}-configuration`} onClick={() => { if (on && key !== id) setKey(id); else { setKey(id); setOn(!on); } }}>card {id}</button>
      ))}
      {shown && (
        <PanelReveal phase={phase} durationMs={500} triggerKey={shown} trigger={() => document.querySelector<HTMLElement>(`[aria-controls='${shown}-configuration']`)}>
          <section id={`${shown}-configuration`}>
            <h2>panel {shown}</h2>
            <input aria-label="field" />
          </section>
        </PanelReveal>
      )}
      <div data-testid="below">content below</div>
    </>
  );
}

const root = () => document.querySelector<HTMLElement>(".panel-reveal")!;
const notch = () => document.querySelector<HTMLElement>(".panel-reveal__notch")!;
const click = (name: string) => act(() => { screen.getByText(name).click(); });

describe("PanelReveal", () => {
  let reduced = false;
  let narrow = false;
  beforeEach(() => {
    vi.useFakeTimers();
    log = [];
    rects.clear();
    reduced = false;
    narrow = false;
    HTMLElement.prototype.animate = function (this: HTMLElement, frames: Keyframe[], options: KeyframeAnimationOptions) {
      const animation = new FakeAnimation(this, frames, options);
      log.push(animation);
      return animation as unknown as Animation;
    } as unknown as typeof HTMLElement.prototype.animate;
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      return rects.get(this) ?? (this.classList?.contains("panel-reveal") ? box(100, 234, 1000, 640) : this.tagName === "SECTION" ? box(100, 254, 1000, 600) : box(0, 0, 0, 0));
    });
    const realComputed = window.getComputedStyle.bind(window);
    vi.spyOn(window, "getComputedStyle").mockImplementation((element: Element, pseudo?: string | null) => {
      const real = realComputed(element, pseudo);
      if (element.tagName === "SECTION") return { ...real, borderTopColor: "rgb(213, 51, 103)", marginTop: "20px", position: "static" } as CSSStyleDeclaration;
      if (element.classList?.contains("panel-reveal__notch")) {
        const x = slides().length ? 321 : 0;
        return { ...real, transform: x ? `matrix(0.7, 0.7, -0.7, 0.7, ${x}, 0)` : "none", borderTopColor: "rgb(100, 100, 100)" } as CSSStyleDeclaration;
      }
      return real;
    });
    window.matchMedia = ((query: string) => ({ matches: query.includes("reduce") ? reduced : query.includes("max-width") ? narrow : false, addEventListener() {}, removeEventListener() {} })) as unknown as typeof window.matchMedia;
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
    delete (HTMLElement.prototype as Partial<HTMLElement>).animate;
  });

  const open = (cardId = "a") => {
    render(<Harness initialKey={cardId} />);
    for (const id of Object.keys(CARD_LEFT)) placeCard(id);
    click(`card ${cardId}`);
  };

  it("stays mounted (inert, aria-hidden) while closing, unmounts after, and returns focus to the card", () => {
    open();
    expect(root().getAttribute("data-phase")).toBe("opening");
    expect(root().hasAttribute("inert")).toBe(false);
    act(() => { vi.advanceTimersByTime(500); });
    expect(root().getAttribute("data-phase")).toBe("open");
    screen.getByLabelText("field").focus();
    click("card a");
    expect(document.querySelector("#a-configuration")).not.toBeNull();
    expect(root().getAttribute("data-phase")).toBe("closing");
    expect(root().hasAttribute("inert")).toBe(true);
    expect(root().getAttribute("aria-hidden")).toBe("true");
    expect(document.activeElement).toBe(screen.getByText("card a"));
    act(() => { vi.advanceTimersByTime(499); });
    expect(document.querySelector("#a-configuration")).not.toBeNull();
    act(() => { vi.advanceTimersByTime(1); });
    expect(document.querySelector("#a-configuration")).toBeNull();
  });

  it("opens with transform on the wrappers and the content below, opacity on the content and the notch, nothing else", () => {
    open("a");
    const targets = log.map((animation) => animation.target);
    expect(targets).toContain(document.querySelector(".panel-reveal__x"));
    expect(targets).toContain(document.querySelector(".panel-reveal__y"));
    expect(targets).toContain(notch());
    for (const animation of log) {
      const keys = props(animation);
      expect([...keys].filter((key) => key !== "offset" && key !== "transform" && key !== "opacity")).toEqual([]);
      expect(animation.options.duration).toBe(500);
      expect(animation.options.fill).toBe("both");
    }
    const x = log.find((animation) => animation.target.classList.contains("panel-reveal__x"))!;
    // strip = card width (180/1000), centred on the card: card centre 190 vs panel centre 600.
    expect(x.frames[0]!.transform).toBe("translateX(-410px) scaleX(0.18)");
    expect(x.options.easing).toBe("cubic-bezier(.65,0,.35,1)");
    const y = log.find((animation) => animation.target.classList.contains("panel-reveal__y"))!;
    expect(y.frames[0]!.transform).toBe("translateY(-20px) scaleY(0.02)"); // gap = 254 - 234, read from layout
    expect(y.options.easing).toBe("cubic-bezier(.4,0,.2,1)");
  });

  it("puts the notch on the card centre for the first, middle and last card, without sliding on the first open", () => {
    for (const [id, centre] of [["a", 190], ["b", 650], ["c", 1010]] as const) {
      const { unmount } = render(<Harness initialKey={id} />);
      for (const card of Object.keys(CARD_LEFT)) placeCard(card);
      click(`card ${id}`);
      // the root starts at x=100, the notch is 16 px wide: translate = centre - 100 - 8
      expect(notch().style.getPropertyValue("--notch-x")).toBe(`${centre - 100 - 8}px`);
      expect(notch().style.getPropertyValue("--notch-accent")).toBe("rgb(213, 51, 103)");
      expect(slides()).toHaveLength(0);
      unmount();
      log = [];
    }
  });

  it("turns the running animation round (reverse) instead of restarting, and the close is not replayed", () => {
    open();
    const before = live().length;
    act(() => { vi.advanceTimersByTime(200); });
    click("card a"); // close mid-open
    expect(root().getAttribute("data-phase")).toBe("closing");
    expect(log).toHaveLength(before);
    expect(live().every((animation) => animation.reversed === 1)).toBe(true);
    click("card a"); // and open again mid-close
    expect(root().getAttribute("data-phase")).toBe("opening");
    expect(live().every((animation) => animation.reversed === 2)).toBe(true);
  });

  it("moves the content below with the panel, and refits it in place if the panel grows after the animation started", () => {
    open();
    const below = log.find((animation) => animation.target === screen.getByTestId("below"))!;
    expect(below.frames[0]!.transform).toBe("translateY(-608px)"); // -(0.98 * 600 + 20)
    expect(below.frames[1]!.transform).toBe("translateY(0px)");
    Object.defineProperty(document.querySelector("section")!, "offsetHeight", { configurable: true, value: 700 });
    act(() => { window.dispatchEvent(new Event("resize")); });
    expect(below.effect.setKeyframes).toHaveBeenCalledWith([{ transform: "translateY(-706px)" }, { transform: "translateY(0px)" }]);
  });

  it("closes with the reversed frames from a settled open", () => {
    open();
    finishAll();
    expect(live()).toHaveLength(0);
    click("card a");
    const x = log.filter((animation) => animation.target.classList.contains("panel-reveal__x")).at(-1)!;
    expect(x.frames[0]!.transform).toBe("translateX(0px) scaleX(1)");
    expect(x.frames[1]!.transform).toBe("translateX(-410px) scaleX(0.18)");
    const content = log.filter((animation) => animation.target.closest("section")).at(-1)!;
    expect(content.frames).toEqual([{ opacity: 1, offset: 0 }, { opacity: 0, offset: 0.4 }, { opacity: 0, offset: 1 }]);
  });

  it("leaves no animations and no will-change at rest after an open", () => {
    open();
    expect(document.querySelector<HTMLElement>(".panel-reveal__x")!.style.willChange).toBe("transform");
    finishAll();
    expect(log.every((animation) => animation.cancelled)).toBe(true);
    for (const element of document.querySelectorAll<HTMLElement>("*")) expect(element.style.willChange).toBe("");
  });

  it("reduced motion: a 120 ms fade only, no transform, no notch motion", () => {
    reduced = true;
    render(<Harness />);
    for (const id of Object.keys(CARD_LEFT)) placeCard(id);
    click("card a");
    expect(log.length).toBeGreaterThan(0);
    for (const animation of log) {
      expect([...props(animation)].filter((key) => key !== "opacity")).toEqual([]);
    }
  });

  it("single column: no horizontal spread", () => {
    narrow = true;
    open();
    expect(log.some((animation) => animation.target.classList.contains("panel-reveal__x"))).toBe(false);
    expect(log.some((animation) => animation.target.classList.contains("panel-reveal__y"))).toBe(true);
  });

  it("slides the notch to a newly picked card without replaying the panel, and retargets from where it is", () => {
    open("a");
    finishAll();
    act(() => { vi.advanceTimersByTime(500); });
    const before = log.length;
    click("card c"); // swap while open
    expect(root().getAttribute("data-phase")).toBe("open");
    expect(log.length).toBe(before + 1);
    const first = slides()[0]!;
    expect(first.target).toBe(notch());
    expect(first.options.easing).toBe("cubic-bezier(.4,0,.2,1)");
    expect(first.frames[0]!.transform).toBe("translateX(82px) rotate(45deg)"); // from card a (190 - 100 - 8)
    expect(first.frames[1]!.transform).toBe("translateX(902px) rotate(45deg)"); // to card c (1010 - 100 - 8)
    expect(first.frames[0]!.borderTopColor).toBeDefined();
    expect([...props(first)].sort()).toEqual(["borderLeftColor", "borderTopColor", "transform"]);
    // pick card b while the slide is in flight: it starts from the notch's current position (computed x = 321), no jump
    click("card b");
    expect(first.cancelled).toBe(true);
    const second = slides()[1]!;
    expect(second.frames[0]!.transform).toBe(`translateX(321px) rotate(45deg)`);
    expect(second.frames[1]!.transform).toBe("translateX(542px) rotate(45deg)"); // card b: 650 - 100 - 8
  });

  it("a re-measure that gives the same answer (panel height changed after a swap) does not cut a slide short", () => {
    open("a");
    finishAll();
    act(() => { vi.advanceTimersByTime(500); });
    click("card c");
    const slide = slides()[0]!;
    expect(slide.cancelled).toBe(false);
    act(() => { window.dispatchEvent(new Event("resize")); });
    expect(slide.cancelled).toBe(false);
    expect(slides()).toHaveLength(1);
  });

  it("never slides the notch while closing", () => {
    open();
    finishAll();
    click("card a"); // close
    act(() => { window.dispatchEvent(new Event("resize")); });
    expect(slides()).toHaveLength(0);
  });

  it("the notch follows the card centre after a resize (a lone resize glides, a burst snaps)", () => {
    open("a");
    finishAll();
    act(() => { vi.advanceTimersByTime(1000); });
    rects.set(document.querySelector('[data-card="a"]')!, box(140, 142, 180, 92)); // card moved 40 px right
    vi.spyOn(performance, "now").mockReturnValue(10_000);
    act(() => { window.dispatchEvent(new Event("resize")); });
    expect(notch().style.getPropertyValue("--notch-x")).toBe(`${230 - 100 - 8}px`);
    expect(slides()).toHaveLength(1);
    rects.set(document.querySelector('[data-card="a"]')!, box(150, 142, 180, 92));
    vi.spyOn(performance, "now").mockReturnValue(10_050); // within 150 ms of the last event: a drag, snap
    act(() => { window.dispatchEvent(new Event("resize")); });
    expect(notch().style.getPropertyValue("--notch-x")).toBe(`${240 - 100 - 8}px`);
    expect(slides()).toHaveLength(1);
  });
});

describe("genieDurationMs", () => {
  afterEach(() => { window.localStorage.removeItem("spGenieMs"); });
  it("defaults, multiplies for small values, takes ms for large ones and ignores nonsense", () => {
    expect(genieDurationMs(500)).toBe(500);
    window.localStorage.setItem("spGenieMs", "6");
    expect(genieDurationMs(500)).toBe(3000);
    window.localStorage.setItem("spGenieMs", "3000");
    expect(genieDurationMs(500)).toBe(3000);
    window.localStorage.setItem("spGenieMs", "abc");
    expect(genieDurationMs(500)).toBe(500);
    window.localStorage.setItem("spGenieMs", "0");
    expect(genieDurationMs(500)).toBe(500);
  });
});
