import { forwardRef, type ComponentPropsWithoutRef, type ElementType } from "react";

import { cx } from "./cx";

export type PanelVariant = "default" | "setup" | "live";

export interface PanelProps extends ComponentPropsWithoutRef<"section"> {
  variant?: PanelVariant;
  /** Element to render. Nothing else is polymorphic. */
  as?: "section" | "div" | "article";
  /** Adds overscroll-contain so a scrolling panel never chains scroll to the page. */
  scrollable?: boolean;
}

// Fills: the generated colour tokens are opaque; the app draws the panel at 94% (live: 90%) alpha so the
// film-flare background shows through faintly, hence the opacity modifiers.
// Class strings are written out in full so Tailwind can see them.
const variants: Record<PanelVariant, string> = {
  default: "bg-surface-panel/[0.94] p-ds-lg",
  // 120deg coral-to-clear wash, 0% to 36% (DESIGN.md Components > Panel). No !important.
  setup:
    "bg-surface-panel/[0.94] p-ds-xl bg-[linear-gradient(120deg,var(--tw-gradient-stops))] from-primary/[0.13] from-0% to-transparent to-[36%]",
  // 135deg coral wash fading out by 42%, plus a coral left rail drawn by ::before (DESIGN.md Components > Live panel).
  live:
    "relative isolate bg-surface-panel-strong/90 p-ds-lg bg-[linear-gradient(135deg,var(--tw-gradient-stops))] from-primary/10 from-0% to-transparent to-[42%] before:absolute before:inset-y-0 before:left-0 before:-z-10 before:w-[5px] before:rounded-l-ds-xl before:bg-primary before:content-['']",
};

export const Panel = forwardRef<HTMLElement, PanelProps>(function Panel(
  { variant = "default", as = "section", scrollable = false, className, ...rest },
  ref,
) {
  const Element = as as ElementType;
  return (
    <Element
      ref={ref}
      data-variant={variant}
      data-scrollable={scrollable ? "true" : undefined}
      className={cx(
        "rounded-ds-xl border border-edge shadow-elevation-default",
        variants[variant],
        scrollable && "overscroll-contain",
        className,
      )}
      {...rest}
    />
  );
});
