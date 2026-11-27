import { useId, type ReactNode } from "react";

import { usePersistedFlag } from "../../hooks/usePersistedFlag";
import { cx } from "../ui/cx";
import type { DisclosureTone } from "../ui/Disclosure";
import { featureTone } from "../ui/featureTone";

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
  /** Let the summary wrap onto more lines instead of being cut off beside the buttons (a state that must be read whole: the Auto-Presenter). The toggle keeps at least 16rem, so on a phone the buttons drop to a row of their own. */
  wrapSummary?: boolean;
  children: ReactNode;
}

/**
 * An OPTIONAL section whose header also carries buttons (Playback's Song order and its Scan button, the Presenter Link). It wears the
 * same card as a feature section (the Song lighting timeline): a rail and a soft wash in the connection's colour, the title in the
 * accent with the one-line summary under it, and the open area divided by a rule. The toggle (chevron, title, summary) and the actions
 * share the header row; the body folds in 300ms and is `inert` while closed.
 */
export function OptionalBand({ title, summary, summaryTestId, actions, persistKey, defaultOpen = true, forceOpen = false, onOpenChange, tone = "playback", headingId, wrapSummary = false, children }: OptionalBandProps) {
  const [saved, setSaved] = usePersistedFlag(persistKey, defaultOpen);
  const open = saved || forceOpen;
  const bodyId = useId();
  const look = featureTone[tone];
  return (
    <section aria-labelledby={headingId} className={cx("min-w-0 overflow-hidden rounded-ds-lg border border-l-[5px]", look.card, look.rail)} data-section="band" data-state={open ? "open" : "closed"} data-tone={tone}>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 pr-3" data-part="band-header">
        <button aria-controls={bodyId} aria-expanded={open} className={cx("flex min-h-[4.25rem] min-w-0 touch-manipulation items-center gap-3 py-3.5 pl-4 text-left transition-colors motion-reduce:transition-none", wrapSummary ? "flex-[1_1_16rem]" : "flex-1", look.hover)} onClick={() => { setSaved(!open); onOpenChange?.(!open); }} type="button">
          <svg aria-hidden="true" className={cx("size-3.5 shrink-0 transition-transform duration-[140ms] motion-reduce:transition-none", look.sub, open && "rotate-90")} data-part="chevron" viewBox="0 0 12 12">
            <path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.75" />
          </svg>
          {/* The title, and the one-line summary under it (the status). With buttons beside the toggle the summary is cut off when there is no room; a band with none (the Presenter Link) lets it wrap on a phone. */}
          <span className="flex min-w-0 flex-1 flex-col">
            <h3 className={cx("font-type-body-md text-lg font-bold", look.title)} data-part="title" id={headingId}>{title}</h3>
            {summary !== undefined && <span className={cx("mt-0.5 min-w-0 text-sm text-ink-body", wrapSummary ? "break-words" : actions ? "truncate" : "sm:truncate")} data-part="summary" data-testid={summaryTestId}>{summary}</span>}
          </span>
        </button>
        {actions}
      </div>
      <div className={cx("grid transition-[grid-template-rows,opacity] duration-300 ease-out motion-reduce:transition-none", open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0")} data-part="region" data-state={open ? "open" : "closed"} id={bodyId}>
        <div className="min-h-0 overflow-hidden" inert={!open}>
          <div className={cx("space-y-2 border-t p-4", look.rule)} data-part="body">{children}</div>
        </div>
      </div>
    </section>
  );
}
