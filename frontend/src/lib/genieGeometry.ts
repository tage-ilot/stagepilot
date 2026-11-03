export interface GenieGeometry {
  /** Card centre, % of the panel row. */
  x: number;
  /** Card width, % of the panel row. */
  w: number;
  /** Card left / right edge, % of the panel row (clamped to 0..100). */
  l: number;
  r: number;
  /** Pixels between the card's bottom edge and the (untransformed) top of the panel's first content; never negative. */
  gap: number;
}

const pct = (value: number) => Math.round(value * 1000) / 1000;

/** Card rect -> where the funnel starts, relative to the panel row. Null when the row has no width. */
export function genieGeometry(
  card: { left: number; width: number; bottom: number },
  host: { left: number; width: number; /** top of the visible panel content */ top: number },
): GenieGeometry | null {
  if (!(host.width > 0)) return null;
  const clamp = (value: number) => Math.min(100, Math.max(0, value));
  const l = clamp(((card.left - host.left) / host.width) * 100);
  const r = clamp(((card.left + card.width - host.left) / host.width) * 100);
  return { x: pct((l + r) / 2), w: pct(r - l), l: pct(l), r: pct(r), gap: Math.max(0, pct(host.top - card.bottom)) };
}
