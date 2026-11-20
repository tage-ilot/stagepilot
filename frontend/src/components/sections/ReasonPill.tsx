import type { ReactNode } from "react";

import { cx } from "../ui/cx";

export interface ReasonPillProps {
  /** The short reason in words (`Choose a playlist`). Empty: nothing is drawn. */
  children?: ReactNode;
  /** What the button's `aria-describedby` points at. */
  id?: string;
  testId?: string;
  className?: string;
}

/**
 * The reason a button is off (or stopped), drawn BESIDE the button as a small neutral pill: an info icon and a few words, never red (it is a
 * reason, not an error) and never colour alone. It sits in the button's `ActionRow`, vertically centred with it, and wraps under the button on
 * a phone. The same words are the button's `title` or `aria-describedby`. One template for every reason drawn beside a button.
 */
export function ReasonPill({ children, id, testId, className }: ReasonPillProps) {
  if (!children) return null;
  return (
    <span
      className={cx("inline-flex min-w-0 max-w-full items-center gap-[7px] rounded-ds-full border border-edge bg-surface-950 px-3 py-[5px] text-[0.8125rem] leading-[1.45] text-ink", className)}
      data-reason-pill="true"
      data-testid={testId}
      id={id}
    >
      <svg aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-ink-muted" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" viewBox="0 0 20 20">
        <circle cx="10" cy="10" r="8" />
        <path d="M10 9v5M10 6.2v.1" />
      </svg>
      <span className="min-w-0 break-words">{children}</span>
    </span>
  );
}
