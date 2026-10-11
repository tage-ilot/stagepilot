import { useId, type ReactNode } from "react";

import { cx } from "./cx";
import type { Tone } from "./types";

/** DESIGN.md wording, typed as a union so callers cannot typo it. */
export type SegmentCaption = "Main" | "Alternate";

export interface SegmentedOption {
  value: string;
  label: string;
  caption?: SegmentCaption;
  disabled?: boolean;
}

export interface SegmentedControlProps {
  legend: ReactNode;
  /** Radio group name. Defaults to a generated id. */
  name?: string;
  value: string;
  onChange: (value: string) => void;
  options: SegmentedOption[];
  /** Service tone of the selected segment. */
  tone: Tone;
  disabled?: boolean;
  /** Message shown below; also marks the group invalid. */
  error?: ReactNode;
  className?: string;
}

// Selected: fill tone @ 15%, border tone, text *-text (D3 tone table). Written in full for Tailwind.
const selectedTone: Record<Tone, string> = {
  services: "border-integration-services bg-integration-services/15 text-integration-services-text",
  presentation: "border-integration-presentation bg-integration-presentation/15 text-integration-presentation-text",
  playback: "border-integration-playback bg-integration-playback/15 text-integration-playback-text",
  lights: "border-integration-lights bg-integration-lights/15 text-info-text",
  success: "border-success bg-success/15 text-success-text",
  warning: "border-warning bg-warning/15 text-warning-text",
  danger: "border-danger bg-danger/15 text-danger-text",
  info: "border-info bg-info/15 text-info-text",
  idle: "border-idle bg-idle/15 text-ink-muted",
};

const segmentBase =
  "flex h-full min-h-ds-touch-target w-full cursor-pointer select-none flex-col items-center justify-center gap-0.5 rounded-ds-lg border px-3 py-2 text-center font-type-text-strong text-type-text-strong touch-manipulation transition-colors duration-[140ms] motion-reduce:transition-none peer-focus-visible:outline peer-focus-visible:outline-2 peer-focus-visible:outline-offset-[3px] peer-focus-visible:outline-primary-focus peer-disabled:cursor-not-allowed peer-disabled:opacity-40";

/**
 * Native radio group: fieldset + visually hidden radios + labels as segments, so arrow keys, grouping and form
 * semantics are the browser's. Controlled. It swaps nothing itself; the caller shows the matching fields.
 * Selected is shown by fill, border, a check glyph and bold weight (never colour alone). 44px tall, segments wrap
 * to a stack when two long labels do not fit side by side.
 */
export function SegmentedControl({ legend, name, value, onChange, options, tone, disabled = false, error, className }: SegmentedControlProps) {
  const uid = useId();
  const groupName = name ?? `${uid}-segments`;
  const errorId = error ? `${uid}-error` : undefined;

  return (
    <fieldset
      data-segmented
      data-tone={tone}
      data-state={disabled ? "disabled" : error ? "error" : "default"}
      aria-describedby={errorId}
      aria-invalid={error ? true : undefined}
      disabled={disabled}
      className={cx("m-0 flex min-w-0 flex-col gap-1.5 border-0 p-0", className)}
    >
      <legend className="mb-1.5 min-w-0 break-words p-0 font-type-text-strong text-type-text-strong text-ink">{legend}</legend>
      <div className="flex flex-wrap gap-2">
        {options.map((option) => {
          const id = `${uid}-${option.value}`;
          const selected = option.value === value;
          const optionOff = disabled || option.disabled;
          return (
            <div key={option.value} data-segment data-state={selected ? "selected" : optionOff ? "disabled" : "default"} className="relative min-w-0 flex-1 basis-40">
              <input
                id={id}
                type="radio"
                name={groupName}
                value={option.value}
                checked={selected}
                disabled={optionOff}
                onChange={() => onChange(option.value)}
                className="peer sr-only"
              />
              <label
                htmlFor={id}
                className={cx(
                  segmentBase,
                  selected
                    ? cx(selectedTone[tone], "font-bold")
                    : cx(error ? "border-danger" : "border-edge", "bg-surface-950 text-ink-body", !optionOff && "can-hover:border-edge-medium can-hover:bg-surface-900"),
                )}
              >
                <span className="flex min-w-0 items-center gap-1.5 break-words">
                  {selected && (
                    <span aria-hidden="true" data-check>
                      ✓
                    </span>
                  )}
                  <span className="min-w-0 break-words">{option.label}</span>
                </span>
                {option.caption && <span className="text-type-body-sm font-normal uppercase tracking-wider opacity-80">{option.caption}</span>}
              </label>
            </div>
          );
        })}
      </div>
      {error && (
        <p id={errorId} role="alert" className="break-words text-type-body-sm text-danger-text">
          {error}
        </p>
      )}
    </fieldset>
  );
}
