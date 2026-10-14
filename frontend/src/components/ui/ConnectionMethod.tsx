import { useId, useLayoutEffect, useRef, useState } from "react";

import { accentClasses, useAccent } from "./accent";
import { cx } from "./cx";

export interface ConnectionMethodOption<T extends string = string> {
  value: T;
  /** The name of the way to connect: "Playback API", "MIDI". */
  label: string;
  /** Primary is the normal way, alternate the fallback. Only the word under the name differs. */
  role: "primary" | "alternate";
  /** Greyed out and not choosable (for example a legacy value that is only shown). */
  disabled?: boolean;
  /** Optional own tint for the chosen look, as a full Tailwind class string (for example the MIDI method's blue-violet). */
  chip?: string;
}

export interface ConnectionMethodProps<T extends string = string> {
  options: ConnectionMethodOption<T>[];
  value: T;
  onChange: (value: T) => void;
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
 * ALTERNATE word. Whichever is chosen, primary or alternate, wears the connection's tint; the toggle itself
 * says which is active, so there is no extra notice. Native radio buttons, so arrow keys, the group
 * label and the 44px target come from the browser. Never build a second method picker.
 */
export function ConnectionMethod<T extends string = string>({
  options, value, onChange, disabled = false, heldChange = false,
  label = "Connection method", className,
}: ConnectionMethodProps<T>) {
  const uid = useId();
  const accent = useAccent();
  // The chosen option, primary or alternate, wears the connection's tint (the primary red outside a panel).
  const chosenChip = accent ? accentClasses[accent].chip : "border-primary bg-primary/45";
  const heldId = heldChange ? `${uid}-held` : undefined;

  // The chosen highlight is one box that slides between the options (it never disappears and reappears).
  // It is measured from the chosen label, so it follows wrapping, resizing and font loading.
  const group = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const [slides, setSlides] = useState(false);
  useLayoutEffect(() => {
    const root = group.current;
    if (!root) return;
    const measure = () => {
      const chosen = root.querySelector<HTMLElement>('[data-selected="true"]');
      if (!chosen) { setPill(null); return; }
      setPill((previous) => {
        const next = { left: chosen.offsetLeft, top: chosen.offsetTop, width: chosen.offsetWidth, height: chosen.offsetHeight };
        return previous && previous.left === next.left && previous.top === next.top && previous.width === next.width && previous.height === next.height ? previous : next;
      });
    };
    measure();
    const frame = requestAnimationFrame(() => setSlides(true));
    if (typeof ResizeObserver === "undefined") return () => cancelAnimationFrame(frame);
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    root.querySelectorAll("label").forEach((label) => observer.observe(label));
    return () => { cancelAnimationFrame(frame); observer.disconnect(); };
  }, [value, options.length]);
  const chosenOption = options.find((option) => option.value === value);
  const pillLook = chosenOption?.chip ?? chosenChip;

  return (
    <fieldset className={cx("min-w-0", className)} data-connection-method data-state={heldChange ? "held" : "default"} disabled={disabled}>
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        {/* min-h matches the 18px "Not saved yet" marker, so the header row (and the switch below it) never moves when that marker appears. */}
        <legend className="mt-6 min-h-[18px] p-0 font-type-kicker text-type-kicker uppercase text-ink-muted">{label}</legend>
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
          "relative mt-2 inline-flex max-w-full rounded-ds-xl border bg-surface-950 p-[3px] gap-[3px]",
          heldChange ? "border-warning" : "border-edge",
          disabled && "opacity-60",
        )}
        ref={group}
        role="radiogroup"
        aria-label={label}
      >
        {pill && (
          <span
            aria-hidden="true"
            className={cx(
              "pointer-events-none absolute rounded-ds-lg border",
              slides && "transition-[left,top,width,height,background-color,border-color] duration-[220ms] ease-[cubic-bezier(0.2,0.8,0.2,1)] motion-reduce:transition-none",
              pillLook,
            )}
            data-method-pill
            style={{ left: pill.left, top: pill.top, width: pill.width, height: pill.height }}
          />
        )}
        {options.map((option) => {
          const on = option.value === value;
          return (
            <label
              key={option.value}
              className={cx(
                "relative z-10 flex min-h-ds-touch-target min-w-0 touch-manipulation flex-col justify-center rounded-ds-lg border border-transparent px-4 py-1 transition-colors duration-[140ms] motion-reduce:transition-none",
                "has-[:focus-visible]:outline has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-ink",
                disabled || option.disabled ? "cursor-not-allowed" : "cursor-pointer",
                on
                  ? "text-ink"
                  : "text-ink-body can-hover:bg-surface-900/60 can-hover:text-ink",
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
              <span id={`${uid}-${option.value}-role`} className="font-type-badge text-[0.6875rem] uppercase tracking-[0.1em] text-ink-muted">
                {roleWord[option.role]}
              </span>
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
