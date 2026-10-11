import { forwardRef, type ComponentPropsWithoutRef } from "react";

import { cx } from "./cx";
import { joinIds, useFieldContext } from "./fieldContext";
import { fieldBase, fieldBorder, fieldState } from "./fieldStyles";

export interface InputProps extends ComponentPropsWithoutRef<"input"> {
  error?: boolean;
}

/** Always 44px. No validation, no paste handling: callers validate on blur. */
export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { error, id, className, disabled, "aria-describedby": describedBy, "aria-invalid": ariaInvalid, ...rest },
  ref,
) {
  const field = useFieldContext();
  const invalid = Boolean(error) || Boolean(field.invalid);
  const held = Boolean(field.held);
  const inert = Boolean(disabled);
  return (
    <input
      ref={ref}
      id={id ?? field.id}
      disabled={disabled}
      aria-invalid={invalid || ariaInvalid || undefined}
      aria-describedby={joinIds(describedBy, field.describedBy)}
      data-state={fieldState({ error: invalid, held, inert })}
      className={cx(fieldBase, fieldBorder({ error: invalid, held, inert }), className)}
      {...rest}
    />
  );
});
