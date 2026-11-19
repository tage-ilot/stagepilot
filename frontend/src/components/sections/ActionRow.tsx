import type { ReactNode } from "react";

import { Button } from "../ui/Button";
import { cx } from "../ui/cx";
import type { Tone } from "../ui/types";

export interface ActionRowProps {
  /** The buttons, in order: use `ActionButton` so each one looks and words its busy state the same way. */
  children: ReactNode;
  /** Why the buttons use the saved settings (shown by the page only while a change is held): the row's one note, under the buttons. */
  note?: ReactNode;
  noteId?: string;
  className?: string;
  testId?: string;
  "aria-label"?: string;
}

/**
 * The ACTIONS section: the Test / Refresh / Reload buttons in one wrapping row, then its one note ("Test connection and Refresh use the
 * saved settings. Save configuration first to try the new ones."). The buttons are quick actions that never save anything.
 */
export function ActionRow({ children, note, noteId, className, testId, "aria-label": ariaLabel }: ActionRowProps) {
  return (
    <div className={cx("min-w-0", note ? "space-y-4" : undefined, className)} data-section="actions" data-testid={testId}>
      <div aria-label={ariaLabel} className="flex flex-wrap gap-2" data-part="buttons" role={ariaLabel ? "group" : undefined}>{children}</div>
      {note && <p className="text-sm text-ink-muted" data-part="note" id={noteId}>{note}</p>}
    </div>
  );
}

export interface ActionButtonProps {
  label: string;
  /** The words while the request runs (`Refreshing…`). */
  busyLabel?: string;
  /** This button's own request is running: it shows `busyLabel`. */
  busy?: boolean;
  disabled?: boolean;
  onClick: () => void;
  /** The connection's accent. */
  tone?: Tone;
  /** Tinted by default: an action that changes nothing saved. `primary` for the one action the row exists for (Connect). */
  variant?: "tinted" | "primary" | "secondary";
  className?: string;
  title?: string;
  "aria-describedby"?: string;
}

/** One button of an `ActionRow`: the label, then the busy words, with the row's one look. */
export function ActionButton({ label, busyLabel, busy = false, disabled = false, onClick, tone, variant = "tinted", className, title, "aria-describedby": describedBy }: ActionButtonProps) {
  return (
    <Button aria-describedby={describedBy} className={className} disabled={disabled} onClick={onClick} title={title} tone={variant === "tinted" ? tone : undefined} variant={variant}>
      {busy && busyLabel ? busyLabel : label}
    </Button>
  );
}
