import { describe, expect, it, vi } from "vitest";
import type { GridItemHTMLElement, GridStack } from "gridstack";

import { CONTENT_FIT, fitWidgetToContent } from "./contentFit";

// A grid item whose widget needs `natural` pixels of height (28px rows, 10px inset: rows = ceil((natural + 10) / 28)).
function widget(id: string, node: { h: number; minH?: number; maxH?: number }, natural: number) {
  const element = document.createElement("div") as unknown as GridItemHTMLElement;
  element.setAttribute("gs-id", id);
  const target = document.createElement("div");
  target.className = "widget-autosize-target";
  Object.defineProperty(target, "scrollHeight", { configurable: true, value: natural });
  element.appendChild(target);
  (element as { gridstackNode?: unknown }).gridstackNode = node;
  const update = vi.fn((_el: unknown, opts: { h?: number; minH?: number; maxH?: number }) => Object.assign(node, opts));
  return { element, update, grid: { update } as unknown as GridStack, node };
}

describe("dashboard content-fit widgets (owner 2026-10-14)", () => {
  it("Now playing and Manual controls are exact-fit, Service plan can only grow, Recent Events is free", () => {
    expect(CONTENT_FIT["now-playing"]).toBe("exact");
    expect(CONTENT_FIT["manual-controls"]).toBe("exact");
    expect(CONTENT_FIT["service-plan"]).toBe("min");
    expect(CONTENT_FIT.events).toBeUndefined();
  });

  it("an exact-fit widget is locked to the rows its content needs, so a drag can neither clip it nor leave empty space", () => {
    const w = widget("now-playing", { h: 18, minH: 11 }, 347); // 347px needs ceil(357 / 28) = 13 rows
    expect(fitWidgetToContent(w.grid, w.element)).toBe(true);
    expect(w.node).toMatchObject({ h: 13, minH: 13, maxH: 13 });
    expect(fitWidgetToContent(w.grid, w.element)).toBe(false); // already fits: no churn
  });

  it("shrinks one that was dragged too short (content cut off) back up to its content", () => {
    const w = widget("manual-controls", { h: 5, minH: 5 }, 161); // ceil(171 / 28) = 7 rows
    fitWidgetToContent(w.grid, w.element);
    expect(w.node).toMatchObject({ h: 7, minH: 7, maxH: 7 });
  });

  it("a min-fit widget keeps any extra height the user gave it but never goes below its content", () => {
    const tall = widget("service-plan", { h: 20, minH: 9 }, 312); // needs 12
    fitWidgetToContent(tall.grid, tall.element);
    expect(tall.node).toMatchObject({ h: 20, minH: 12 });
    expect(tall.node.maxH).toBeUndefined();
    const short = widget("service-plan", { h: 9, minH: 9 }, 312);
    fitWidgetToContent(short.grid, short.element);
    expect(short.node).toMatchObject({ h: 12, minH: 12 });
  });

  it("leaves Recent Events and spacers alone", () => {
    const events = widget("events", { h: 12, minH: 7 }, 900);
    expect(fitWidgetToContent(events.grid, events.element)).toBe(false);
    expect(events.update).not.toHaveBeenCalled();
  });
});
