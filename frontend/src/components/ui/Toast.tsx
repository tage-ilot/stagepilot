import { forwardRef, useId, type MouseEvent, type ReactNode } from "react";

import { Button } from "./Button";
import { cx } from "./cx";

export type ToastTone = "danger" | "warning";
export type ToastSide = "left" | "right";

export interface ToastProps {
  /** Default danger (rose border). */
  tone?: ToastTone;
  /** DESIGN.md Toasts: left = global error, right = crash-loop. */
  side: ToastSide;
  /** Small uppercase line above the title (optional). */
  kicker?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  /** Action buttons (use `Button`). Rendered right-aligned after the Dismiss button. */
  actions?: ReactNode;
  /** When set, a secondary Dismiss button is rendered first in the action row. */
  onDismiss?: () => void;
  dismissLabel?: string;
  /** Extra content under the actions (for example a polite status line). */
  children?: ReactNode;
  /** Default false: a toast never blocks the page, traps focus or draws a scrim. */
  modal?: boolean;
  className?: string;
}

// Class strings are written out in full so Tailwind can see them.
const tones: Record<ToastTone, { border: string; kicker: string }> = {
  danger: { border: "border-danger-soft/30", kicker: "text-danger-text" },
  warning: { border: "border-warning/30", kicker: "text-warning-text" },
};

const sides: Record<ToastSide, string> = {
  left: "left-6",
  right: "right-6",
};

/**
 * Fixed bottom-corner notice. Non-modal `alertdialog` (aria-modal=false), assertive live region, programmatically
 * focusable (tabIndex -1) so the caller can move focus to it; no focus trap, no scrim. The ref targets the
 * alertdialog element. z 110 (DESIGN.md Stacking order).
 */
export const Toast = forwardRef<HTMLDivElement, ToastProps>(function Toast(
  { tone = "danger", side, kicker, title, description, actions, onDismiss, dismissLabel = "Dismiss", children, modal = false, className },
  ref,
) {
  const id = useId();
  const titleId = `${id}-title`;
  const descId = `${id}-desc`;
  const look = tones[tone];
  return (
    <div
      data-toast
      data-tone={tone}
      data-side={side}
      onMouseDown={(event: MouseEvent<HTMLDivElement>) => event.stopPropagation()}
      className={cx(
        "fixed bottom-6 z-[110] w-full max-w-[min(24rem,calc(100vw-3rem))] overscroll-contain rounded-ds-2xl border bg-surface-950 p-5 shadow-elevation-overlay touch-manipulation",
        sides[side],
        look.border,
        className,
      )}
    >
      <div
        ref={ref}
        role="alertdialog"
        aria-modal={modal ? "true" : "false"}
        aria-live="assertive"
        aria-labelledby={titleId}
        aria-describedby={description ? descId : undefined}
        tabIndex={-1}
      >
        {kicker && <p className={cx("font-type-kicker text-type-kicker uppercase", look.kicker)}>{kicker}</p>}
        <h2 id={titleId} className="mt-1 break-words font-type-h2 text-type-h2 text-ink">
          {title}
        </h2>
        {description && (
          <p id={descId} className="mt-2 break-words font-type-body-md text-type-body-md text-ink-body">
            {description}
          </p>
        )}
        {(actions || onDismiss) && (
          <div data-toast-actions className="mt-4 flex flex-wrap justify-end gap-2">
            {onDismiss && (
              <Button size="touch" variant="secondary" onClick={onDismiss}>
                {dismissLabel}
              </Button>
            )}
            {actions}
          </div>
        )}
        {children}
      </div>
    </div>
  );
});
