import { forwardRef, type ComponentPropsWithoutRef } from "react";

import { cx } from "./cx";
import { joinIds, useFieldContext } from "./fieldContext";
import { fieldBase, fieldBorder, fieldState } from "./fieldStyles";

export interface TextareaProps extends ComponentPropsWithoutRef<"textarea"> {
  error?: boolean;
}

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea(
  { error, id, className, disabled, "aria-describedby": describedBy, "aria-invalid": ariaInvalid, ...rest },
  ref,
) {
  const field = useFieldContext();
  const invalid = Boolean(error) || Boolean(field.invalid);
  const held = Boolean(field.held);
  const inert = Boolean(disabled);
  return (
    <textarea
      ref={ref}
      id={id ?? field.id}
      disabled={disabled}
      aria-invalid={invalid || ariaInvalid || undefined}
      aria-describedby={joinIds(describedBy, field.describedBy)}
      data-state={fieldState({ error: invalid, held, inert })}
      className={cx(fieldBase, "min-h-[5.5rem] resize-y", fieldBorder({ error: invalid, held, inert }), className)}
      {...rest}
    />
  );
});
