import { forwardRef, useId, type HTMLAttributes, type ReactNode } from "react";

import { Button, type ButtonProps } from "../ui/Button";
import { cx } from "../ui/cx";

/** The wrapper's own attributes: `data-*` hooks for tests and CSS (`data-part`, `data-scan`), a class. Never `title` (that is the reason). */
export type ReasonWrapAttributes = Omit<HTMLAttributes<HTMLSpanElement>, "title" | "children"> & { [hook: `data-${string}`]: string | undefined };

export interface ReasonWrapOptions {
  /** Why the control is off, in words an operator can act on (`Sign in to Planning Center first.`). Empty or missing: the control works and nothing is added. */
  reason?: string | null;
  /**
   * The wrapper is drawn even while there is no reason (the default): a control that turns off while it has the focus (Zoom out at the end of its
   * range, Disconnect once it has disconnected) keeps its element and its focus when the reason comes, and the layout never moves. `false`: no
   * wrapper while there is no reason (only where nothing can toggle).
   */
  alwaysWrap?: boolean;
  wrapProps?: ReasonWrapAttributes;
  /** The control, given the id of the hidden copy of the reason to point `aria-describedby` at (undefined while there is no reason). */
  children: (describedBy: string | undefined) => ReactNode;
}

/**
 * The ONE wrapper for a control that is switched off with a reason (decided 2026-11-18, the design audit's "disabled controls say why").
 * A disabled control shows no tooltip of its own in every browser and a touch screen has no hover, so the reason sits on a `<span title>`
 * around it AND as a visually hidden copy the control points at with `aria-describedby`: a screen reader reads it as the control's description.
 * Works for any control (a button, a select, an arrow); `ReasonButton` and `ActionButton` use it.
 */
export function ReasonWrap({ reason, alwaysWrap = true, wrapProps, children }: ReasonWrapOptions) {
  const reasonId = useId();
  const why = reason?.trim() ? reason : null;
  if (!why && !alwaysWrap) return <>{children(undefined)}</>;
  const { className, ...rest } = wrapProps ?? {};
  return (
    <span className={cx("inline-flex", className)} data-reason={why ? "true" : undefined} title={why ?? undefined} {...rest}>
      {children(why ? reasonId : undefined)}
      {why && <span className="sr-only" id={reasonId}>{why}</span>}
    </span>
  );
}

export interface ReasonButtonProps extends ButtonProps {
  /** Why it is off. Given, the button is disabled, whatever `disabled` says. */
  reason?: string | null;
  alwaysWrap?: boolean;
  wrapProps?: ReasonWrapAttributes;
}

/** A `Button` that says why it is off: with a `reason` it is disabled, wrapped, titled and described by it (see `ReasonWrap`). Without one it is the plain button. */
export const ReasonButton = forwardRef<HTMLButtonElement, ReasonButtonProps>(function ReasonButton(
  { reason, alwaysWrap, wrapProps, disabled, "aria-describedby": describedBy, title, ...button },
  ref,
) {
  const why = reason?.trim() ? reason : null;
  return (
    <ReasonWrap alwaysWrap={alwaysWrap} reason={why} wrapProps={wrapProps}>
      {(reasonId) => (
        <Button
          {...button}
          aria-describedby={[describedBy, reasonId].filter(Boolean).join(" ") || undefined}
          disabled={Boolean(why) || disabled}
          ref={ref}
          title={why ?? title}
        />
      )}
    </ReasonWrap>
  );
});
