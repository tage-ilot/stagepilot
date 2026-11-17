/**
 * `next` with every part that equals the same part of `prev` replaced by that very object. A reload of
 * the settings then changes only what really changed: whatever is edited on an unchanged song (rows
 * being typed, an open row, the selection) keeps its object, so nothing downstream sees a new value.
 */
export function keepUnchanged<T>(prev: unknown, next: T): T {
  if (prev === next) return next;
  if (typeof prev !== "object" || typeof next !== "object" || prev === null || next === null) return next;
  if (Array.isArray(prev) !== Array.isArray(next)) return next;
  if (Array.isArray(next)) {
    const before = prev as unknown[];
    let same = before.length === next.length;
    const merged = next.map((item, index) => {
      const kept = keepUnchanged(before[index], item);
      if (kept !== before[index]) same = false;
      return kept;
    });
    return (same ? before : merged) as T;
  }
  const before = prev as Record<string, unknown>;
  const after = next as Record<string, unknown>;
  const keys = Object.keys(after);
  let same = keys.length === Object.keys(before).length;
  const merged: Record<string, unknown> = {};
  for (const key of keys) {
    const kept = keepUnchanged(before[key], after[key]);
    merged[key] = kept;
    if (!(key in before) || kept !== before[key]) same = false;
  }
  return (same ? before : merged) as T;
}
