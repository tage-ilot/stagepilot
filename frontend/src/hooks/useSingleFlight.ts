import { useMemo, useRef } from "react";

/**
 * Synchronous in-flight guard. React state (`pending`) updates on the next render, so two clicks
 * in the same tick both see "not pending". A ref is updated immediately: the second click of a
 * double-click, an Enter repeat or a click while pending finds the key taken and does nothing.
 * `claim` returns false when the key is already in flight; call `release` in `finally`.
 */
export function useSingleFlight() {
  const taken = useRef(new Set<string>());
  return useMemo(() => ({
    claim(key: string): boolean {
      if (taken.current.has(key)) return false;
      taken.current.add(key);
      return true;
    },
    release(key: string): void {
      taken.current.delete(key);
    },
  }), []);
}
