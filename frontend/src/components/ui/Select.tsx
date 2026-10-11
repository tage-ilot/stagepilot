import { forwardRef, useState, type ChangeEvent, type ComponentPropsWithoutRef } from "react";

import { cx } from "./cx";
import { joinIds, useFieldContext } from "./fieldContext";
import { fieldBase, fieldBorder, fieldState } from "./fieldStyles";

export interface SelectProps extends ComponentPropsWithoutRef<"select"> {
  error?: boolean;
  /** Leading option shown muted until a real value is chosen (disabled, hidden in the list). */
  placeholder?: string;
  loading?: boolean;
  /** First option text while loading, e.g. "Loading…". Supplied by the caller. */
  loadingLabel?: string;
}

/** Native select, same look as Input. Options are plain children; the chevron is a sibling SVG in currentColor. */
export const Select = forwardRef<HTMLSelectElement, SelectProps>(function Select(
  {
    error,
    placeholder,
    loading = false,
    loadingLabel,
    id,
    className,
    disabled,
    children,
    value,
    defaultValue,
    onChange,
    "aria-describedby": describedBy,
    "aria-invalid": ariaInvalid,
    ...rest
  },
  ref,
) {
  const field = useFieldContext();
  const invalid = Boolean(error) || Boolean(field.invalid);
  const held = Boolean(field.held);
  const inert = Boolean(disabled) || loading;
  const controlled = value !== undefined;
  const [inner, setInner] = useState(defaultValue ?? "");
  const current = controlled ? value : inner;
  const showingPlaceholder = Boolean(placeholder) && !loading && (current === "" || current === undefined);

  const handleChange = (event: ChangeEvent<HTMLSelectElement>) => {
    if (!controlled) setInner(event.target.value);
    onChange?.(event);
  };

  return (
    <div className={cx("relative", inert && !loading && "opacity-40", loading && "cursor-wait")}>
      <select
        ref={ref}
        id={id ?? field.id}
        disabled={inert}
        aria-busy={loading || undefined}
        aria-invalid={invalid || ariaInvalid || undefined}
        aria-describedby={joinIds(describedBy, field.describedBy)}
        data-state={fieldState({ error: invalid, held, inert: Boolean(disabled), loading })}
        data-loading={loading ? "true" : undefined}
        data-placeholder={showingPlaceholder ? "true" : undefined}
        key={loading ? "loading" : "ready"}
        {...(loading ? { value: "" } : controlled ? { value } : { defaultValue: defaultValue ?? (placeholder ? "" : undefined) })}
        onChange={handleChange}
        className={cx(
          fieldBase,
          "appearance-none truncate pr-10 disabled:opacity-100",
          showingPlaceholder && "text-ink-muted",
          fieldBorder({ error: invalid, held, inert }),
          className,
        )}
        {...rest}
      >
        {loading && <option value="">{loadingLabel ?? "Loading…"}</option>}
        {!loading && placeholder && (
          <option value="" disabled hidden>
            {placeholder}
          </option>
        )}
        {children}
      </select>
      <svg
        aria-hidden="true"
        viewBox="0 0 20 20"
        className="pointer-events-none absolute right-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink-muted"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M5 8l5 5 5-5" />
      </svg>
    </div>
  );
});
