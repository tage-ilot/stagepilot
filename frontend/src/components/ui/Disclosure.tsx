import { useId, useState, type ComponentPropsWithoutRef, type ReactNode } from "react";

import { accentClasses, useAccent } from "./accent";
import { cx } from "./cx";

export type DisclosureVariant = "advanced" | "activity" | "feature";
export type DisclosureTone = "services" | "presentation" | "playback" | "midi" | "lighting" | "backend";

export interface DisclosureProps extends Omit<ComponentPropsWithoutRef<"div">, "title" | "children"> {
  variant: DisclosureVariant;
  /** Required for `advanced` (picks the advanced-* colour); ignored by `activity`, which is always neutral. */
  tone?: DisclosureTone;
  title: ReactNode;
  /** One-line summary shown on the right ONLY while closed, e.g. `Port 8080 · Channel 1`. Truncates; full text in title. */
  summary?: ReactNode;
  /** Feature: a second line under the title, always shown, saying what is inside (for example `16 cues · 2 of 4 songs`). */
  detail?: ReactNode;
  /** Feature: a small tag at the right of the header, for example the active method. */
  aside?: ReactNode;
  /** Activity: renders a count badge such as `12 events`. */
  count?: number;
  /** Singular noun for the count badge; plural adds an `s`. Default `event`. */
  countNoun?: string;
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: ReactNode;
}

// Composition rule (DESIGN.md "Collapsible sections"): General configuration content is never inside a Disclosure and
// Advanced always comes AFTER General configuration. The caller composes that order; this component does not enforce it.
//
// Collapsed body: the inner wrapper is `inert`, which removes it from the tab order and the accessibility
// tree at once, with no timing dependence on transitionend. (Chosen over `hidden`-after-transition.)
//
// Reduced motion: `motion-reduce:transition-none` makes the body appear/disappear instantly; the chevron
// still rotates (the state stays visible), just without animation.
const advancedTone: Record<DisclosureTone, { row: string; bar: string }> = {
  services: { row: "bg-advanced-services/10 text-advanced-services can-hover:bg-advanced-services/20", bar: "border-advanced-services" },
  presentation: { row: "bg-advanced-presentation/10 text-advanced-presentation can-hover:bg-advanced-presentation/20", bar: "border-advanced-presentation" },
  playback: { row: "bg-advanced-playback/10 text-advanced-playback can-hover:bg-advanced-playback/20", bar: "border-advanced-playback" },
  midi: { row: "bg-advanced-playback-midi/10 text-advanced-playback-midi can-hover:bg-advanced-playback-midi/20", bar: "border-advanced-playback-midi" },
  lighting: { row: "bg-advanced-lighting/10 text-advanced-lighting can-hover:bg-advanced-lighting/20", bar: "border-advanced-lighting" },
  backend: { row: "bg-advanced-backend/10 text-advanced-backend can-hover:bg-advanced-backend/20", bar: "border-advanced-backend" },
};

// A "feature" section is a major part of a panel that some people never use, so it folds away but still reads as a destination:
// a card with a 3px rail and a soft wash in the connection's colour, a two-line header, and the open area divided by a rule.
// The Save bar below it is a flat full tint with no rail, so the two sit together without looking the same.
const featureTone: Record<DisclosureTone, { card: string; rail: string; title: string; sub: string; rule: string; hover: string }> = {
  services: { card: "border-integration-services-soft/70 bg-gradient-to-r from-integration-services-soft/[0.26] via-integration-services-soft/[0.10] to-integration-services-soft/[0.03]", rail: "border-l-integration-services-soft", title: "text-integration-services-soft", sub: "text-integration-services-text", rule: "border-integration-services-soft/25", hover: "can-hover:bg-integration-services-soft/10" },
  presentation: { card: "border-integration-presentation-soft/70 bg-gradient-to-r from-integration-presentation-soft/[0.26] via-integration-presentation-soft/[0.10] to-integration-presentation-soft/[0.03]", rail: "border-l-integration-presentation-soft", title: "text-integration-presentation-soft", sub: "text-integration-presentation-text", rule: "border-integration-presentation-soft/25", hover: "can-hover:bg-integration-presentation-soft/10" },
  playback: { card: "border-integration-playback-soft/70 bg-gradient-to-r from-integration-playback-soft/[0.26] via-integration-playback-soft/[0.10] to-integration-playback-soft/[0.03]", rail: "border-l-integration-playback-soft", title: "text-integration-playback-text", sub: "text-integration-playback-text", rule: "border-integration-playback-soft/25", hover: "can-hover:bg-integration-playback-soft/10" },
  midi: { card: "border-integration-playback-midi-soft/70 bg-gradient-to-r from-integration-playback-midi-soft/[0.26] via-integration-playback-midi-soft/[0.10] to-integration-playback-midi-soft/[0.03]", rail: "border-l-integration-playback-midi-soft", title: "text-integration-playback-midi-text", sub: "text-integration-playback-midi-text", rule: "border-integration-playback-midi-soft/25", hover: "can-hover:bg-integration-playback-midi-soft/10" },
  lighting: { card: "border-integration-lighting-soft/70 bg-gradient-to-r from-integration-lighting-soft/[0.26] via-integration-lighting-soft/[0.10] to-integration-lighting-soft/[0.03]", rail: "border-l-integration-lighting-soft", title: "text-integration-lighting-soft", sub: "text-integration-lighting-text", rule: "border-integration-lighting-soft/25", hover: "can-hover:bg-integration-lighting-soft/10" },
  backend: { card: "border-integration-backend-soft/70 bg-gradient-to-r from-integration-backend-soft/[0.26] via-integration-backend-soft/[0.10] to-integration-backend-soft/[0.03]", rail: "border-l-integration-backend-soft", title: "text-integration-backend-text", sub: "text-integration-backend-text", rule: "border-integration-backend-soft/25", hover: "can-hover:bg-integration-backend-soft/10" },
};

function summaryText(summary: ReactNode) {
  return typeof summary === "string" || typeof summary === "number" ? String(summary) : undefined;
}

export function Disclosure({
  variant,
  tone,
  title,
  summary,
  detail,
  aside,
  count,
  countNoun = "event",
  open: openProp,
  defaultOpen = false,
  onOpenChange,
  children,
  className,
  ...rest
}: DisclosureProps) {
  const [inner, setInner] = useState(defaultOpen);
  const controlled = openProp !== undefined;
  const open = controlled ? openProp : inner;
  const bodyId = useId();
  const advanced = variant === "advanced";
  const palette = advanced ? advancedTone[tone ?? "services"] : undefined;
  // The count badge wears the connection colour inside a connection panel, teal elsewhere.
  const accent = useAccent();
  const countLook = accent ? accentClasses[accent].tinted : "border-transparent bg-ink-muted/15 text-ink-body";

  const toggle = () => {
    if (!controlled) setInner(!open);
    onOpenChange?.(!open);
  };

  if (variant === "feature") {
    const look = featureTone[tone ?? "services"];
    return (
      <div
        className={cx("min-w-0 overflow-hidden rounded-ds-lg border border-l-[5px]", look.card, look.rail, className)}
        data-state={open ? "open" : "closed"}
        data-tone={tone ?? "services"}
        data-variant="feature"
        {...rest}
      >
        <button
          aria-controls={bodyId}
          aria-expanded={open}
          className={cx("flex min-h-[4.25rem] w-full touch-manipulation items-center gap-3 pl-4 pr-3 py-3.5 text-left transition-colors motion-reduce:transition-none", look.hover)}
          onClick={toggle}
          type="button"
        >
          <svg aria-hidden="true" className={cx("size-3.5 shrink-0 transition-transform duration-[140ms] motion-reduce:transition-none", look.sub, open && "rotate-90")} data-part="chevron" viewBox="0 0 12 12">
            <path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.75" />
          </svg>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className={cx("font-type-body-md text-lg font-bold", look.title)} data-part="title">{title}</span>
            {detail !== undefined && <span className="mt-0.5 min-w-0 truncate text-sm text-ink-body" data-part="detail" title={summaryText(detail)}>{detail}</span>}
          </span>
          {aside !== undefined && <span className="shrink-0" data-part="aside">{aside}</span>}
        </button>
        <div className={cx("grid transition-[grid-template-rows,opacity] duration-500 ease-out motion-reduce:transition-none", open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0")} data-part="region" id={bodyId}>
          <div className="min-h-0 overflow-hidden" inert={!open}>
            <div className={cx("border-t p-4", look.rule)} data-part="body">{children}</div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div data-variant={variant} data-tone={advanced ? (tone ?? "services") : undefined} data-state={open ? "open" : "closed"} className={cx("min-w-0", !advanced && "border-t border-dashed border-edge-subtle pt-2", className)} {...rest}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={bodyId}
        onClick={toggle}
        className={cx(
          "flex min-h-ds-touch-target w-full touch-manipulation items-center gap-3 rounded-ds-lg border px-3 text-left font-type-body-md text-sm font-semibold transition-colors motion-reduce:transition-none",
          advanced
            ? cx("border-edge", palette?.row)
            : "border-transparent bg-transparent px-1 text-ink-body can-hover:bg-surface-900/40 can-hover:text-ink",
        )}
      >
        <svg
          aria-hidden="true"
          data-part="chevron"
          viewBox="0 0 12 12"
          className={cx("size-3 shrink-0 transition-transform duration-[140ms] motion-reduce:transition-none", open && "rotate-90")}
        >
          <path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        <span className="shrink-0">{title}</span>
        {!advanced && count !== undefined ? (
          <span
            data-part="count"
            className={cx("rounded-ds-full border px-2 py-0.5 text-xs font-bold tabular-nums", countLook)}
          >
            {count} {count === 1 ? countNoun : `${countNoun}s`}
          </span>
        ) : null}
        {!open && summary !== undefined ? (
          <span data-part="summary" title={summaryText(summary)} className="ml-auto min-w-0 truncate text-right text-sm font-normal">
            {summary}
          </span>
        ) : null}
      </button>
      <div
        id={bodyId}
        data-part="region"
        className={cx(
          "grid transition-[grid-template-rows,opacity] duration-500 ease-out motion-reduce:transition-none",
          open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
        )}
      >
        <div inert={!open} className="min-h-0 overflow-hidden">
          <div
            data-part={advanced ? "bar" : "rule"}
            className={cx(
              "mt-2 p-ds-lg",
              advanced ? cx("border-l-[3px]", palette?.bar) : "border-t border-dashed border-edge-subtle",
            )}
          >
            {children}
          </div>
        </div>
      </div>
    </div>
  );
}
