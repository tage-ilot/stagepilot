import { useLayoutEffect, useRef, type CSSProperties, type ReactNode } from "react";

import type { PresencePhase } from "../hooks/usePanelPresence";

export interface PanelRevealProps {
  phase: PresencePhase;
  durationMs: number;
  /** The connection card this panel belongs to; the panel emerges from, and returns into, it. */
  trigger: () => HTMLElement | null;
  children: ReactNode;
}

/**
 * One wrapper animates the height (grid rows 0fr <-> 1fr, so the content below moves in lock-step); the panel inside
 * animates only transform, clip-path and opacity. See .panel-reveal in index.css and DESIGN.md "Config panel reveal".
 */
export function PanelReveal({ phase, durationMs, trigger, children }: PanelRevealProps) {
  const root = useRef<HTMLDivElement>(null);
  const closing = phase === "closing";

  // Horizontal origin = the card's own centre and width, as a percentage of the panel row.
  useLayoutEffect(() => {
    const element = root.current;
    const card = trigger();
    if (!element || !card) return;
    const host = element.getBoundingClientRect();
    const box = card.getBoundingClientRect();
    if (host.width <= 0) return;
    element.style.setProperty("--genie-x", `${(((box.left + box.width / 2) - host.left) / host.width) * 100}%`);
    element.style.setProperty("--genie-w", `${Math.min(100, (box.width / host.width) * 100)}%`);
  }, [trigger, phase]);

  // Closing: if focus is inside the panel, hand it back to the card before the panel goes inert.
  useLayoutEffect(() => {
    if (!closing) return;
    const element = root.current;
    if (element && element.contains(document.activeElement)) trigger()?.focus();
  }, [closing, trigger]);

  return (
    <div
      ref={root}
      className="panel-reveal"
      data-phase={phase}
      style={{ "--genie-ms": `${durationMs}ms` } as CSSProperties}
      aria-hidden={closing || undefined}
      inert={closing}
    >
      <div className="panel-reveal__clip">
        <div className="panel-reveal__panel">{children}</div>
      </div>
    </div>
  );
}
