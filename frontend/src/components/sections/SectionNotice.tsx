import type { ReactNode } from "react";

import { accentClasses, useAccent } from "../ui/accent";
import { cx } from "../ui/cx";
import { InlineAlert } from "../ui/InlineAlert";

export type NoticeTone = "info" | "warning" | "danger" | "success";

/** `accent` (lines only; a box takes it as `info`): the connection's own colour, for what this connection is telling (the song order's mode). */
type LineTone = NoticeTone | "accent";

/** A page's existing test hooks (`data-access-hint`) ride along unchanged. */
type DataAttributes = { [key: `data-${string}`]: string | boolean | undefined };

export interface SectionNoticeProps extends DataAttributes {
  tone: LineTone;
  children?: ReactNode;
  /**
   * `box`: the bordered line (an `InlineAlert`). `line`: a quiet sentence in the tone's text colour, for what belongs to a field or a
   * button (why Save is off, what a restart does).
   */
  variant?: "box" | "line";
  /** A bold first line (`Your Services connection has expired`). Box only. */
  title?: ReactNode;
  /** A button (or two): under the words of a box, beside the words of a line. */
  actions?: ReactNode;
  /** A line's role: none unless the page asks. A box always follows its tone, as `InlineAlert` does (warning and danger interrupt, info and success are polite), unless it is a `group`. */
  role?: "alert" | "status";
  /**
   * Box only: the box asks a question the person just caused ("Disconnect all people?") and holds its buttons. It is a named group, not an
   * interruption: the name is its accessible label.
   */
  group?: string;
  /** An `info` line in the muted ink (a hint-like note) instead of the body ink. Line only. */
  quiet?: boolean;
  /** A polite live region the page already had (a message that appears after an action). */
  live?: boolean;
  /** Keep the plain tone inside a connection's panel instead of taking the connection's colour (a neutral note). Box only. */
  neutral?: boolean;
  /** Semi-bold words (a line that is the section's headline). Line only. */
  strong?: boolean;
  /** The bit of text size a line takes: `sm` (default) or `xs`. */
  size?: "sm" | "xs";
  id?: string;
  className?: string;
  testId?: string;
}

const lineTone: Record<Exclude<NoticeTone, "success" | "info">, string> = { warning: "text-warning-text", danger: "text-danger-text" };

/**
 * The NOTICE section: one line of news in a tone (info, warning, danger, success). A box is an `InlineAlert` (same roles as everywhere:
 * warning and danger are `alert`, info and success are `status`); a line is the same words without the box. Colour never carries it
 * alone: the page's words say what happened.
 */
export function SectionNotice({ tone, children, variant = "box", title, actions, role, group, quiet = false, strong = false, live = false, neutral = false, size = "sm", id, className, testId, ...data }: SectionNoticeProps) {
  const accent = useAccent();
  if (variant === "box") {
    return (
      <InlineAlert {...data} aria-label={group} aria-live={live ? "polite" : undefined} role={group ? "group" : undefined} className={cx(title !== undefined && "px-4 py-3", className)} data-part="notice" data-testid={testId} id={id} neutral={neutral} tone={tone === "accent" ? "info" : tone}>
        {title !== undefined ? <p className="font-semibold">{title}</p> : null}
        {title !== undefined ? (children ? <p className="mt-1">{children}</p> : null) : children}
        {actions && <div className="mt-3">{actions}</div>}
      </InlineAlert>
    );
  }
  const colour = tone === "warning" || tone === "danger" ? lineTone[tone] : tone === "accent" ? (accent ? accentClasses[accent].text : "text-info-text") : tone === "success" ? (accent ? accentClasses[accent].text : "text-success-text") : quiet ? "text-ink-muted" : "text-ink-body";
  const text = cx(size === "xs" ? "text-xs" : "text-sm", strong && "font-semibold", colour);
  if (actions) {
    // A line that has its button beside it ("Use the OSC host"): one row, the words first. The row carries the role and the test id.
    return (
      <div {...data} aria-live={live ? "polite" : undefined} className={cx("flex flex-wrap items-center gap-2", className)} data-part="notice" data-testid={testId} data-tone={tone} id={id} role={role}>
        <p className={text}>{children}</p>
        {actions}
      </div>
    );
  }
  return (
    <p {...data} aria-live={live ? "polite" : undefined} className={cx(text, className)} data-part="notice" data-testid={testId} data-tone={tone} id={id} role={role}>
      {children}
    </p>
  );
}
