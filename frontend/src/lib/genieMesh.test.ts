import { describe, expect, it } from "vitest";

import {
  GENIE_K, MESH_COLS, MESH_ROWS, buildMesh, easeInCubic, easeOutCubic, genieProgress, layoutBottom, rowQ, rowTransform,
  type CardTarget, type PanelBox,
} from "./genieMesh";

const panel: PanelBox = { width: 1000, height: 500 };
const card: CardTarget = { cx: 300, y: -14, width: 180 };
const rows = Array.from({ length: 65 }, (_, i) => i / 64);

describe("rowTransform", () => {
  it("p = 0 is the identity: every row sits at its own place, full width, centred", () => {
    for (const v of rows) {
      const row = rowTransform(0, v, panel, card);
      expect(row.y).toBeCloseTo(v * panel.height, 9);
      expect(row.cx).toBeCloseTo(500, 9);
      expect(row.width).toBeCloseTo(1000, 9);
    }
  });

  it("p = 1 collapses every row onto the card's bottom edge, centre and width", () => {
    for (const v of rows) {
      const row = rowTransform(1, v, panel, card);
      expect(row.y).toBeCloseTo(card.y, 9);
      expect(row.cx).toBeCloseTo(card.cx, 9);
      expect(row.width).toBeCloseTo(card.width, 9);
    }
  });

  it("rows nearest the card (top) reach the card width first", () => {
    for (const p of [0.1, 0.3, 0.5, 0.7]) {
      const widths = rows.map((v) => rowTransform(p, v, panel, card).width);
      for (let i = 1; i < widths.length; i += 1) expect(widths[i]!).toBeGreaterThanOrEqual(widths[i - 1]! - 1e-9);
    }
    // the top row is already collapsed at p = 1 / (1 + k), the bottom row is not
    const p = 1 / (1 + GENIE_K);
    expect(rowTransform(p, 0, panel, card).width).toBeCloseTo(card.width, 6);
    expect(rowTransform(p, 1, panel, card).width).toBeGreaterThan(card.width + 1);
  });

  it("keeps row order monotonic (no row crosses the one below it) for every p", () => {
    for (let step = 0; step <= 100; step += 1) {
      const ys = rows.map((v) => rowTransform(step / 100, v, panel, card).y);
      for (let i = 1; i < ys.length; i += 1) expect(ys[i]!).toBeGreaterThanOrEqual(ys[i - 1]! - 1e-9);
    }
  });

  it("moves each row's centre monotonically from the panel centre to the card centre", () => {
    for (const v of [0, 0.5, 1]) {
      let previous = 500;
      for (let step = 0; step <= 100; step += 1) {
        const { cx } = rowTransform(step / 100, v, panel, card);
        expect(cx).toBeLessThanOrEqual(previous + 1e-9);
        previous = cx;
      }
      expect(previous).toBeCloseTo(card.cx, 9);
    }
  });

  it("works for the first card (left), the last card (right) and a card wider than the panel", () => {
    const first: CardTarget = { cx: 90, y: -10, width: 170 };
    const last: CardTarget = { cx: 910, y: -10, width: 170 };
    const wide: CardTarget = { cx: 500, y: -10, width: 1400 };
    for (const target of [first, last, wide]) {
      const end = rowTransform(1, 0.5, panel, target);
      expect(end.cx).toBeCloseTo(target.cx, 9);
      expect(end.width).toBeCloseTo(target.width, 9);
      for (let step = 0; step <= 20; step += 1) {
        const row = rowTransform(step / 20, 0.3, panel, target);
        expect(Number.isFinite(row.y + row.cx + row.width)).toBe(true);
      }
    }
    // a card wider than the panel widens the rows instead of narrowing them
    expect(rowTransform(0.5, 0, panel, wide).width).toBeGreaterThan(panel.width);
  });
});

describe("rowQ / layout", () => {
  it("clamps and lags by k * v", () => {
    expect(rowQ(0, 0)).toBe(0);
    expect(rowQ(1, 1)).toBeCloseTo(1, 9);
    expect(rowQ(0.2, 1)).toBe(0);
    expect(rowQ(0.5, 0)).toBeCloseTo(0.8, 9);
  });

  it("the content below follows the lowest row: full height open, 0 collapsed, never negative", () => {
    expect(layoutBottom(0, panel, card)).toBeCloseTo(panel.height, 9);
    expect(layoutBottom(1, panel, card)).toBe(0);
    let previous = Infinity;
    for (let step = 0; step <= 100; step += 1) {
      const bottom = layoutBottom(step / 100, panel, card);
      expect(bottom).toBeLessThanOrEqual(previous + 1e-9);
      expect(bottom).toBeGreaterThanOrEqual(0);
      previous = bottom;
    }
  });
});

describe("genieProgress", () => {
  it("opening eases out from the start, closing eases in; both hit their ends", () => {
    expect(genieProgress(0, 1, 0)).toBe(1);
    expect(genieProgress(1, 1, 0)).toBe(0);
    expect(genieProgress(0, 0, 1)).toBe(0);
    expect(genieProgress(1, 0, 1)).toBe(1);
    expect(genieProgress(0.5, 0, 1)).toBeCloseTo(easeInCubic(0.5), 9);
    expect(genieProgress(0.5, 1, 0)).toBeCloseTo(1 - easeOutCubic(0.5), 9);
  });

  it("a reversed animation continues from the current progress", () => {
    expect(genieProgress(0, 0.37, 0)).toBeCloseTo(0.37, 9);
    expect(genieProgress(0, 0.37, 1)).toBeCloseTo(0.37, 9);
    expect(genieProgress(1, 0.37, 0)).toBeCloseTo(0, 9);
  });
});

describe("buildMesh", () => {
  it("builds rows x cols quads as two triangles each", () => {
    const mesh = buildMesh(MESH_ROWS, MESH_COLS);
    expect(mesh.triangles).toBe(MESH_ROWS * MESH_COLS * 2);
    expect(mesh.uv.length).toBe((MESH_ROWS + 1) * (MESH_COLS + 1) * 2);
    expect(mesh.indices.length).toBe(mesh.triangles * 3);
    expect(Math.max(...mesh.indices)).toBe((MESH_ROWS + 1) * (MESH_COLS + 1) - 1);
  });
});

// ------------------------------------------------------------------ column count study (design item 3)

const SIZE = 64;

/** Deterministic panel-like texture: dark ground, pale text-like bars, a bright border. */
function texture(): Uint8ClampedArray {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4);
  let seed = 7;
  const random = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 2 ** 32; };
  for (let i = 0; i < SIZE * SIZE; i += 1) data.set([16, 22, 32, 255], i * 4);
  for (let bar = 0; bar < 40; bar += 1) {
    const x0 = Math.floor(random() * 52); const y0 = 3 + Math.floor(random() * 56);
    const w = 4 + Math.floor(random() * 14); const h = 1 + Math.floor(random() * 3);
    const shade = 110 + Math.floor(random() * 140);
    for (let y = y0; y < Math.min(SIZE, y0 + h); y += 1) for (let x = x0; x < Math.min(SIZE, x0 + w); x += 1) data.set([shade, shade, shade, 255], (y * SIZE + x) * 4);
  }
  for (let i = 0; i < SIZE; i += 1) for (const [x, y] of [[i, 0], [i, SIZE - 1], [0, i], [SIZE - 1, i]] as const) data.set([224, 90, 138, 255], (y * SIZE + x) * 4);
  return data;
}

function sample(tex: Uint8ClampedArray, u: number, v: number): number[] {
  const x = Math.min(SIZE - 1, Math.max(0, u * SIZE - 0.5)); const y = Math.min(SIZE - 1, Math.max(0, v * SIZE - 0.5));
  const x0 = Math.floor(x); const y0 = Math.floor(y); const x1 = Math.min(SIZE - 1, x0 + 1); const y1 = Math.min(SIZE - 1, y0 + 1);
  const fx = x - x0; const fy = y - y0;
  return [0, 1, 2, 3].map((c) => {
    const at = (px: number, py: number) => tex[(py * SIZE + px) * 4 + c]!;
    return (at(x0, y0) * (1 - fx) + at(x1, y0) * fx) * (1 - fy) + (at(x0, y1) * (1 - fx) + at(x1, y1) * fx) * fy;
  });
}

/** CPU triangle rasteriser with WebGL's affine (w = 1) varying interpolation; sampled at pixel centres. */
function render(p: number, cols: number, tex: Uint8ClampedArray): Uint8ClampedArray {
  const out = new Uint8ClampedArray(SIZE * SIZE * 4);
  const box: PanelBox = { width: SIZE, height: SIZE };
  const target: CardTarget = { cx: 44, y: -4, width: 12 };
  const vertex = (i: number, j: number) => {
    const u = i / cols; const v = j / MESH_ROWS;
    const row = rowTransform(p, v, box, target);
    return { x: row.cx + (u - 0.5) * row.width, y: row.y, u, v };
  };
  const fill = (a: ReturnType<typeof vertex>, b: typeof a, c: typeof a) => {
    const area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    if (Math.abs(area) < 1e-9) return;
    const minX = Math.max(0, Math.floor(Math.min(a.x, b.x, c.x))); const maxX = Math.min(SIZE - 1, Math.ceil(Math.max(a.x, b.x, c.x)));
    const minY = Math.max(0, Math.floor(Math.min(a.y, b.y, c.y))); const maxY = Math.min(SIZE - 1, Math.ceil(Math.max(a.y, b.y, c.y)));
    for (let y = minY; y <= maxY; y += 1) {
      for (let x = minX; x <= maxX; x += 1) {
        const px = x + 0.5; const py = y + 0.5;
        const w0 = ((b.x - px) * (c.y - py) - (c.x - px) * (b.y - py)) / area;
        const w1 = ((c.x - px) * (a.y - py) - (a.x - px) * (c.y - py)) / area;
        const w2 = 1 - w0 - w1;
        if (w0 < 0 || w1 < 0 || w2 < 0) continue;
        const rgba = sample(tex, w0 * a.u + w1 * b.u + w2 * c.u, w0 * a.v + w1 * b.v + w2 * c.v);
        out.set(rgba, (y * SIZE + x) * 4);
      }
    }
  };
  for (let j = 0; j < MESH_ROWS; j += 1) {
    for (let i = 0; i < cols; i += 1) {
      const a = vertex(i, j); const b = vertex(i + 1, j); const c = vertex(i, j + 1); const d = vertex(i + 1, j + 1);
      fill(a, b, c);
      fill(b, d, c);
    }
  }
  return out;
}

function differingShare(a: Uint8ClampedArray, b: Uint8ClampedArray): number {
  let count = 0;
  for (let i = 0; i < SIZE * SIZE; i += 1) {
    if ([0, 1, 2, 3].some((c) => Math.abs(a[i * 4 + c]! - b[i * 4 + c]!) > 24)) count += 1;
  }
  return count / (SIZE * SIZE);
}

/** Share of pixels differing by more than 24/255 from the 64x64 reference mesh, worst of p = 0.2, 0.5, 0.8. */
export function meshColumnsStudy(): Record<number, number> {
  const tex = texture();
  const result: Record<number, number> = {};
  for (const cols of [1, 2, 4]) {
    result[cols] = Math.max(...[0.2, 0.5, 0.8].map((p) => differingShare(render(p, cols, tex), render(p, 64, tex))));
  }
  return result;
}

describe("mesh column count (64x64 reference)", () => {
  it("MESH_COLS is the smallest of {1, 2, 4} with at most 1% of pixels differing by more than 24/255", () => {
    const study = meshColumnsStudy();
    console.info("meshColumnsStudy (worst share of differing pixels at p = .2/.5/.8):", JSON.stringify(study));
    const smallest = [1, 2, 4].find((cols) => study[cols]! <= 0.01);
    expect(smallest).toBeDefined();
    expect(MESH_COLS).toBe(smallest);
  });
});
