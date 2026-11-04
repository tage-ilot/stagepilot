/**
 * Geometry and keyframes for the config-panel "emerge" animation (DESIGN.md "Config panel reveal").
 *
 * The panel is laid out at its final size straight away. Only compositor properties animate (transform, opacity):
 *  - outer wrapper: translateX(dx) scaleX(sx) -> identity, so the strip starts exactly the card's width, centred on the card;
 *  - inner wrapper: translateY(-gap) scaleY(0.02) -> identity, so it starts on the card's bottom edge and grows down;
 *  - everything below the panel: translateY(-(0.98 H + gap)) -> 0 (FLIP), so the gap under the panel never changes;
 *  - the panel's content fades in late (so scaled text is never visible), the notch fades in last.
 * Closing is the same frames reversed (the content/notch fades have their own early-fade timings).
 */

/** Open / close duration in ms. The slow-motion hook (genieDuration.ts) stretches it. */
export const EMERGE_MS = 500;
/** Reduced motion: a plain fade. */
export const REDUCED_MS = 120;
/** The notch glides to a newly picked card (or re-measured position) in this time. */
export const SLIDE_MS = 280;
export const SLIDE_EASING = "cubic-bezier(.4,0,.2,1)";
/** Side of the notch's square (rotated 45 degrees). */
export const NOTCH_SIZE = 16;

export const SX_FLOOR = 0.06;
export const SY_START = 0.02;
const EASE_OUTER = "cubic-bezier(.65,0,.35,1)";
const EASE_INNER = "cubic-bezier(.4,0,.2,1)";

export interface CardBox { left: number; width: number; bottom: number }
export interface PanelBox { left: number; width: number; top: number; height: number }

export interface EmergeGeometry {
  /** Horizontal distance from the panel's centre to the card's centre. */
  dx: number;
  /** Starting horizontal scale: card width / panel width (floored). */
  sx: number;
  /** Distance from the card's bottom edge to the panel's top, read from layout. Never negative. */
  gap: number;
  /** The panel's measured height. */
  height: number;
}

export function emergeGeometry(card: CardBox, panel: PanelBox): EmergeGeometry | null {
  if (!(panel.width > 0) || !(panel.height > 0)) return null;
  return {
    dx: card.left + card.width / 2 - (panel.left + panel.width / 2),
    sx: Math.max(SX_FLOOR, card.width / panel.width),
    gap: Math.max(0, panel.top - card.bottom),
    height: panel.height,
  };
}

/** Where the notch sits: the card's centre, relative to the panel's left edge. */
export function notchCentre(card: CardBox, panel: Pick<PanelBox, "left">): number {
  return card.left + card.width / 2 - panel.left;
}

/** Width of the strip at horizontal progress p (0 = card width, 1 = panel width). */
export function stripWidth(g: EmergeGeometry, panelWidth: number, p: number): number {
  return panelWidth * (g.sx + (1 - g.sx) * p);
}

/** Vertical offset of the content below the panel at vertical progress p (0 = just opened, 1 = at rest). */
export function flipOffset(g: EmergeGeometry, p: number): number {
  return -(1 - SY_START) * g.height * (1 - p) - g.gap * (1 - p);
}

/** Top and bottom edge of the animated panel at vertical progress p, given its resting top. */
export function panelEdges(g: EmergeGeometry, restTop: number, p: number): { top: number; bottom: number } {
  const top = restTop - g.gap * (1 - p);
  return { top, bottom: top + g.height * (SY_START + (1 - SY_START) * p) };
}

export type EmergeDirection = "open" | "close";

export interface EmergeFrames {
  outer: Keyframe[];
  inner: Keyframe[];
  below: Keyframe[];
  content: Keyframe[];
  notch: Keyframe[];
  easing: { outer: string; inner: string; below: string; content: string; notch: string };
}

export function emergeFrames(g: EmergeGeometry, direction: EmergeDirection): EmergeFrames {
  const opening = direction === "open";
  const pair = (from: Keyframe, to: Keyframe): Keyframe[] => (opening ? [from, to] : [to, from]);
  return {
    outer: pair({ transform: `translateX(${g.dx}px) scaleX(${g.sx})` }, { transform: "translateX(0px) scaleX(1)" }),
    inner: pair({ transform: `translateY(${-g.gap}px) scaleY(${SY_START})` }, { transform: "translateY(0px) scaleY(1)" }),
    below: pair({ transform: `translateY(${-((1 - SY_START) * g.height + g.gap)}px)` }, { transform: "translateY(0px)" }),
    content: opening
      ? [{ opacity: 0, offset: 0 }, { opacity: 0, offset: 0.45 }, { opacity: 1, offset: 1 }]
      : [{ opacity: 1, offset: 0 }, { opacity: 0, offset: 0.4 }, { opacity: 0, offset: 1 }],
    notch: opening
      ? [{ opacity: 0, offset: 0 }, { opacity: 0, offset: 0.7 }, { opacity: 1, offset: 1 }]
      : [{ opacity: 1, offset: 0 }, { opacity: 0, offset: 0.3 }, { opacity: 0, offset: 1 }],
    easing: { outer: EASE_OUTER, inner: EASE_INNER, below: EASE_INNER, content: "linear", notch: "linear" },
  };
}

/** Fade only (reduced motion): no travel, no scale, no notch motion. */
export function fadeFrames(direction: EmergeDirection): Keyframe[] {
  return direction === "open" ? [{ opacity: 0 }, { opacity: 1 }] : [{ opacity: 1 }, { opacity: 0 }];
}

/** The notch glides along the panel's top edge; its accent colour cross-fades over the same time. */
export function slideFrames(fromCentre: number, toCentre: number, fromColour: string, toColour: string): Keyframe[] {
  const at = (centre: number) => `translateX(${centre - NOTCH_SIZE / 2}px) rotate(45deg)`;
  return [
    { transform: at(fromCentre), borderTopColor: fromColour, borderLeftColor: fromColour },
    { transform: at(toCentre), borderTopColor: toColour, borderLeftColor: toColour },
  ];
}

/** x translation of a computed `matrix(a, b, c, d, e, f)` (translate-then-rotate keeps e = the translation). Null when none. */
export function parseTranslateX(transform: string): number | null {
  const match = /^matrix\(([^)]+)\)$/.exec(transform.trim());
  if (!match) return null;
  const x = Number(match[1]!.split(",")[4]);
  return Number.isFinite(x) ? x : null;
}
