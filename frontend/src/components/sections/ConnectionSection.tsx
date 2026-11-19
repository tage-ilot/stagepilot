import { Fragment, useId, useState, type ReactNode } from "react";

import type { Tone } from "../ui/types";
import { DisclosureButton } from "./DisclosureButton";
import { LiveLinkShell } from "./LiveLinkShell";

/** One connection's words in the single status line ("Playback API: Connected to Playback at 10.30.0.141."). */
export interface StatusLine {
  text: ReactNode;
  /** Kept so a page's existing test id is produced unchanged. */
  testId?: string;
  /** The line's tone, exposed as `data-tone` (the colour itself is the pill's job: words and dot, never colour alone). */
  tone?: string;
}

/** What the details area tells the page, so a setup box can start its command only when the person opened it on purpose. */
export interface DetailsState {
  open: boolean;
  /** Opened by the Details button (not by `attention`): the person asked for it. */
  byChoice: boolean;
}

export interface ConnectionSectionProps {
  /** The block's title ("Playback Live Link"). */
  title: string;
  /** The state pills beside the title, one `LiveLinkPill` per connection. */
  pills?: ReactNode;
  /** The connection row: the address input(s), the port and the one or two main buttons, with whatever belongs right under it (field errors, scan help). */
  connection: ReactNode;
  /** Each connection's status words. They are joined into ONE line by " · " and announced politely. */
  status?: StatusLine[];
  /** Quick actions that sit in the same row as the Details button, before it (Release lights, Send a test cue). Optional. */
  actions?: ReactNode;
  /** Boxes the quick actions open (the test cue's fields). They sit under the status row, outside the Details area. Optional. */
  panels?: ReactNode;
  /** The Details area: helper setup box, update line, notes. Collapsed by default. A function gets the open state. Left out: no Details button. */
  details?: ReactNode | ((state: DetailsState) => ReactNode);
  /** The Details button's words ("Library Link setup"). `null` draws the area but no button. */
  detailsLabel?: string | null;
  /** Something needs the person (the helper is not set up, an update needs the command): the Details open by themselves. */
  attention?: boolean;
  /** The connection's accent, for the page's own marks; the section itself stays neutral. */
  tone?: Tone;
  /** Test id of the section; its parts take the prefix below. */
  testId?: string;
  /** The status line's own test id (Lighting's headline), when the page's test addresses the line rather than its parts. */
  statusTestId?: string;
  /** `${prefix}-sub` is the status row, `${prefix}-actions` the group of actions and the Details button, `${prefix}-details` the area. */
  testIdPrefix?: string;
  className?: string;
  /** Always visible, under the Details area (an error that must not hide, a saving line). */
  footer?: ReactNode;
}

/**
 * The Live Link section: ONE layout for every connection (Lighting, Playback, Presentation). The order never changes, so a design
 * change here changes all three:
 *
 *   1. the title and the state pills
 *   2. the CONNECTION ROW (address, port, the main buttons) and what belongs right under it
 *   3. ONE status line: each connection's words joined by " · ", a polite live region
 *   4. the actions row: quick actions, then the Details button (the arrow shows open or closed)
 *   5. the boxes the quick actions open
 *   6. the DETAILS area, collapsed by default
 *   7. the footer
 *
 * The Details area opens by itself only when `attention` is true (initially, and again whenever it turns true). The person's choice
 * (the button) is remembered while the section stays on screen, and wins over `attention` until it turns true again.
 */
export function ConnectionSection({
  title, pills, connection, status = [], statusTestId, actions, panels, details, detailsLabel = null, attention = false, tone, testId, testIdPrefix = "live-link", className, footer,
}: ConnectionSectionProps) {
  const detailsId = useId();
  const [choice, setChoice] = useState<boolean | null>(null);
  const [seen, setSeen] = useState(attention);
  if (attention !== seen) {
    // Adjusted while rendering, so there is no frame with the old choice: attention turning true opens the details again.
    setSeen(attention);
    if (attention) setChoice(null);
  }
  const open = details !== undefined && (choice ?? attention);
  const state: DetailsState = { open, byChoice: choice === true };
  const hasButton = details !== undefined && Boolean(detailsLabel);
  const lines = status.filter((line) => line.text !== null && line.text !== undefined && line.text !== false && line.text !== "");
  const showRow = lines.length > 0 || Boolean(actions) || hasButton;

  return (
    <LiveLinkShell className={className} pill={pills} testId={testId} title={title} tone={tone}>
      <div data-part="connection">{connection}</div>

      {showRow && (
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-[0.78125rem] text-ink-muted" data-part="status-row" data-testid={`${testIdPrefix}-sub`}>
          {lines.length > 0 && (
            <p aria-live="polite" className="min-w-0 flex-[1_1_16rem] text-[0.78125rem] text-ink-muted" data-part="status" data-testid={statusTestId} role="status">
              {lines.map((line, index) => (
                <Fragment key={index}>
                  {index > 0 && <span aria-hidden="true" className="hidden sm:inline">{" \u00B7 "}</span>}
                  <span className="block sm:inline" data-testid={line.testId} data-tone={line.tone}>{line.text}</span>
                </Fragment>
              ))}
            </p>
          )}
          {(actions || hasButton) && (
            <div className="flex min-w-0 flex-wrap items-center gap-2" data-part="actions" data-testid={`${testIdPrefix}-actions`}>
              {actions}
              {hasButton && <DisclosureButton controls={detailsId} onToggle={() => setChoice(!open)} open={open}>{detailsLabel}</DisclosureButton>}
            </div>
          )}
        </div>
      )}

      {panels}

      {details !== undefined && (
        <div data-part="details" data-testid={`${testIdPrefix}-details`} hidden={!open} id={detailsId}>
          {typeof details === "function" ? details(state) : details}
        </div>
      )}

      {footer}
    </LiveLinkShell>
  );
}
