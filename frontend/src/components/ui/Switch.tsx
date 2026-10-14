import type { ComponentPropsWithoutRef } from "react";

import { useAccent, accentClasses } from "./accent";
import { cx } from "./cx";

export interface SwitchProps extends Omit<ComponentPropsWithoutRef<"input">, "type" | "role" | "children"> {
  /** The visible word beside the switch. Give `aria-label` too when the word alone is not enough ("Skip So Much"). */
  label: string;
}

/**
 * A small on/off switch with a word beside it (DESIGN.md "Switch"). A real checkbox with role="switch", so keyboard and
 * screen readers work. The whole row is a 44px target. On wears the connection colour inside a configuration panel.
 */
export function Switch({ label, className, disabled, ...rest }: SwitchProps) {
  const accent = useAccent();
  const on = accent ? accentClasses[accent].switchOn : "peer-checked:border-primary peer-checked:bg-primary";
  return (
    <label className={cx("relative inline-flex min-h-ds-touch-target touch-manipulation select-none items-center gap-2.5 text-sm text-ink-body", disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer", className)}>
      <input className="peer sr-only" disabled={disabled} role="switch" type="checkbox" {...rest} />
      <span aria-hidden="true" className={cx("relative h-6 w-[2.625rem] flex-none rounded-full border border-ink-muted/60 bg-ink/10 transition-colors duration-[140ms] motion-reduce:transition-none after:absolute after:left-0.5 after:top-0.5 after:h-[1.125rem] after:w-[1.125rem] after:rounded-full after:bg-ink-body after:transition-transform after:duration-[140ms] motion-reduce:after:transition-none peer-checked:after:translate-x-[1.125rem] peer-checked:after:bg-ink peer-focus-visible:outline peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-ink", on)} />
      <span>{label}</span>
    </label>
  );
}
