import type { ComponentPropsWithoutRef } from "react";

import { cx } from "./cx";
import { useAccent } from "./accent";
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
  /** Keep the real status colour (green for success) inside a connection panel instead of the connection's accent. */
  ignoreAccent?: boolean;
}

const tinted: Record<Tone, string> = {
  services: "border-integration-services-soft/30 bg-integration-services-soft/10 text-integration-services-text",
  presentation: "border-integration-presentation-soft/30 bg-integration-presentation-soft/10 text-integration-presentation-text",
  playback: "border-integration-playback-soft/30 bg-integration-playback-soft/10 text-integration-playback-text",
  midi: "border-integration-playback-midi-soft/30 bg-integration-playback-midi-soft/10 text-integration-playback-midi-text",
  lighting: "border-integration-lighting-soft/30 bg-integration-lighting-soft/10 text-integration-lighting-text",
  backend: "border-integration-backend-soft/30 bg-integration-backend-soft/10 text-integration-backend-text",
  success: "border-success-soft/40 bg-success-soft/10 text-success-text",
  warning: "border-warning/30 bg-warning/10 text-warning-text",
  danger: "border-danger/30 bg-danger/10 text-danger-text",
  info: "border-info/30 bg-info/10 text-info-text",
  idle: "border-idle/30 bg-idle/10 text-ink-muted",
};

// Dark label text gets no shadow; light label text on danger-solid gets text-shadow-label (D5).
const solid: Partial<Record<Tone, string>> = {
  success: "border-transparent bg-success text-ink text-shadow-label",
  warning: "border-transparent bg-warning text-on-light",
  danger: "border-transparent bg-danger-solid text-on-danger text-shadow-label",
};

// Tinted badges use light tone text on a dark tinted fill, so they carry the label shadow.
const base =
  "inline-flex items-center gap-1.5 rounded-ds-full border px-3 py-1.5 font-type-badge text-type-badge font-bold uppercase tracking-wider";

export function Badge({ tone, emphasis = "tinted", dot = false, ignoreAccent = false, className, children, ...rest }: BadgeProps) {
  const panelAccent = useAccent();
  const accent = ignoreAccent ? null : panelAccent;
  // Inside a connection's panel a "good" badge wears the connection's colour instead of green.
  const effectiveTone: Tone = accent && tone === "success" ? accent : tone;
  const solidLook = emphasis === "solid" && !(accent && tone === "success") ? solid[tone] : undefined;
  const look = solidLook ?? cx(tinted[effectiveTone], "text-shadow-label");
  return (
    <span
      data-tone={effectiveTone}
      data-emphasis={solidLook ? "solid" : "tinted"}
      className={cx(base, look, className)}
      {...rest}
    >
      {dot && <StatusDot tone={effectiveTone} ignoreAccent={ignoreAccent} />}
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
    <Badge tone={statusTone[status]} dot ignoreAccent data-status={status} {...rest}>
      {status}
    </Badge>
  );
}
