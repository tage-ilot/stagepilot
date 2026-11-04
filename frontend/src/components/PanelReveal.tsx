import { useLayoutEffect, useRef, type CSSProperties, type ReactNode } from "react";

import { GenieCanvas } from "./GenieCanvas";
import { useGenieMesh } from "../hooks/useGenieMesh";
import { genieGeometry } from "../lib/genieGeometry";
import type { PresencePhase } from "../hooks/usePanelPresence";

export interface PanelRevealProps {
  phase: PresencePhase;
  durationMs: number;
  /** The connection card this panel belongs to; the panel emerges from, and returns into, it. */
  trigger: () => HTMLElement | null;
  children: ReactNode;
}

/** Current translateY of an element (the genie animation moves the wrapper up), so layout position can be recovered. */
function currentTranslateY(element: HTMLElement): number {
  try {
    const transform = getComputedStyle(element).transform;
    if (!transform || transform === "none" || typeof DOMMatrixReadOnly === "undefined") return 0;
    return new DOMMatrixReadOnly(transform).m42 || 0;
  } catch {
    return 0;
  }
}

/**
 * One wrapper animates the height (grid rows 0fr <-> 1fr, so the content below moves in lock-step) and slides up by the
 * card gap; the visible clip box animates the funnel clip-path and opacity. See .panel-reveal in index.css and
 * DESIGN.md "Config panel reveal".
 */
export function PanelReveal({ phase, durationMs, trigger, children }: PanelRevealProps) {
  const root = useRef<HTMLDivElement>(null);
  const slot = useRef<HTMLDivElement>(null);
  const closing = phase === "closing";
  useGenieMesh({ root, slot, phase, durationMs, trigger });

  // The card's own edges (as a % of the panel row) and the gap between its bottom edge and this wrapper.
  useLayoutEffect(() => {
    const element = root.current;
    const card = trigger();
    if (!element || !card) return;
    // A mesh run owns the wrapper's geometry; these variables only feed the CSS fallback animation.
    if (element.hasAttribute("data-mesh")) return;
    const host = element.getBoundingClientRect();
    // The panel's own top margin sits inside the clip box, so the visible shape starts at the first content element.
    const content = element.querySelector<HTMLElement>(".panel-reveal__panel")?.firstElementChild ?? element;
    const contentTop = content.getBoundingClientRect().top - currentTranslateY(element);
    const geometry = genieGeometry(card.getBoundingClientRect(), { left: host.left, width: host.width, top: contentTop });
    if (!geometry) return;
    element.style.setProperty("--genie-x", `${geometry.x}%`);
    element.style.setProperty("--genie-w", `${geometry.w}%`);
    element.style.setProperty("--genie-l", `${geometry.l}%`);
    element.style.setProperty("--genie-r", `${geometry.r}%`);
    element.style.setProperty("--genie-gap", `${geometry.gap}px`);
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
      <GenieCanvas slot={slot} />
      <div className="panel-reveal__clip">
        <div className="panel-reveal__panel">{children}</div>
      </div>
    </div>
  );
}
