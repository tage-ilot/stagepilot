import { useId, type ReactNode } from "react";

import { cx } from "./cx";
import { FieldContext } from "./fieldContext";

export interface FieldProps {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  /** Visual marker only (warning dot, "Not saved yet", warning border). Save logic lives elsewhere. */
  heldChange?: boolean;
  /** Something that belongs right beside the label, not inside it (a `?` help button). */
  labelExtra?: ReactNode;
  /** A button that belongs to this one control and sits right under it (Presentation's `Refresh timers and Looks` under Timer): an `ActionRow` with its `ActionButton`. Drawn after the hint and the error, one `gap-1.5` below the control, so it stays in this column whatever the neighbouring field's height. */
  action?: ReactNode;
  className?: string;
  children: ReactNode;
}

/**
 * Label above, control, hint, then error. Wiring uses a small context (simpler than a render prop):
 * Input and Select inside a Field pick up id, aria-describedby, aria-invalid and the held border.
 * Field clones nothing. Place exactly one control inside. An `action` (one button of its own, with its note) sits under all of that.
 */
export function Field({ label, hint, error, heldChange = false, labelExtra, action, className, children }: FieldProps) {
  const uid = useId();
  const id = `${uid}-control`;
  const hintId = hint ? `${uid}-hint` : undefined;
  const errorId = error ? `${uid}-error` : undefined;
  const heldId = heldChange ? `${uid}-held` : undefined;
  const describedBy = [hintId, errorId, heldId].filter(Boolean).join(" ") || undefined;

  return (
    <FieldContext.Provider value={{ id, describedBy, invalid: Boolean(error), held: heldChange }}>
      <div data-field data-state={error ? "error" : heldChange ? "held" : "default"} className={cx("flex min-w-0 flex-col gap-1.5", className)}>
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
          {labelExtra ? (
            <span className="flex min-w-0 items-center gap-1.5">
              <label htmlFor={id} className="min-w-0 break-words font-type-text-strong text-type-text-strong text-ink">{label}</label>
              {labelExtra}
            </span>
          ) : (
            <label htmlFor={id} className="min-w-0 break-words font-type-text-strong text-type-text-strong text-ink">
              {label}
            </label>
          )}
          {heldChange && <HeldMarker id={heldId} />}
        </div>
        {children}
        {hint && (
          <p id={hintId} className="break-words text-type-body-sm text-ink-muted">
            {hint}
          </p>
        )}
        {error && (
          <p id={errorId} role="alert" className="break-words text-type-body-sm text-danger-text">
            {error}
          </p>
        )}
        {action && <div className="min-w-0" data-part="action">{action}</div>}
      </div>
    </FieldContext.Provider>
  );
}

/** The one "Not saved yet" mark: a warning dot and the words (never colour alone). Field draws it; a control that is not a Field (a checkbox row) uses it too. */
export function HeldMarker({ id, className }: { id?: string; className?: string }) {
  return (
    <span id={id} data-held-marker className={cx("inline-flex items-center gap-1.5 text-type-body-sm text-warning-text", className)}>
      <span aria-hidden="true" className="h-2 w-2 rounded-ds-full bg-warning" />
      Not saved yet
    </span>
  );
}
