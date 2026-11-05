import { useCallback, useEffect, useRef, useState } from "react";

export type PresencePhase = "closed" | "opening" | "open" | "closing";

export interface PanelPresence<T> {
  /** The panel that is currently mounted (it stays mounted while it animates closed). */
  shown: T | null;
  phase: PresencePhase;
  /** Call when the close animation has really ended: the panel unmounts (or the next one opens) right away instead of waiting for the timer. */
  closeEnded: () => void;
}

/**
 * Keeps a panel mounted long enough to animate out. `target` is what the user wants (null = nothing).
 * closed -> opening -> open; open -> closing -> closed. Asking for a different panel closes the shown one first and
 * then opens the new one; asking for the shown panel again while it closes turns it round (closing -> opening).
 * Every timer is cleared on change/unmount, so a quick open/close never leaves a half-state.
 * With `options.swap`, asking for a different panel while one is open (or opening) swaps it in place instead: `shown`
 * changes, the phase stays, and the caller keeps one mounted wrapper so the pointer can glide to the new card.
 */
export function usePanelPresence<T>(
  target: T | null,
  durationMs: number,
  initial: T | null = null,
  options: { swap?: boolean } = {},
): PanelPresence<T> {
  const swap = options.swap === true;
  const [state, setState] = useState<{ shown: T | null; phase: PresencePhase }>({ shown: initial, phase: initial === null ? "closed" : "open" });
  const { shown, phase } = state;
  const latest = useRef({ target, shown, phase });
  latest.current = { target, shown, phase };

  const closeEnded = useCallback(() => {
    const current = latest.current;
    if (current.phase !== "closing") return;
    setState(current.target === null ? { shown: null, phase: "closed" } : { shown: current.target, phase: "opening" });
  }, []);

  useEffect(() => {
    if (target === shown) {
      if (shown === null) return;
      if (phase === "closing") { setState({ shown, phase: "opening" }); return; }
      if (phase === "opening") {
        const timer = setTimeout(() => setState({ shown, phase: "open" }), durationMs);
        return () => clearTimeout(timer);
      }
      return;
    }
    if (shown === null) { setState({ shown: target, phase: "opening" }); return; }
    if (swap && target !== null && (phase === "open" || phase === "opening")) { setState({ shown: target, phase }); return; }
    if (phase !== "closing") { setState({ shown, phase: "closing" }); return; }
    const timer = setTimeout(
      () => setState(target === null ? { shown: null, phase: "closed" } : { shown: target, phase: "opening" }),
      durationMs,
    );
    return () => clearTimeout(timer);
  }, [target, shown, phase, durationMs, swap]);

  return { shown, phase, closeEnded };
}
