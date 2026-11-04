import { describe, expect, it } from "vitest";

import {
  EMERGE_MS,
  NOTCH_ABOVE,
  NOTCH_HEIGHT,
  NOTCH_RISE,
  NOTCH_WIDTH,
  RISE_EASING,
  RISE_FROM,
  SINK_EASING,
  RISE_LATEST,
  SINK_TO,
  bezierTimeAt,
  emergeFrames,
  emergeGeometry,
  flipOffset,
  notchCentre,
  panelEdges,
  parseTranslateX,
  riseFrames,
  riseStart,
  slideFrames,
  stripWidth,
  type CardBox,
} from "./panelEmerge";

// A 1376 px wide panel starting at x=32; five cards of 265.6 px across the same row (the real demo layout).
const panel = { left: 32, width: 1376, top: 254, height: 600 };
const cardAt = (index: number, width = 265.6): CardBox => ({ left: 32 + index * (width + 12), width, bottom: 234 });
const CARDS = [0, 1, 2, 3, 4].map((i) => cardAt(i)) as [CardBox, CardBox, CardBox, CardBox, CardBox];

describe("emergeGeometry", () => {
  it("starts the strip exactly the card's width", () => {
    for (const card of CARDS) {
      const g = emergeGeometry(card, panel)!;
      expect(stripWidth(g, panel.width, 0)).toBeCloseTo(card.width, 6);
      expect(stripWidth(g, panel.width, 1)).toBeCloseTo(panel.width, 6);
    }
  });

  it("centres the strip on the first, middle and last card", () => {
    for (const card of [CARDS[0], CARDS[2], CARDS[4]]) {
      const g = emergeGeometry(card, panel)!;
      const stripCentre = panel.left + panel.width / 2 + g.dx; // outer origin is 50% 0, so scaleX keeps the centre
      expect(stripCentre).toBeCloseTo(card.left + card.width / 2, 6);
    }
  });

  it("handles a card wider than the panel (dx still centres; sx grows past 1)", () => {
    const narrow = { left: 100, width: 300, top: 254, height: 200 };
    const card = { left: 60, width: 400, bottom: 234 };
    const g = emergeGeometry(card, narrow)!;
    expect(g.sx).toBeCloseTo(400 / 300, 6);
    expect(narrow.left + narrow.width / 2 + g.dx).toBeCloseTo(card.left + card.width / 2, 6);
  });

  it("floors the horizontal scale at 0.06", () => {
    expect(emergeGeometry({ left: 0, width: 2, bottom: 10 }, panel)!.sx).toBe(0.06);
  });

  it("reads the gap from layout (card bottom to panel top), never negative, never hard-coded", () => {
    expect(emergeGeometry(CARDS[0], panel)!.gap).toBe(20);
    expect(emergeGeometry(CARDS[0], { ...panel, top: 250 })!.gap).toBe(16);
    expect(emergeGeometry({ ...CARDS[0], bottom: 300 }, panel)!.gap).toBe(0);
  });

  it("returns null for a panel without size", () => {
    expect(emergeGeometry(CARDS[0], { ...panel, width: 0 })).toBeNull();
    expect(emergeGeometry(CARDS[0], { ...panel, height: 0 })).toBeNull();
  });

  it("keeps the gap below the panel constant while it opens (FLIP) at t = 0, .25, .5, .75, 1", () => {
    const g = emergeGeometry(CARDS[3], panel)!;
    const restGap = 20; // distance between the panel's bottom and the next block when at rest
    const restBelowTop = panel.top + panel.height + restGap;
    for (const t of [0, 0.25, 0.5, 0.75, 1]) {
      const edges = panelEdges(g, panel.top, t);
      const belowTop = restBelowTop + flipOffset(g, t);
      expect(belowTop - edges.bottom).toBeCloseTo(restGap, 6);
    }
    expect(panelEdges(g, panel.top, 0).top).toBeCloseTo(CARDS[3].bottom, 6); // touches the card's bottom edge
    expect(flipOffset(g, 1)).toBeCloseTo(0, 6);
    expect(flipOffset(g, 0)).toBeCloseTo(-(0.98 * panel.height + g.gap), 6);
  });
});

describe("notchCentre", () => {
  it("is the card centre relative to the panel's left edge, for the first, middle and last card", () => {
    expect(notchCentre(CARDS[0], panel)).toBeCloseTo(CARDS[0].width / 2, 6);
    expect(notchCentre(CARDS[2], panel)).toBeCloseTo(2 * (265.6 + 12) + 265.6 / 2, 6);
    expect(notchCentre(CARDS[4], panel)).toBeCloseTo(4 * (265.6 + 12) + 265.6 / 2, 6);
    expect(NOTCH_WIDTH).toBe(28);
  });
});

const LAYOUT_PROPS = ["height", "width", "top", "left", "right", "bottom", "margin", "marginTop", "clipPath", "clip-path", "filter", "gridTemplateRows", "padding"];
// The only properties anything here may animate: transform and opacity, plus the accent colour of a notch glide (paint only).
const ALLOWED = new Set(["transform", "opacity", "offset", "easing", "composite", "color"]);

describe("emergeFrames", () => {
  const g = emergeGeometry(CARDS[0], panel)!;
  const all = (direction: "open" | "close") => {
    const f = emergeFrames(g, direction);
    return [f.outer, f.inner, f.below, f.content, f.notch, f.notchCounter, f.track];
  };

  it("animate only transform and opacity (layout properties can never creep back)", () => {
    for (const direction of ["open", "close"] as const) {
      for (const frames of all(direction)) {
        for (const frame of frames) {
          for (const key of Object.keys(frame)) {
            expect(LAYOUT_PROPS).not.toContain(key);
            expect(["transform", "opacity", "offset", "easing"]).toContain(key);
          }
        }
      }
    }
    const f = emergeFrames(g, "open");
    expect(f.outer.every((frame) => Object.keys(frame).every((k) => k === "transform"))).toBe(true);
    expect(f.content.every((frame) => Object.keys(frame).every((k) => ["opacity", "offset"].includes(k)))).toBe(true);
  });

  it("outer = translateX/scaleX, inner = translateY/scaleY, below = translateY, with the approved easings", () => {
    const f = emergeFrames(g, "open");
    expect(f.outer[0]!.transform).toBe(`translateX(${g.dx}px) scaleX(${g.sx})`);
    expect(f.outer[1]!.transform).toBe("translateX(0px) scaleX(1)");
    expect(f.inner[0]!.transform).toBe(`translateY(${-g.gap}px) scaleY(0.02)`);
    expect(f.below[0]!.transform).toBe(`translateY(${-(0.98 * g.height + g.gap)}px)`);
    expect(f.easing.outer).toBe("cubic-bezier(.65,0,.35,1)");
    expect(f.easing.inner).toBe("cubic-bezier(.4,0,.2,1)");
  });

  it("fades the content in over the last 55% of an open and out over the first 40% of a close", () => {
    const open = emergeFrames(g, "open").content;
    expect(open).toEqual([{ opacity: 0, offset: 0 }, { opacity: 0, offset: 0.45 }, { opacity: 1, offset: 1 }]);
    const close = emergeFrames(g, "close").content;
    expect(close).toEqual([{ opacity: 1, offset: 0 }, { opacity: 0, offset: 0.4 }, { opacity: 0, offset: 1 }]);
  });

  it("the notch rises from behind the border (translateY +h -> 0) from 60% of an open, with the approved easing, and only transform animates", () => {
    const middle = emergeGeometry(CARDS[2], panel)!;
    const open = emergeFrames(middle, "open");
    expect(NOTCH_RISE).toBe(NOTCH_HEIGHT);
    expect(open.notch).toEqual([
      { transform: `translateY(${NOTCH_RISE}px)`, offset: 0 },
      { transform: `translateY(${NOTCH_RISE}px)`, offset: RISE_FROM, easing: "cubic-bezier(.2,.8,.2,1)" },
      { transform: "translateY(0px)", offset: 1 },
    ]);
    expect(RISE_FROM).toBe(0.6);
    expect(RISE_EASING).toBe("cubic-bezier(.2,.8,.2,1)");
    for (const frame of open.notch) expect(Object.keys(frame).filter((key) => !["transform", "offset", "easing"].includes(key))).toEqual([]);
    // never faded: opacity stays 1 for the whole travel, the clip does the revealing
    expect(open.notch.some((frame) => "opacity" in frame)).toBe(false);
  });

  it("starts the rise at 60% for cards near the middle and a little later for edge cards, once the strip covers the notch", () => {
    expect(riseStart(emergeGeometry(CARDS[2], panel)!)).toBe(RISE_FROM);
    for (const index of [0, 4]) {
      const g = emergeGeometry(CARDS[index]!, panel)!;
      const start = riseStart(g);
      expect(start).toBeGreaterThan(RISE_FROM);
      expect(start).toBeLessThanOrEqual(RISE_LATEST);
      // at that moment the strip (scaleX eased by cubic-bezier(.65,0,.35,1)) reaches past the notch plus the strip's rounded end
      const progress = ((): number => {
        let low = 0; let high = 1;
        for (let step = 0; step < 50; step += 1) { const mid = (low + high) / 2; if (bezierTimeAt(mid, [0.65, 0, 0.35, 1]) < start) low = mid; else high = mid; }
        return (low + high) / 2;
      })();
      const half = stripWidth(g, panel.width, progress) / 2;
      expect(half).toBeGreaterThanOrEqual(Math.abs(g.dx) + NOTCH_WIDTH / 2 + 12 - 0.5);
      expect(emergeFrames(g, "open").notch[1]!.offset).toBe(start);
      expect(emergeFrames(g, "open").notchCounter[1]!.offset).toBe(start);
    }
    // single column: no spread, so nothing to wait for
    expect(riseStart(emergeGeometry(CARDS[0], panel)!, false)).toBe(RISE_FROM);
  });

  it("bezierTimeAt inverts the timing function (monotonic, 0 -> 0, 1 -> 1, the midpoint of a symmetric curve is 0.5)", () => {
    expect(bezierTimeAt(0, [0.65, 0, 0.35, 1])).toBeCloseTo(0, 6);
    expect(bezierTimeAt(1, [0.65, 0, 0.35, 1])).toBeCloseTo(1, 6);
    expect(bezierTimeAt(0.5, [0.65, 0, 0.35, 1])).toBeCloseTo(0.5, 4);
    expect(bezierTimeAt(0.9, [0.65, 0, 0.35, 1])).toBeGreaterThan(bezierTimeAt(0.1, [0.65, 0, 0.35, 1]));
  });

  it("the notch's clip follows the panel's top edge with the inner wrapper's translateY and easing, so the gap closing never exposes the notch over the panel", () => {
    const open = emergeFrames(g, "open");
    expect(open.track).toEqual([{ transform: `translateY(${-g.gap}px)` }, { transform: "translateY(0px)" }]);
    expect(open.track[0]!.transform).toBe(`translateY(${-g.gap}px)`);
    expect(open.inner[0]!.transform).toBe(`translateY(${-g.gap}px) scaleY(0.02)`);
    expect(emergeFrames(g, "close").track).toEqual([...open.track].reverse());
  });

  it("the notch sinks back down within the first 35% of a close (the rise curve run backwards)", () => {
    const close = emergeFrames(g, "close");
    expect(close.notch).toEqual([
      { transform: "translateY(0px)", offset: 0, easing: SINK_EASING },
      { transform: `translateY(${NOTCH_RISE}px)`, offset: SINK_TO },
      { transform: `translateY(${NOTCH_RISE}px)`, offset: 1 },
    ]);
    expect(SINK_TO).toBeLessThanOrEqual(0.35);
    expect(SINK_EASING).toBe("cubic-bezier(.8,0,.8,.2)"); // 1-x2, 1-y2, 1-x1, 1-y1 of the rise curve
  });

  it("the fill's counter-translation has the same offsets and easings and the exact opposite offsets in y, so the two always cancel", () => {
    for (const direction of ["open", "close"] as const) {
      const frames = emergeFrames(g, direction);
      expect(frames.notchCounter).toHaveLength(frames.notch.length);
      frames.notch.forEach((frame, index) => {
        const counter = frames.notchCounter[index]!;
        expect(counter.offset).toBe(frame.offset);
        expect(counter.easing).toBe(frame.easing);
        const y = Number.parseFloat(String(frame.transform).replace(/[^\d.-]/g, ""));
        const yCounter = Number.parseFloat(String(counter.transform).replace(/[^\d.-]/g, ""));
        expect(yCounter + y).toBeCloseTo(0, 9);
      });
    }
    expect(riseFrames("open", -1)[0]!.transform).toBe(`translateY(${-NOTCH_RISE}px)`);
  });

  it("the clip strip reaches from the notch's tip to just under the border row (what the CSS clip shows)", () => {
    expect(NOTCH_HEIGHT).toBe(14);
    expect(NOTCH_ABOVE).toBe(12); // the border's top edge is 12px below the clip's top: the border row is 12..13, the highlight under it 13..14
    expect(NOTCH_RISE).toBeGreaterThanOrEqual(NOTCH_HEIGHT); // fully hidden behind the strip at the start
  });

  it("closing is the exact reverse of the transform frames", () => {
    const open = emergeFrames(g, "open");
    const close = emergeFrames(g, "close");
    expect(close.outer).toEqual([...open.outer].reverse());
    expect(close.inner).toEqual([...open.inner].reverse());
    expect(close.below).toEqual([...open.below].reverse());
  });

  it("opens in 500 ms", () => {
    expect(EMERGE_MS).toBe(500);
  });
});

describe("slideFrames", () => {
  it("moves the notch with transform only (plus the accent colour) and the fill's counter-translation the other way", () => {
    const frames = slideFrames(100, 400, "rgb(1, 2, 3)", "rgb(4, 5, 6)");
    expect(frames.notch).toHaveLength(2);
    expect(frames.notch[0]!.transform).toBe(`translateX(${100 - NOTCH_WIDTH / 2}px)`);
    expect(frames.notch[1]!.transform).toBe(`translateX(${400 - NOTCH_WIDTH / 2}px)`);
    for (const frame of [...frames.notch, ...frames.counter]) for (const key of Object.keys(frame)) expect(ALLOWED.has(key)).toBe(true);
    expect(frames.notch[0]!.color).toBe("rgb(1, 2, 3)");
    expect(frames.notch[1]!.color).toBe("rgb(4, 5, 6)");
    expect(frames.counter[0]!.transform).toBe(`translateX(${-(100 - NOTCH_WIDTH / 2)}px)`);
    expect(frames.counter[1]!.transform).toBe(`translateX(${-(400 - NOTCH_WIDTH / 2)}px)`);
    expect(frames.counter.every((frame) => !("color" in frame))).toBe(true);
  });
});

describe("parseTranslateX", () => {
  it("reads the x translation out of a matrix and tolerates none", () => {
    expect(parseTranslateX("matrix(1, 0, 0, 1, 312.5, 0)")).toBeCloseTo(312.5, 6);
    expect(parseTranslateX("none")).toBeNull();
    expect(parseTranslateX("")).toBeNull();
  });
});
