import type { ComponentPropsWithoutRef } from "react";

import { cx } from "./cx";
import type { Tone } from "./types";

export interface StatusDotProps extends Omit<ComponentPropsWithoutRef<"span">, "children"> {
  tone: Tone;
  size?: "sm" | "md";
  /** Makes the dot meaningful to assistive tech. Without it the dot is decorative; a word must carry the status. */
  label?: string;
}

const dotTone: Record<Tone, string> = {
  services: "bg-integration-services",
  presentation: "bg-integration-presentation",
  playback: "bg-integration-playback",
  lights: "bg-integration-lights",
  success: "bg-success",
  warning: "bg-warning",
  danger: "bg-danger",
  info: "bg-info",
  idle: "bg-idle",
};

const sizes = { sm: "size-2", md: "size-2.5" } as const;

/** Static by design: no pulse or animation, so a dot never fakes liveness. */
export function StatusDot({ tone, size = "sm", label, className, ...rest }: StatusDotProps) {
  const a11y = label ? { role: "img", "aria-label": label } : { "aria-hidden": true as const };
  return (
    <span
      data-tone={tone}
      data-size={size}
      className={cx("inline-block shrink-0 rounded-ds-full", sizes[size], dotTone[tone], className)}
      {...a11y}
      {...rest}
    />
  );
}
