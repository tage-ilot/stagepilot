import { useId, useState, type ComponentPropsWithoutRef, type ReactNode } from "react";

import { accentClasses, useAccent } from "./accent";
import { cx } from "./cx";

export type DisclosureVariant = "advanced" | "activity";
export type DisclosureTone = "services" | "presentation" | "playback" | "lighting" | "backend";

export interface DisclosureProps extends Omit<ComponentPropsWithoutRef<"div">, "title" | "children"> {
  variant: DisclosureVariant;
  /** Required for `advanced` (picks the advanced-* colour); ignored by `activity`, which is always neutral. */
  tone?: DisclosureTone;
  title: ReactNode;
  /** One-line summary shown on the right ONLY while closed, e.g. `Port 8080 · Channel 1`. Truncates; full text in title. */
  summary?: ReactNode;
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
  lighting: { row: "bg-advanced-lighting/10 text-advanced-lighting can-hover:bg-advanced-lighting/20", bar: "border-advanced-lighting" },
  backend: { row: "bg-advanced-backend/10 text-advanced-backend can-hover:bg-advanced-backend/20", bar: "border-advanced-backend" },
};

function summaryText(summary: ReactNode) {
  return typeof summary === "string" || typeof summary === "number" ? String(summary) : undefined;
}

export function Disclosure({
  variant,
  tone,
  title,
  summary,
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
