import { useEffect, useState } from "react";

export type PresencePhase = "closed" | "opening" | "open" | "closing";

export interface PanelPresence<T> {
  /** The panel that is currently mounted (it stays mounted while it animates closed). */
  shown: T | null;
  phase: PresencePhase;
}

/**
 * Keeps a panel mounted long enough to animate out. `target` is what the user wants (null = nothing).
 * closed -> opening -> open; open -> closing -> closed. Asking for a different panel closes the shown one first and
 * then opens the new one; asking for the shown panel again while it closes turns it round (closing -> opening).
 * Every timer is cleared on change/unmount, so a quick open/close never leaves a half-state.
 */
export function usePanelPresence<T>(target: T | null, durationMs: number, initial: T | null = null): PanelPresence<T> {
  const [state, setState] = useState<PanelPresence<T>>({ shown: initial, phase: initial === null ? "closed" : "open" });
  const { shown, phase } = state;

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
    if (phase !== "closing") { setState({ shown, phase: "closing" }); return; }
    const timer = setTimeout(
      () => setState(target === null ? { shown: null, phase: "closed" } : { shown: target, phase: "opening" }),
      durationMs,
    );
    return () => clearTimeout(timer);
  }, [target, shown, phase, durationMs]);

  return state;
}
