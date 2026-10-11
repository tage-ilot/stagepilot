import { forwardRef, type ComponentPropsWithoutRef, type MouseEvent } from "react";

import { cx } from "./cx";
import type { LiveTone, Tone } from "./types";

export type ButtonVariant = "primary" | "secondary" | "tinted" | "destructive" | "destructive-soft" | "live";
export type ButtonSize = "control" | "touch";

export interface ButtonProps extends ComponentPropsWithoutRef<"button"> {
  variant?: ButtonVariant;
  /** Tinted only. Defaults to info. */
  tone?: Tone;
  /** Live only (required there). */
  liveTone?: LiveTone;
  size?: ButtonSize;
  loading?: boolean;
  /** Replaces the label while loading, e.g. "Saving…". */
  loadingLabel?: string;
  fullWidth?: boolean;
}

// Class strings are written out in full so Tailwind can see them.
const tinted: Record<Tone, string> = {
  services: "border-integration-services/30 bg-integration-services/10 text-integration-services-text can-hover:bg-integration-services/20",
  presentation: "border-integration-presentation/30 bg-integration-presentation/10 text-integration-presentation-text can-hover:bg-integration-presentation/20",
  playback: "border-integration-playback/30 bg-integration-playback/10 text-integration-playback-text can-hover:bg-integration-playback/20",
  lights: "border-integration-lights/30 bg-integration-lights/10 text-info-text can-hover:bg-integration-lights/20",
  success: "border-success/30 bg-success/10 text-success-text can-hover:bg-success/20",
  warning: "border-warning/30 bg-warning/10 text-warning-text can-hover:bg-warning/20",
  danger: "border-danger/30 bg-danger/10 text-danger-text can-hover:bg-danger/20",
  info: "border-info/30 bg-info/10 text-info-text can-hover:bg-info/20",
  idle: "border-idle/30 bg-idle/10 text-ink-muted can-hover:bg-idle/20",
};

// Rest: transparent on surface-950, light text (label shadow). Go/back flip to dark text on hover/press, so the shadow is removed there.
const live: Record<LiveTone, string> = {
  go: "border-live-go/40 text-live-go-text can-hover:border-live-go/70 can-hover:bg-live-go can-hover:text-on-primary can-hover:[text-shadow:none] active:bg-live-go/80 active:text-on-primary active:[text-shadow:none]",
  back: "border-live-back/40 text-live-back-text can-hover:border-live-back/70 can-hover:bg-live-back can-hover:text-on-primary can-hover:[text-shadow:none] active:bg-live-back/80 active:text-on-primary active:[text-shadow:none]",
  reload: "border-live-reload/50 text-live-reload-text can-hover:border-live-reload/70 can-hover:bg-live-reload can-hover:text-on-danger active:bg-live-reload/80 active:text-on-danger",
  stop: "border-live-stop/40 text-live-stop-text can-hover:border-live-stop/70 can-hover:bg-live-stop can-hover:text-on-danger active:bg-live-stop/80 active:text-on-danger",
};

const base =
  "inline-flex items-center justify-center rounded-ds-lg border text-center font-type-label text-type-label touch-manipulation transition-colors duration-[140ms] motion-reduce:transition-none break-words";

const variants: Record<Exclude<ButtonVariant, "tinted" | "live">, string> = {
  primary: "border-transparent bg-primary text-on-primary can-hover:bg-primary-hover active:bg-primary-focus",
  secondary: "border-edge-medium bg-surface-950 text-ink-body text-shadow-label can-hover:bg-surface-900 active:bg-surface-900",
  destructive: "border-transparent bg-danger-solid text-on-danger text-shadow-label can-hover:bg-danger active:bg-danger",
  "destructive-soft": "border-transparent bg-danger-soft text-on-danger-soft can-hover:bg-danger active:bg-danger",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = "secondary",
    tone,
    liveTone,
    size = "control",
    loading = false,
    loadingLabel,
    fullWidth = false,
    type = "button",
    disabled,
    className,
    children,
    onClick,
    ...rest
  },
  ref,
) {
  const isLive = variant === "live";
  const resolvedTone = variant === "tinted" ? (tone ?? "info") : undefined;
  const inert = Boolean(disabled) || loading;

  let look: string;
  if (isLive) look = cx("bg-surface-950 px-4 py-2.5 min-h-ds-touch-target text-shadow-label", liveTone && live[liveTone]);
  else if (variant === "tinted") look = cx("text-shadow-label", tinted[resolvedTone!]);
  else look = variants[variant];

  // Hover/press styling is dropped while inert so a disabled or busy control never looks live.
  if (inert) look = look.split(" ").filter((c) => !c.startsWith("can-hover:") && !c.startsWith("active:")).join(" ");

  const handleClick = (event: MouseEvent<HTMLButtonElement>) => {
    if (loading) {
      event.preventDefault();
      return;
    }
    onClick?.(event);
  };

  return (
    <button
      ref={ref}
      type={type}
      disabled={disabled}
      aria-disabled={inert || undefined}
      aria-busy={loading || undefined}
      data-variant={variant}
      data-tone={isLive ? liveTone : resolvedTone}
      data-size={isLive ? "touch" : size}
      data-loading={loading ? "true" : undefined}
      onClick={handleClick}
      className={cx(
        base,
        !isLive && "px-3.5 py-2",
        !isLive && (size === "touch" ? "min-h-ds-touch-target" : "min-h-ds-control coarse:min-h-ds-touch-target"),
        look,
        fullWidth && "w-full",
        disabled && "cursor-not-allowed opacity-40",
        loading && !disabled && "cursor-wait",
        className,
      )}
      {...rest}
    >
      {loading && loadingLabel ? loadingLabel : children}
    </button>
  );
});
