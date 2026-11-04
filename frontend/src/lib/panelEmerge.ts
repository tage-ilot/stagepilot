/**
 * Geometry and keyframes for the config-panel "emerge" animation (DESIGN.md "Config panel reveal").
 *
 * The panel is laid out at its final size straight away. Only compositor properties animate (transform, opacity):
 *  - outer wrapper: translateX(dx) scaleX(sx) -> identity, so the strip starts exactly the card's width, centred on the card;
 *  - inner wrapper: translateY(-gap) scaleY(0.02) -> identity, so it starts on the card's bottom edge and grows down;
 *  - everything below the panel: translateY(-(0.98 H + gap)) -> 0 (FLIP), so the gap under the panel never changes;
 *  - the panel's content fades in late (so scaled text is never visible), the notch rises out of the panel's top border last
 *    (translateY inside a static clip that only shows the strip above the border, so no part of it is ever drawn over the panel).
 * Closing is the same frames reversed (the content fade and the notch's sink have their own early timings).
 */

/** Open / close duration in ms. The slow-motion hook (genieDuration.ts) stretches it. */
export const EMERGE_MS = 500;
/** Reduced motion: a plain fade. */
export const REDUCED_MS = 120;
/** The notch glides to a newly picked card (or re-measured position) in this time. */
export const SLIDE_MS = 280;
export const SLIDE_EASING = "cubic-bezier(.4,0,.2,1)";
/** Width of the notch's box. Its shape is a 45 degree triangle: half the width is its height. */
export const NOTCH_WIDTH = 28;
/** The notch's visible height: the strip above the panel's top border plus the two rows below its top edge that the notch's fill covers (the border row and the panel's inset highlight under it). */
export const NOTCH_HEIGHT = 14;
/** How far the notch's visible part reaches above the panel's top border (its fill continues the panel's background this far up). */
export const NOTCH_ABOVE = 12;
/** The notch travels this far down to be fully hidden behind the border, and back up to rest. */
export const NOTCH_RISE = NOTCH_HEIGHT;
/** The rise starts at this offset of an open (or later, when the strip does not cover the notch yet), so it is finished when the panel is. */
export const RISE_FROM = 0.6;
/** ...but never later than this, so the rise always has time to travel. */
export const RISE_LATEST = 0.8;
/** The strip's rounded end is this far from its edge before it is under the whole notch. */
const STRIP_CORNER = 12;
/** The sink is finished at this offset of a close, before the panel shrinks away. */
export const SINK_TO = 0.35;
export const RISE_EASING = "cubic-bezier(.2,.8,.2,1)";
/** The rise curve run backwards in time. */
export const SINK_EASING = "cubic-bezier(.8,0,.8,.2)";

export const SX_FLOOR = 0.06;
export const SY_START = 0.02;
const EASE_OUTER = "cubic-bezier(.65,0,.35,1)";
const BEZIER_OUTER = [0.65, 0, 0.35, 1] as const;
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
  /** The panel's measured width. */
  width: number;
}

export function emergeGeometry(card: CardBox, panel: PanelBox): EmergeGeometry | null {
  if (!(panel.width > 0) || !(panel.height > 0)) return null;
  return {
    dx: card.left + card.width / 2 - (panel.left + panel.width / 2),
    sx: Math.max(SX_FLOOR, card.width / panel.width),
    gap: Math.max(0, panel.top - card.bottom),
    height: panel.height,
    width: panel.width,
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
  /** The notch's clip strip follows the panel's top edge while the gap to the card closes (the same translateY as the inner wrapper). */
  track: Keyframe[];
  /** The notch's rise (open) or sink (close): translateY on the wrapper inside the static clip. */
  notch: Keyframe[];
  /** The exact opposite translation, for the notch's panel-coloured fill, which must stay put while the notch moves. */
  notchCounter: Keyframe[];
  easing: { outer: string; inner: string; below: string; content: string; notch: string };
}

/** Time (0..1) at which a cubic-bezier(x1, y1, x2, y2) timing function has reached `progress` (y). */
export function bezierTimeAt(progress: number, [x1, y1, x2, y2]: readonly [number, number, number, number]): number {
  const axis = (a: number, b: number, u: number) => 3 * a * u * (1 - u) ** 2 + 3 * b * u * u * (1 - u) + u ** 3;
  let low = 0;
  let high = 1;
  for (let step = 0; step < 40; step += 1) {
    const mid = (low + high) / 2;
    if (axis(y1, y2, mid) < progress) low = mid; else high = mid;
  }
  return axis(x1, x2, (low + high) / 2);
}

/**
 * Offset of an open at which the notch may start to rise: RISE_FROM, or later for a card near the panel's edge, when the
 * (still narrow) strip does not cover the notch yet: the notch must not climb out of thin air beside the strip.
 */
export function riseStart(g: EmergeGeometry, spread = true): number {
  if (!spread || !(g.width > 0)) return RISE_FROM;
  const covered = (2 * (Math.abs(g.dx) + NOTCH_WIDTH / 2 + STRIP_CORNER)) / g.width; // strip width, as a fraction of the panel
  const horizontal = (covered - g.sx) / (1 - g.sx);
  if (!(horizontal > 0) || !Number.isFinite(horizontal)) return RISE_FROM;
  return Math.min(RISE_LATEST, Math.max(RISE_FROM, bezierTimeAt(Math.min(1, horizontal), BEZIER_OUTER)));
}

/** The notch's travel. `sign` -1 gives the counter-translation of the fill (same offsets and easings, so the two always cancel). */
export function riseFrames(direction: EmergeDirection, sign: 1 | -1, from = RISE_FROM): Keyframe[] {
  const down = `translateY(${sign * NOTCH_RISE}px)`;
  const up = "translateY(0px)";
  return direction === "open"
    ? [{ transform: down, offset: 0 }, { transform: down, offset: from, easing: RISE_EASING }, { transform: up, offset: 1 }]
    : [{ transform: up, offset: 0, easing: SINK_EASING }, { transform: down, offset: SINK_TO }, { transform: down, offset: 1 }];
}

export function emergeFrames(g: EmergeGeometry, direction: EmergeDirection, spread = true): EmergeFrames {
  const opening = direction === "open";
  const pair = (from: Keyframe, to: Keyframe): Keyframe[] => (opening ? [from, to] : [to, from]);
  return {
    outer: pair({ transform: `translateX(${g.dx}px) scaleX(${g.sx})` }, { transform: "translateX(0px) scaleX(1)" }),
    inner: pair({ transform: `translateY(${-g.gap}px) scaleY(${SY_START})` }, { transform: "translateY(0px) scaleY(1)" }),
    below: pair({ transform: `translateY(${-((1 - SY_START) * g.height + g.gap)}px)` }, { transform: "translateY(0px)" }),
    content: opening
      ? [{ opacity: 0, offset: 0 }, { opacity: 0, offset: 0.45 }, { opacity: 1, offset: 1 }]
      : [{ opacity: 1, offset: 0 }, { opacity: 0, offset: 0.4 }, { opacity: 0, offset: 1 }],
    track: pair({ transform: `translateY(${-g.gap}px)` }, { transform: "translateY(0px)" }),
    notch: riseFrames(direction, 1, riseStart(g, spread)),
    notchCounter: riseFrames(direction, -1, riseStart(g, spread)),
    easing: { outer: EASE_OUTER, inner: EASE_INNER, below: EASE_INNER, content: "linear", notch: "linear" },
  };
}

/** Fade only (reduced motion): no travel, no scale, no notch motion. */
export function fadeFrames(direction: EmergeDirection): Keyframe[] {
  return direction === "open" ? [{ opacity: 0 }, { opacity: 1 }] : [{ opacity: 1 }, { opacity: 0 }];
}

/**
 * The notch glides along the panel's top edge; its accent colour cross-fades over the same time. `counter` is the opposite
 * translation of the notch's panel-coloured fill (same timing), which keeps the fill's gradient lined up with the panel.
 */
export function slideFrames(fromCentre: number, toCentre: number, fromColour: string, toColour: string): { notch: Keyframe[]; counter: Keyframe[] } {
  const at = (centre: number) => centre - NOTCH_WIDTH / 2;
  return {
    notch: [
      { transform: `translateX(${at(fromCentre)}px)`, color: fromColour },
      { transform: `translateX(${at(toCentre)}px)`, color: toColour },
    ],
    counter: [{ transform: `translateX(${-at(fromCentre)}px)` }, { transform: `translateX(${-at(toCentre)}px)` }],
  };
}

/** x translation of a computed `matrix(a, b, c, d, e, f)` (a pure translation keeps e = the translation). Null when none. */
export function parseTranslateX(transform: string): number | null {
  const match = /^matrix\(([^)]+)\)$/.exec(transform.trim());
  if (!match) return null;
  const x = Number(match[1]!.split(",")[4]);
  return Number.isFinite(x) ? x : null;
}
