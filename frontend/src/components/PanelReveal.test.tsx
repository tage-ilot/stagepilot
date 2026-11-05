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
const slides = () => log.filter((animation) => animation.options.duration === 280 && animation.target === notch());
const slideCounters = () => log.filter((animation) => animation.options.duration === 280 && animation.target === document.querySelector(".panel-reveal__notch-cx"));
const riseOf = () => log.filter((animation) => animation.target === document.querySelector(".panel-reveal__notch-rise"));
const riseCounterOf = () => log.filter((animation) => animation.target === document.querySelector(".panel-reveal__notch-cy"));

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
const clipOf = () => document.querySelector<HTMLElement>(".panel-reveal__notch-clip")!;
const trackOf = () => document.querySelector<HTMLElement>(".panel-reveal__notch-track")!;
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
      if (element.tagName === "SECTION") return { ...real, borderTopColor: "rgb(213, 51, 103)", marginTop: "20px", marginLeft: "0px", borderTopWidth: "1px", borderRightWidth: "1px", borderBottomWidth: "1px", borderLeftWidth: "1px", backgroundColor: "rgba(15, 23, 32, 0.94)", backgroundImage: "linear-gradient(120deg, rgba(213, 51, 103, 0.13) 0%, rgba(0, 0, 0, 0) 36%)", position: "static" } as CSSStyleDeclaration;
      if (element.classList?.contains("panel-reveal__notch")) {
        const x = slides().length ? 321 : 0;
        return { ...real, transform: x ? `matrix(1, 0, 0, 1, ${x}, 0)` : "none", color: "rgb(100, 100, 100)" } as CSSStyleDeclaration;
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

  it("opens with transform on the wrappers, the content below and the notch, opacity on the content, nothing else", () => {
    open("a");
    const targets = log.map((animation) => animation.target);
    expect(targets).toContain(document.querySelector(".panel-reveal__x"));
    expect(targets).toContain(document.querySelector(".panel-reveal__y"));
    expect(targets).toContain(document.querySelector(".panel-reveal__notch-rise"));
    expect(targets).toContain(document.querySelector(".panel-reveal__notch-cy"));
    for (const animation of log) {
      const keys = props(animation);
      expect([...keys].filter((key) => key !== "offset" && key !== "transform" && key !== "opacity" && key !== "easing")).toEqual([]);
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
      // the root starts at x=100, the notch is 28 px wide: translate = centre - 100 - 14
      expect(notch().style.getPropertyValue("--notch-x")).toBe(`${centre - 100 - 14}px`);
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

  it("closes from a settled open: onto the card, accelerating, ending at scaleY(0) and opacity 0", () => {
    open();
    finishAll();
    expect(live()).toHaveLength(0);
    click("card a");
    const x = log.filter((animation) => animation.target.classList.contains("panel-reveal__x") && props(animation).has("transform")).at(-1)!;
    expect(x.frames[0]!.transform).toBe("translateX(0px) scaleX(1)");
    expect(x.frames[1]!.transform).toBe("translateX(-410px) scaleX(0.18)");
    const content = log.filter((animation) => animation.target.closest("section")).at(-1)!;
    expect(content.frames).toEqual([{ opacity: 1, offset: 0 }, { opacity: 0, offset: 0.4 }, { opacity: 0, offset: 1 }]);
  });

  it("a close ends at scaleY(0), the card's width and opacity 0: only transform and opacity animate, one easing for the strip and the content below", () => {
    open();
    finishAll();
    click("card a");
    const closing = live();
    const y = closing.find((animation) => animation.target.classList.contains("panel-reveal__y"))!;
    expect(y.frames.at(-1)!.transform).toBe("translateY(-20px) scaleY(0)");
    expect(y.options.easing).toBe("cubic-bezier(.5,0,.75,.4)");
    const below = closing.find((animation) => animation.target === screen.getByTestId("below"))!;
    expect(below.frames.at(-1)!.transform).toBe("translateY(-620px)"); // the whole panel height (600, from layout) + the gap (20)
    expect(below.options.easing).toBe(y.options.easing);
    const group = closing.find((animation) => animation.target.classList.contains("panel-reveal__x") && props(animation).has("opacity"))!;
    expect(group.frames.at(-1)).toMatchObject({ opacity: 0, offset: 1 });
    expect(group.options.easing).toBe("linear");
    for (const animation of closing) {
      for (const key of props(animation)) expect(["transform", "opacity", "offset", "easing"]).toContain(key);
    }
  });

  it("onCloseEnd fires when the close animation finishes (no timer in between), not for an open and not for an interrupted close", () => {
    const ended = vi.fn();
    function Wrapper() {
      const [on, setOn] = useState(true);
      return (
        <>
          <button data-card="a" aria-controls="a-configuration" onClick={() => setOn(false)}>card a</button>
          <PanelReveal phase={on ? "opening" : "closing"} durationMs={500} trigger={() => document.querySelector<HTMLElement>("[aria-controls='a-configuration']")} onCloseEnd={ended}>
            <section id="a-configuration"><h2>panel a</h2></section>
          </PanelReveal>
        </>
      );
    }
    render(<Wrapper />);
    placeCard("a");
    finishAll();
    expect(ended).not.toHaveBeenCalled(); // an open finishing
    click("card a");
    expect(ended).not.toHaveBeenCalled();
    finishAll();
    expect(ended).toHaveBeenCalledTimes(1);
  });

  it("leaves no animations and no will-change at rest after an open", () => {
    open();
    expect(document.querySelector<HTMLElement>(".panel-reveal__x")!.style.willChange).toBe("transform, opacity");
    finishAll();
    expect(log.every((animation) => animation.cancelled)).toBe(true);
    for (const element of document.querySelectorAll<HTMLElement>("*")) expect(element.style.willChange).toBe("");
  });

  it("the notch sits in a static, pointer-events-free clip; only its wrappers animate, the clip itself never does", () => {
    open();
    const clip = clipOf();
    expect(trackOf().getAttribute("aria-hidden")).toBe("true");
    expect(trackOf().parentElement).toBe(root());
    expect(clip.parentElement).toBe(trackOf());
    // clip > rise > notch > fill > cx > cy > bg: the nesting the CSS clip and the counter-translation rely on
    const chain = [".panel-reveal__notch-rise", ".panel-reveal__notch", ".panel-reveal__notch-fill", ".panel-reveal__notch-cx", ".panel-reveal__notch-cy", ".panel-reveal__notch-bg"];
    let parent: Element = clip;
    for (const selector of chain) {
      const child = parent.querySelector(`:scope > ${selector}`);
      expect(child, selector).not.toBeNull();
      parent = child!;
    }
    expect(log.some((animation) => animation.target === clip)).toBe(false);
    // the track (not the clip) follows the panel's top edge: the same translateY and easing as the inner wrapper
    const trackAnimation = log.find((animation) => animation.target === trackOf())!;
    const inner = log.find((animation) => animation.target.classList.contains("panel-reveal__y"))!;
    expect(trackAnimation.frames).toEqual([{ transform: "translateY(-20px)" }, { transform: "translateY(0px)" }]);
    expect(trackAnimation.options.easing).toBe(inner.options.easing);
    expect(trackAnimation.options.duration).toBe(inner.options.duration);
  });

  it("the notch rises on open: translateY(+h) -> 0 on the rise wrapper from 60%, with the fill counter-translated identically", () => {
    open("b"); // the middle card: the strip covers the notch early, so the rise starts at 60%
    expect(riseOf()).toHaveLength(1);
    const rise = riseOf()[0]!;
    expect(rise.options.duration).toBe(500);
    expect(rise.options.fill).toBe("both");
    expect(rise.frames).toEqual([
      { transform: "translateY(14px)", offset: 0 },
      { transform: "translateY(14px)", offset: 0.6, easing: "cubic-bezier(.2,.8,.2,1)" },
      { transform: "translateY(0px)", offset: 1 },
    ]);
    expect([...props(rise)].sort()).toEqual(["easing", "offset", "transform"]); // no opacity: the clip does the revealing
    const counter = riseCounterOf()[0]!;
    expect(counter.frames.map((frame) => frame.transform)).toEqual(["translateY(-14px)", "translateY(-14px)", "translateY(0px)"]);
    expect(counter.frames.map((frame) => frame.offset)).toEqual(rise.frames.map((frame) => frame.offset));
    expect(counter.frames.map((frame) => frame.easing)).toEqual(rise.frames.map((frame) => frame.easing));
    // the notch's own x is not animated by the rise
    expect(log.filter((animation) => animation.target === notch())).toHaveLength(0);
  });

  it("an edge card's rise starts a little later, once the strip is wide enough to carry the notch", () => {
    open("a");
    const start = riseOf()[0]!.frames[1]!.offset!;
    expect(start).toBeGreaterThan(0.6);
    expect(start).toBeLessThanOrEqual(0.8);
    expect(riseCounterOf()[0]!.frames[1]!.offset).toBe(start);
  });

  it("the notch sinks early in a close, before the panel shrinks away", () => {
    open();
    finishAll();
    click("card a");
    const sink = riseOf().at(-1)!;
    expect(sink.frames).toEqual([
      { transform: "translateY(0px)", offset: 0, easing: "cubic-bezier(.8,0,.8,.2)" },
      { transform: "translateY(14px)", offset: 0.35 },
      { transform: "translateY(14px)", offset: 1 },
    ]);
    const x = log.filter((animation) => animation.target.classList.contains("panel-reveal__x")).at(-1)!;
    expect(x.options.duration).toBe(sink.options.duration); // same group: the sink is finished (offset .35) while the panel is still shrinking
  });

  it("a reversal mid-flight reverses the rise and its counter together (no new animation, no jump)", () => {
    open();
    act(() => { vi.advanceTimersByTime(350); });
    const rise = riseOf()[0]!;
    const counter = riseCounterOf()[0]!;
    click("card a"); // close while the notch is rising
    expect(rise.reversed).toBe(1);
    expect(counter.reversed).toBe(1);
    expect(riseOf()).toHaveLength(1);
    click("card a"); // and open again
    expect(rise.reversed).toBe(2);
    expect(counter.reversed).toBe(2);
    expect(riseOf()).toHaveLength(1);
    expect(rise.cancelled).toBe(false);
  });

  it("a swap while open neither replays the rise nor touches the group; the notch only glides (also while risen)", () => {
    open("a");
    finishAll();
    act(() => { vi.advanceTimersByTime(500); });
    expect(live()).toHaveLength(0); // risen: no animation left running
    const risesBefore = riseOf().length;
    click("card b");
    click("card c");
    expect(riseOf()).toHaveLength(risesBefore);
    expect(riseCounterOf()).toHaveLength(risesBefore);
    expect(slides()).toHaveLength(2);
    expect(log.filter((animation) => animation.target.classList.contains("panel-reveal__x"))).toHaveLength(2); // transform + group opacity, from the one open
  });

  it("the notch's fill is a copy of the panel's background, continued 12 px above it", () => {
    open();
    const bg = document.querySelector<HTMLElement>(".panel-reveal__notch-bg")!;
    expect(bg.style.getPropertyValue("background-color")).toBe("rgba(15, 23, 32, 0.94)");
    expect(bg.style.getPropertyValue("background-image")).toContain("linear-gradient(120deg");
    expect(bg.style.getPropertyValue("left")).toBe("0px");
    expect(bg.style.getPropertyValue("border-width")).toBe("13px 1px 1px"); // 1px panel border + 12px reach above the panel (the shorthand collapses equal sides)
  });

  it("a child of the panel replaced during the open hands its fade to the new child, in step with the others", async () => {
    vi.useRealTimers();
    open();
    const panel = document.querySelector("section")!;
    const first = panel.firstElementChild ?? panel.appendChild(document.createElement("div"));
    const fadesBefore = log.filter((animation) => animation.target === first);
    expect(fadesBefore).toHaveLength(1);
    const reference = log.find((animation) => animation.target.classList.contains("panel-reveal__x"))!;
    (reference as unknown as { currentTime: number; playbackRate: number }).currentTime = 180;
    (reference as unknown as { currentTime: number; playbackRate: number }).playbackRate = 1;
    const replacement = document.createElement("div");
    act(() => { first.replaceWith(replacement); });
    await act(async () => { await Promise.resolve(); });
    expect(fadesBefore[0]!.cancelled).toBe(true); // the stale fade stops ticking
    const fresh = log.filter((animation) => animation.target === replacement);
    expect(fresh).toHaveLength(1);
    expect(fresh[0]!.frames).toEqual(fadesBefore[0]!.frames);
    expect((fresh[0] as unknown as { currentTime: number }).currentTime).toBe(180);
    expect(replacement.style.willChange).toBe("opacity");
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
    // no travel: nothing translates the notch, the whole notch only fades with the panel
    expect(riseOf()).toHaveLength(0);
    expect(riseCounterOf()).toHaveLength(0);
    expect(log.some((animation) => animation.target === clipOf())).toBe(true);
  });

  it("single column: no horizontal spread", () => {
    narrow = true;
    open();
    expect(log.some((animation) => animation.target.classList.contains("panel-reveal__x") && props(animation).has("transform"))).toBe(false);
    expect(log.some((animation) => animation.target.classList.contains("panel-reveal__y"))).toBe(true);
  });

  it("slides the notch to a newly picked card without replaying the panel, and retargets from where it is", () => {
    open("a");
    finishAll();
    act(() => { vi.advanceTimersByTime(500); });
    const before = log.length;
    click("card c"); // swap while open
    expect(root().getAttribute("data-phase")).toBe("open");
    expect(log.length).toBe(before + 2); // the notch and its fill's counter-translation; nothing else (no rise, no panel)
    expect(riseOf()).toHaveLength(1); // the one from the first open
    const first = slides()[0]!;
    expect(first.target).toBe(notch());
    expect(first.options.easing).toBe("cubic-bezier(.4,0,.2,1)");
    expect(first.frames[0]!.transform).toBe("translateX(76px)"); // from card a (190 - 100 - 14)
    expect(first.frames[1]!.transform).toBe("translateX(896px)"); // to card c (1010 - 100 - 14)
    expect(first.frames[0]!.color).toBeDefined();
    expect([...props(first)].sort()).toEqual(["color", "transform"]);
    // the panel-coloured fill is counter-translated with the same timing, so it stays lined up with the panel
    const counter = slideCounters()[0]!;
    expect(counter.options.duration).toBe(280);
    expect(counter.options.easing).toBe("cubic-bezier(.4,0,.2,1)");
    expect(counter.frames.map((frame) => frame.transform)).toEqual(["translateX(-76px)", "translateX(-896px)"]);
    // pick card b while the slide is in flight: it starts from the notch's current position (computed x = 321), no jump
    click("card b");
    expect(first.cancelled).toBe(true);
    const second = slides()[1]!;
    expect(second.frames[0]!.transform).toBe("translateX(321px)"); // exactly where the notch is now (computed x), no jump
    expect(second.frames[1]!.transform).toBe("translateX(536px)"); // card b: 650 - 100 - 14
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
    expect(notch().style.getPropertyValue("--notch-x")).toBe(`${230 - 100 - 14}px`);
    expect(slides()).toHaveLength(1);
    rects.set(document.querySelector('[data-card="a"]')!, box(150, 142, 180, 92));
    vi.spyOn(performance, "now").mockReturnValue(10_050); // within 150 ms of the last event: a drag, snap
    act(() => { window.dispatchEvent(new Event("resize")); });
    expect(notch().style.getPropertyValue("--notch-x")).toBe(`${240 - 100 - 14}px`);
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
