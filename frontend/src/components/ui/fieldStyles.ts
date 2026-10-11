import { cx } from "./cx";

// Shared look for Input, Select and Textarea. Written out in full so Tailwind can see every class.
export const fieldBase =
  "block w-full min-h-ds-touch-target rounded-ds-lg border bg-surface-950 px-3 py-2.5 font-type-body-md text-type-body-md text-ink placeholder:text-ink-muted touch-manipulation transition-colors duration-[140ms] motion-reduce:transition-none disabled:cursor-not-allowed disabled:opacity-40";

export function fieldBorder({ error, held, inert }: { error: boolean; held: boolean; inert: boolean }) {
  if (error) return "border-danger";
  if (held) return cx("border-warning", !inert && "can-hover:border-warning");
  return cx("border-edge", !inert && "can-hover:border-edge-medium");
}

export function fieldState({ error, held, inert, loading }: { error: boolean; held: boolean; inert: boolean; loading?: boolean }) {
  if (loading) return "loading";
  if (inert) return "disabled";
  if (error) return "error";
  if (held) return "held";
  return "default";
}
