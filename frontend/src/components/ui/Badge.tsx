import type { ComponentPropsWithoutRef } from "react";

import { cx } from "./cx";
import { statusTone, type StatusWord } from "./statusWord";
import { StatusDot } from "./StatusDot";
import type { Tone } from "./types";

export type BadgeEmphasis = "tinted" | "solid";

export interface BadgeProps extends ComponentPropsWithoutRef<"span"> {
  tone: Tone;
  /** Solid exists only for success, warning and danger (DESIGN.md badge-*). Other tones fall back to tinted. */
  emphasis?: BadgeEmphasis;
  /** Leading status dot in the same tone. */
  dot?: boolean;
}

const tinted: Record<Tone, string> = {
  services: "border-integration-services/30 bg-integration-services/10 text-integration-services-text",
  presentation: "border-integration-presentation/30 bg-integration-presentation/10 text-integration-presentation-text",
  playback: "border-integration-playback/30 bg-integration-playback/10 text-integration-playback-text",
  lights: "border-integration-lights/30 bg-integration-lights/10 text-info-text",
  success: "border-success/30 bg-success/10 text-success-text",
  warning: "border-warning/30 bg-warning/10 text-warning-text",
  danger: "border-danger/30 bg-danger/10 text-danger-text",
  info: "border-info/30 bg-info/10 text-info-text",
  idle: "border-idle/30 bg-idle/10 text-ink-muted",
};

// Dark label text gets no shadow; light label text on danger-solid gets text-shadow-label (D5).
const solid: Partial<Record<Tone, string>> = {
  success: "border-transparent bg-success text-on-primary",
  warning: "border-transparent bg-warning text-on-primary",
  danger: "border-transparent bg-danger-solid text-on-danger text-shadow-label",
};

// Tinted badges use light tone text on a dark tinted fill, so they carry the label shadow.
const base =
  "inline-flex items-center gap-1.5 rounded-ds-full border px-3 py-1.5 font-type-badge text-type-badge font-bold uppercase tracking-wider";

export function Badge({ tone, emphasis = "tinted", dot = false, className, children, ...rest }: BadgeProps) {
  const solidLook = emphasis === "solid" ? solid[tone] : undefined;
  const look = solidLook ?? cx(tinted[tone], "text-shadow-label");
  return (
    <span
      data-tone={tone}
      data-emphasis={solidLook ? "solid" : "tinted"}
      className={cx(base, look, className)}
      {...rest}
    >
      {dot && <StatusDot tone={tone} />}
      {children}
    </span>
  );
}

export interface StatusBadgeProps extends Omit<BadgeProps, "tone" | "children" | "dot"> {
  status: StatusWord;
}

/** Sentence case in the DOM (what assistive tech reads); the `uppercase` class does the capitals. */
export function StatusBadge({ status, ...rest }: StatusBadgeProps) {
  return (
    <Badge tone={statusTone[status]} dot data-status={status} {...rest}>
      {status}
    </Badge>
  );
}
