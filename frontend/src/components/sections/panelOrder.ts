/**
 * The order of a configuration page, top to bottom, written ONCE (DESIGN.md §6.6). `PanelLayout` draws the header, then `alerts`, then the
 * body in the order of `BODY_SLOTS`; the page tests read the same list, so a reordered page fails.
 */
export const BODY_SLOTS = ["connection", "settings", "actions", "optional", "content", "notices", "saveBar", "summary"] as const;
export type BodySlot = (typeof BODY_SLOTS)[number];
export const PANEL_ORDER = ["header", "alerts", ...BODY_SLOTS] as const;

/** The body's slots in the order they are drawn: `summaryAt="top"` moves the summary to the front (Backend's Status cards), nothing else. */
export const bodyOrder = (summaryAt: "top" | "bottom" = "bottom"): readonly BodySlot[] => (summaryAt === "top" ? ["summary", ...BODY_SLOTS.filter((slot) => slot !== "summary")] : BODY_SLOTS);
