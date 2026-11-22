import { useId, type ReactNode } from "react";

import { usePersistedFlag } from "../../hooks/usePersistedFlag";
import { accentClasses, useAccent } from "../ui/accent";
import { cx } from "../ui/cx";
import type { DisclosureTone } from "../ui/Disclosure";

export interface OptionalBandProps {
  /** The heading inside the button (`Song order`). */
  title: string;
  /** One line beside the heading: what is inside (`Saved: 12 songs`). A polite status of the section. */
  summary?: ReactNode;
  summaryTestId?: string;
  /** Buttons that sit on the header row, beside the toggle (never inside it): `Scan Playback API` / `Scan Playback Library`. */
  actions?: ReactNode;
  persistKey: string;
  defaultOpen?: boolean;
  /** Open while something needs the person (a job that is running or just ended), whatever they chose; their remembered choice is left alone. */
  forceOpen?: boolean;
  /** The person pressed the toggle: what it asked for (`true` opens). Called even while `forceOpen` keeps the body open. */
  onOpenChange?: (open: boolean) => void;
  /** Tint of the band (the connection's own). */
  tone?: DisclosureTone;
  headingId?: string;
  children: ReactNode;
}

const bandTone: Record<string, string> = { playback: "bg-integration-playback-soft/5", presentation: "bg-integration-presentation-soft/5" };

/**
 * An OPTIONAL section whose header also carries buttons (Playback's Song order and its Scan button). A tinted band: the toggle
 * (chevron, heading, one-line summary) and the actions share the header row; the body folds in 300ms and is `inert` while closed.
 */
export function OptionalBand({ title, summary, summaryTestId, actions, persistKey, defaultOpen = true, forceOpen = false, onOpenChange, tone = "playback", headingId, children }: OptionalBandProps) {
  const [saved, setSaved] = usePersistedFlag(persistKey, defaultOpen);
  const open = saved || forceOpen;
  const bodyId = useId();
  const accent = useAccent();
  const chevron = accent ? accentClasses[accent].text : "text-integration-playback-text";
  return (
    <section aria-labelledby={headingId} className={cx("space-y-2 rounded-ds-xl p-4", bandTone[tone] ?? bandTone.playback)} data-section="band">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2" data-part="band-header">
        <button aria-controls={bodyId} aria-expanded={open} className="flex min-h-ds-touch-target min-w-0 flex-1 touch-manipulation items-center gap-3 rounded-ds-lg text-left can-hover:bg-surface-900/40" onClick={() => { setSaved(!open); onOpenChange?.(!open); }} type="button">
          <svg aria-hidden="true" className={cx("size-3.5 shrink-0 transition-transform duration-[140ms] motion-reduce:transition-none", chevron, open && "rotate-90")} data-part="chevron" viewBox="0 0 12 12">
            <path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.75" />
          </svg>
          {/* The heading and the one-line summary, side by side. With buttons beside the toggle the summary is cut off when there is no room; a band with none (the Presenter Link) drops it under the heading on a phone instead, because it is the status. */}
          <span className={cx("flex min-w-0 flex-1 items-center gap-x-3 gap-y-0.5", !actions && "flex-wrap")}>
            <h3 className="shrink-0 font-bold text-ink" id={headingId}>{title}</h3>
            {summary !== undefined && <span className={cx("min-w-0 text-sm text-ink-body", actions ? "truncate" : "sm:truncate")} data-part="summary" data-testid={summaryTestId}>{summary}</span>}
          </span>
        </button>
        {actions}
      </div>
      <div className={cx("grid transition-[grid-template-rows,opacity] duration-300 ease-out motion-reduce:transition-none", open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0")} data-part="region" data-state={open ? "open" : "closed"} id={bodyId}>
        <div className="min-h-0 space-y-2 overflow-hidden" inert={!open}>{children}</div>
      </div>
    </section>
  );
}
