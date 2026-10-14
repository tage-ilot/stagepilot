import { forwardRef, type ComponentPropsWithoutRef, type ElementType } from "react";

import { AccentContext } from "./accent";
import { cx } from "./cx";

export type PanelVariant = "default" | "setup" | "live";

export type PanelIdentity = "services" | "presentation" | "playback" | "midi" | "lighting" | "backend";

// Identity = a 1px top border in the -soft role (never a rail or wash; the setup wash stays primary-derived).
const identityTop: Record<PanelIdentity, string> = {
  services: "border-t-integration-services-soft",
  presentation: "border-t-integration-presentation-soft",
  playback: "border-t-integration-playback-soft",
  midi: "border-t-integration-playback-midi-soft",
  lighting: "border-t-integration-lighting-soft",
  backend: "border-t-integration-backend-soft",
};

export interface PanelProps extends ComponentPropsWithoutRef<"section"> {
  variant?: PanelVariant;
  /** Element to render. Nothing else is polymorphic. */
  as?: "section" | "div" | "article";
  /** Adds overscroll-contain so a scrolling panel never chains scroll to the page. */
  scrollable?: boolean;
  /** Connection identity: tints the top border and the setup wash. */
  tone?: PanelIdentity;
  /** Colour of the controls inside, when it differs from the panel's own identity (default: the same as `tone`). */
  accent?: PanelIdentity;
}

// Fills: the generated colour tokens are opaque; the app draws the panel at 94% (live: 90%) alpha so the
// film-flare background shows through faintly, hence the opacity modifiers.
// Class strings are written out in full so Tailwind can see them.
const variants: Record<PanelVariant, string> = {
  default: "bg-surface-panel/[0.94] p-ds-lg",
  // 120deg primary-red-to-clear wash, 0% to 36% (DESIGN.md Components > Panel). No !important.
  setup:
    "bg-surface-panel/[0.94] p-ds-xl bg-[linear-gradient(120deg,var(--tw-gradient-stops))] from-0% to-transparent to-[36%]",
  // 135deg primary-red wash fading out by 42%, plus a primary-red left rail drawn by ::before (DESIGN.md Components > Live panel).
  live:
    "relative isolate bg-surface-panel-strong/90 p-ds-lg bg-[linear-gradient(135deg,var(--tw-gradient-stops))] from-primary/10 from-0% to-transparent to-[42%] before:absolute before:inset-y-0 before:left-0 before:-z-10 before:w-[5px] before:rounded-l-ds-xl before:bg-primary before:content-['']",
};

// The setup wash is the connection's own colour when the panel has one (DESIGN.md "Accent inside a configuration panel").
const setupWash: Record<PanelIdentity, string> = {
  services: "from-integration-services/[0.13]",
  presentation: "from-integration-presentation/[0.13]",
  playback: "from-integration-playback/[0.13]",
  midi: "from-integration-playback-midi/[0.13]",
  lighting: "from-integration-lighting/[0.13]",
  backend: "from-integration-backend/[0.13]",
};

export const Panel = forwardRef<HTMLElement, PanelProps>(function Panel(
  { variant = "default", as = "section", scrollable = false, tone, accent, className, ...rest },
  ref,
) {
  const Element = as as ElementType;
  const panel = (
    <Element
      ref={ref}
      data-variant={variant}
      data-scrollable={scrollable ? "true" : undefined}
      className={cx(
        "rounded-ds-xl border border-edge shadow-elevation-default",
        variants[variant],
        scrollable && "overscroll-contain",
        tone && identityTop[tone],
        variant === "setup" && (tone ? setupWash[tone] : "from-primary/[0.13]"),
        className,
      )}
      {...rest}
    />
  );
  const inside = accent ?? tone;
  return inside ? <AccentContext.Provider value={inside}>{panel}</AccentContext.Provider> : panel;
});
