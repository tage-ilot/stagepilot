import { useId, type ReactNode } from "react";

import { cx } from "./cx";
import { FieldContext } from "./fieldContext";

export interface FieldProps {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  /** Visual marker only (amber dot, "Not saved yet", amber border). Save logic lives elsewhere. */
  heldChange?: boolean;
  className?: string;
  children: ReactNode;
}

/**
 * Label above, control, hint, then error. Wiring uses a small context (simpler than a render prop):
 * Input, Select and Textarea inside a Field pick up id, aria-describedby, aria-invalid and the held border.
 * Field clones nothing. Place exactly one control inside.
 */
export function Field({ label, hint, error, heldChange = false, className, children }: FieldProps) {
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
          <label htmlFor={id} className="min-w-0 break-words font-type-text-strong text-type-text-strong text-ink">
            {label}
          </label>
          {heldChange && (
            <span id={heldId} data-held-marker className="inline-flex items-center gap-1.5 text-type-body-sm text-warning-text">
              <span aria-hidden="true" className="h-2 w-2 rounded-ds-full bg-warning" />
              Not saved yet
            </span>
          )}
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
      </div>
    </FieldContext.Provider>
  );
}
