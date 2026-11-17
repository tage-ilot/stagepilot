import { describe, expect, it } from "vitest";

import { keepUnchanged } from "./keepUnchanged";

describe("keepUnchanged", () => {
  it("returns the previous object for every part that is equal, and the new one for what differs", () => {
    const prev = { a: { rows: [{ id: 1 }, { id: 2 }] }, b: { rows: [{ id: 3 }] }, n: 1 };
    const next = { a: { rows: [{ id: 1 }, { id: 2 }] }, b: { rows: [{ id: 3 }, { id: 4 }] }, n: 1 };
    const merged = keepUnchanged(prev, next);
    expect(merged).toEqual(next);
    expect(merged.a).toBe(prev.a);
    expect(merged.b).not.toBe(prev.b);
    expect(merged.b.rows[0]).toBe(prev.b.rows[0]);
  });

  it("returns the previous value itself when nothing differs, and handles first load, null and type changes", () => {
    const prev = { a: [1, 2], b: null as null | { x: number } };
    expect(keepUnchanged(prev, { a: [1, 2], b: null })).toBe(prev);
    expect(keepUnchanged(null, { a: 1 })).toEqual({ a: 1 });
    expect(keepUnchanged({ a: [1] }, { a: { 0: 1 } })).toEqual({ a: { 0: 1 } });
    expect(keepUnchanged({ a: 1, b: 2 }, { a: 1 })).toEqual({ a: 1 });
    expect(keepUnchanged({ a: 1 }, { a: 1, c: 3 })).toEqual({ a: 1, c: 3 });
  });
});
