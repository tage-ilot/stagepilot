/**
 * Pure math for the genie mesh (no DOM, no WebGL): where each row of the panel snapshot sits for a given progress.
 * The same formulas run in the vertex shader (see genieEngine.ts); this copy is what the unit tests pin down and what
 * drives the layout of the content below the panel.
 *
 * Progress `p`: 0 = fully open (identity), 1 = collapsed onto the card. Coordinates are CSS px in the panel's own space
 * (x from the panel's left edge, y from the panel's top edge; the card's bottom edge is usually a little above y=0).
 */

/** Row lag: rows nearest the card (v = 0) move first. */
export const GENIE_K = 0.6;
export const MESH_ROWS = 64;
/**
 * Columns per row. Chosen by `meshColumnsStudy` (genieMesh.test.ts): the smallest of {1, 2, 4} whose render differs from
 * a 64x64 reference in at most 1% of the pixels (by more than 24/255) at p = 0.2, 0.5 and 0.8. See PR notes for numbers.
 */
export const MESH_COLS = 1;

export interface PanelBox { width: number; height: number }
/** The card the panel pours into: centre x, bottom edge y and width, in the panel's space. */
export interface CardTarget { cx: number; y: number; width: number }
export interface RowTransform { y: number; cx: number; width: number }

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));
export const smoothstep = (q: number) => q * q * (3 - 2 * q);

/** Per-row progress: the row at height `v` (0 = top, nearest the card, 1 = bottom) lags by `k * v`. */
export function rowQ(p: number, v: number, k = GENIE_K): number {
  return clamp01(p * (1 + k) - v * k);
}

/** Where row `v` (0..1) of the snapshot is drawn at progress `p`. */
export function rowTransform(p: number, v: number, panel: PanelBox, card: CardTarget, k = GENIE_K): RowTransform {
  const qe = smoothstep(rowQ(p, v, k));
  const panelCx = panel.width / 2;
  return {
    y: v * panel.height + (card.y - v * panel.height) * qe,
    cx: panelCx + (card.cx - panelCx) * Math.pow(qe, 1.05),
    width: panel.width + (card.width - panel.width) * Math.pow(qe, 0.45),
  };
}

/** y of the lowest row: the content below the panel follows it (never above the panel's own top, y = 0). */
export function layoutBottom(p: number, panel: PanelBox, card: CardTarget, k = GENIE_K): number {
  return Math.max(0, rowTransform(p, 1, panel, card, k).y);
}

/** Open eases out, close eases in (cubic), as in the prototype. `t` is the fraction of the (remaining) duration. */
export const easeOutCubic = (t: number) => 1 - Math.pow(1 - clamp01(t), 3);
export const easeInCubic = (t: number) => Math.pow(clamp01(t), 3);

/** Progress at time fraction `t` when moving from `from` to `to`; opening (to < from) eases out, closing eases in. */
export function genieProgress(t: number, from: number, to: number): number {
  const eased = to < from ? easeOutCubic(t) : easeInCubic(t);
  return from + (to - from) * eased;
}

export interface MeshGeometry {
  /** (u, v) per vertex, row-major, (cols + 1) x (rows + 1). */
  uv: Float32Array;
  indices: Uint16Array;
  triangles: number;
}

export function buildMesh(rows = MESH_ROWS, cols = MESH_COLS): MeshGeometry {
  const uv = new Float32Array((rows + 1) * (cols + 1) * 2);
  let n = 0;
  for (let j = 0; j <= rows; j += 1) for (let i = 0; i <= cols; i += 1) { uv[n++] = i / cols; uv[n++] = j / rows; }
  const indices = new Uint16Array(rows * cols * 6);
  n = 0;
  for (let j = 0; j < rows; j += 1) {
    for (let i = 0; i < cols; i += 1) {
      const a = j * (cols + 1) + i;
      const b = a + 1;
      const c = a + cols + 1;
      const d = c + 1;
      indices.set([a, b, c, b, d, c], n);
      n += 6;
    }
  }
  return { uv, indices, triangles: rows * cols * 2 };
}
