import { useEffect, useReducer } from "react";

const MINUTE_MS = 60_000;

/**
 * The time, to the minute, for a key change that is said for a day ("Ab (was G)"). While `showing(now)` is true the caller is drawn
 * again every minute, so the words go away by themselves when the day is over; while it is false there is no timer at all.
 */
export function useKeyChangeClock(showing: (now: number) => boolean): number {
  const [, tick] = useReducer((n: number) => n + 1, 0);
  const now = Math.floor(Date.now() / MINUTE_MS) * MINUTE_MS;
  const active = showing(now);
  useEffect(() => {
    if (!active) return undefined;
    const timer = window.setInterval(tick, MINUTE_MS);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}
