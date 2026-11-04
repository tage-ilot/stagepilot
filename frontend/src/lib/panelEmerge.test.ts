import { describe, expect, it } from "vitest";

import {
  EMERGE_MS,
  NOTCH_SIZE,
  emergeFrames,
  emergeGeometry,
  flipOffset,
  notchCentre,
  panelEdges,
  parseTranslateX,
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
    expect(NOTCH_SIZE).toBe(16);
  });
});

const LAYOUT_PROPS = ["height", "width", "top", "left", "right", "bottom", "margin", "marginTop", "clipPath", "clip-path", "filter", "gridTemplateRows", "padding"];
const ALLOWED = new Set(["transform", "opacity", "offset", "easing", "composite", "borderTopColor", "borderLeftColor"]);

describe("emergeFrames", () => {
  const g = emergeGeometry(CARDS[0], panel)!;
  const all = (direction: "open" | "close") => {
    const f = emergeFrames(g, direction);
    return [f.outer, f.inner, f.below, f.content, f.notch];
  };

  it("animate only transform and opacity (layout properties can never creep back)", () => {
    for (const direction of ["open", "close"] as const) {
      for (const frames of all(direction)) {
        for (const frame of frames) {
          for (const key of Object.keys(frame)) {
            expect(LAYOUT_PROPS).not.toContain(key);
            expect(["transform", "opacity", "offset"]).toContain(key);
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

  it("fades the notch in from 70% on open and out within the first 30% of a close", () => {
    expect(emergeFrames(g, "open").notch).toEqual([{ opacity: 0, offset: 0 }, { opacity: 0, offset: 0.7 }, { opacity: 1, offset: 1 }]);
    expect(emergeFrames(g, "close").notch).toEqual([{ opacity: 1, offset: 0 }, { opacity: 0, offset: 0.3 }, { opacity: 0, offset: 1 }]);
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
  it("moves the notch with transform only (plus the accent colour)", () => {
    const frames = slideFrames(100, 400, "rgb(1, 2, 3)", "rgb(4, 5, 6)");
    expect(frames).toHaveLength(2);
    expect(frames[0]!.transform).toBe(`translateX(${100 - NOTCH_SIZE / 2}px) rotate(45deg)`);
    expect(frames[1]!.transform).toBe(`translateX(${400 - NOTCH_SIZE / 2}px) rotate(45deg)`);
    for (const frame of frames) for (const key of Object.keys(frame)) expect(ALLOWED.has(key)).toBe(true);
    expect(frames[0]!.borderTopColor).toBe("rgb(1, 2, 3)");
    expect(frames[1]!.borderLeftColor).toBe("rgb(4, 5, 6)");
  });
});

describe("parseTranslateX", () => {
  it("reads the x translation out of a rotated matrix and tolerates none", () => {
    expect(parseTranslateX("matrix(0.707107, 0.707107, -0.707107, 0.707107, 312.5, 0)")).toBeCloseTo(312.5, 6);
    expect(parseTranslateX("none")).toBeNull();
    expect(parseTranslateX("")).toBeNull();
  });
});
