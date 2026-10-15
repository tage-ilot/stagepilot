import type { GridItemHTMLElement, GridStack } from "gridstack";

import type { DashboardItemId } from "./dashboardLayoutTypes";

export const CELL_HEIGHT = 28;
const GRID_ITEM_VERTICAL_INSET = 10;

export const contentRows = (
  target: HTMLElement,
  constraints: { minH?: number; maxH?: number },
) => {
  const previousHeight = target.style.height;
  const previousMaxHeight = target.style.maxHeight;
  const previousOverflow = target.style.overflow;
  target.style.height = "auto";
  target.style.maxHeight = "none";
  target.style.overflow = "visible";

  let naturalHeight = target.scrollHeight;
  for (const child of target.querySelectorAll<HTMLElement>(
    ".overflow-auto, [data-autosize-content]",
  )) {
    naturalHeight += Math.max(0, child.scrollHeight - child.clientHeight);
  }

  target.style.height = previousHeight;
  target.style.maxHeight = previousMaxHeight;
  target.style.overflow = previousOverflow;

  const rows = Math.max(
    constraints.minH ?? 1,
    Math.ceil((naturalHeight + GRID_ITEM_VERTICAL_INSET) / CELL_HEIGHT),
  );
  return constraints.maxH ? Math.min(rows, constraints.maxH) : rows;
};

// Content-fit rules (owner, 2026-10-14). Their height follows their content, which depends on the width they are given:
//  - "exact": Now playing and Manual controls are exactly as tall as their content, so a drag can neither cut anything off nor
//    leave empty space. Dragging the handle up or down does nothing; widening or narrowing changes the height by itself.
//  - "min": Service plan can be made taller (spare room under the list is fine) but never shorter than its content.
// Recent Events scrolls inside its own frame, so it stays free-form.
export const CONTENT_FIT: Partial<Record<DashboardItemId, "exact" | "min">> = {
  "now-playing": "exact",
  "manual-controls": "exact",
  "service-plan": "min",
};

/** Applies the content-fit rule to one grid item. Returns true when the grid changed. */
export const fitWidgetToContent = (grid: GridStack, element: GridItemHTMLElement): boolean => {
  const id = element.getAttribute("gs-id") as DashboardItemId | null;
  const rule = id ? CONTENT_FIT[id] : undefined;
  const node = element.gridstackNode;
  const target = element.querySelector<HTMLElement>(".widget-autosize-target");
  if (!rule || !node || !target) return false;
  const rows = contentRows(target, {});
  if (rule === "exact") {
    if (node.h === rows && node.minH === rows && node.maxH === rows) return false;
    grid.update(element, { h: rows, minH: rows, maxH: rows });
    return true;
  }
  if (node.minH === rows && (node.h ?? 0) >= rows) return false;
  grid.update(element, { minH: rows, h: Math.max(node.h ?? rows, rows) });
  return true;
};
