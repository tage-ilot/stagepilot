import { useId, type ReactNode } from "react";

import { accentClasses, useAccent } from "./accent";
import { cx } from "./cx";

export interface ConnectionMethodOption<T extends string = string> {
  value: T;
  /** The name of the way to connect: "Playback API", "MIDI". */
  label: string;
  /** Primary is the normal way. Alternate is the fallback; choosing it shows the notice. */
  role: "primary" | "alternate";
  /** Greyed out and not choosable (for example a legacy value that is only shown). */
  disabled?: boolean;
}

export interface ConnectionMethodProps<T extends string = string> {
  options: ConnectionMethodOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Shown beneath the control while an alternate option is selected: how to get back. */
  alternateNotice?: ReactNode;
  disabled?: boolean;
  /** Visual marker only (warning dot, "Not saved yet"). Save logic lives elsewhere. */
  heldChange?: boolean;
  /** The kicker above the control. Default "Connection method". */
  label?: string;
  className?: string;
}

const roleWord = { primary: "Primary", alternate: "Alternate" } as const;

/**
 * THE template for choosing how a connection connects (DESIGN.md "Connection method"). Every connection that
 * has more than one way to connect (Playback: Playback API primary, MIDI alternate; Lighting, later) uses this
 * and nothing else: two or more side-by-side options, each a bold name over a tiny uppercase PRIMARY or
 * ALTERNATE word. The primary, when chosen, wears the connection's colour; an alternate, when chosen, is a
 * lighter slate chip and a neutral notice appears under it. Native radio buttons, so arrow keys, the group
 * label and the 44px target come from the browser. Never build a second method picker.
 */
export function ConnectionMethod<T extends string = string>({
  options, value, onChange, alternateNotice, disabled = false, heldChange = false,
  label = "Connection method", className,
}: ConnectionMethodProps<T>) {
  const uid = useId();
  const accent = useAccent();
  const primaryChip = accent ? accentClasses[accent].chip : "border-primary/60 bg-primary/20";
  const heldId = heldChange ? `${uid}-held` : undefined;
  const selected = options.find((option) => option.value === value);
  const showNotice = selected?.role === "alternate" && alternateNotice !== undefined;

  return (
    <fieldset className={cx("min-w-0", className)} data-connection-method data-state={heldChange ? "held" : "default"} disabled={disabled}>
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <legend className="mt-6 p-0 font-type-kicker text-type-kicker uppercase text-ink-muted">{label}</legend>
        {heldChange && (
          <span id={heldId} data-held-marker className="mt-6 inline-flex items-center gap-1.5 text-type-body-sm text-warning-text">
            <span aria-hidden="true" className="h-2 w-2 rounded-ds-full bg-warning" />
            Not saved yet
          </span>
        )}
      </div>
      <div
        aria-describedby={heldId}
        className={cx(
          "mt-2 inline-flex max-w-full rounded-ds-xl border bg-surface-950 p-[3px] gap-[3px]",
          heldChange ? "border-warning" : "border-edge",
          disabled && "opacity-60",
        )}
        role="radiogroup"
        aria-label={label}
      >
        {options.map((option) => {
          const on = option.value === value;
          return (
            <label
              key={option.value}
              className={cx(
                "relative flex min-h-ds-touch-target min-w-0 touch-manipulation flex-col justify-center rounded-ds-lg border px-4 py-1 transition-colors duration-[140ms] motion-reduce:transition-none",
                "has-[:focus-visible]:outline has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-ink",
                disabled || option.disabled ? "cursor-not-allowed" : "cursor-pointer",
                on
                  ? option.role === "alternate"
                    ? "border-edge-medium bg-ink-muted/15 text-ink"
                    : cx(primaryChip, "text-ink")
                  : "border-transparent text-ink-body can-hover:bg-surface-900/60 can-hover:text-ink",
              )}
              data-role={option.role}
              data-selected={on ? "true" : "false"}
            >
              <input
                checked={on}
                className="absolute inset-0 size-full cursor-[inherit] opacity-0"
                disabled={disabled || option.disabled}
                name={uid}
                aria-labelledby={`${uid}-${option.value}-name ${uid}-${option.value}-role`}
                onChange={() => onChange(option.value)}
                type="radio"
                value={option.value}
              />
              <span className="font-type-text-strong text-type-text-strong" id={`${uid}-${option.value}-name`}>{option.label}</span>
              <span id={`${uid}-${option.value}-role`} className={cx("font-type-badge text-[0.6875rem] uppercase tracking-[0.1em]", on && option.role === "alternate" ? "text-ink-body" : "text-ink-muted")}>
                {roleWord[option.role]}
              </span>
            </label>
          );
        })}
      </div>
      {showNotice && (
        <p className="mt-3 rounded-ds-lg border border-edge-medium bg-ink-muted/10 px-3 py-2 text-sm text-ink-body" data-alternate-notice>
          {alternateNotice}
        </p>
      )}
    </fieldset>
  );
}
