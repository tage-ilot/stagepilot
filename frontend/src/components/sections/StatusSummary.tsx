import type { ReactNode } from "react";

import { accentClasses, useAccent } from "../ui/accent";
import { cx } from "../ui/cx";

/** What the value's words say, by words AND colour (a colour alone never carries it): `ok` wears the connection's accent. */
export type SummaryTone = "ok" | "warn" | "danger";

export interface SummaryItem {
  label: ReactNode;
  value: ReactNode;
  tone?: SummaryTone;
  /** A second, quieter line under the value (the WebSocket's address). */
  detail?: ReactNode;
  /** Draw the value in the monospace face and break it anywhere (an address). */
  mono?: boolean;
  /** Numbers and times line up. */
  tabular?: boolean;
  testId?: string;
}

export interface StatusSummaryProps {
  items: SummaryItem[];
  /** `list`: label and words in columns under a rule (the bottom of a page). `cards`: each fact in its own small recessed box (Backend's Status at the top). */
  variant?: "list" | "cards";
  /** Names the list for a screen reader (`Status`). */
  label?: string;
  className?: string;
  testId?: string;
}

const tones: Record<Exclude<SummaryTone, "ok">, string> = { warn: "text-warning-text", danger: "text-danger-text" };

/**
 * The SUMMARY section: facts as a `dl` (API, Configured timer, Last checked). One grid for every page: 1, 2 or up to 4 across by the width
 * of the panel, in as many columns as there are facts. A fact's tone is a word's colour (`ok` = the connection's own colour, `warn`,
 * `danger`); the words always say it too.
 */
export function StatusSummary({ items, variant = "list", label, className, testId }: StatusSummaryProps) {
  const accent = useAccent();
  const okClass = accent ? accentClasses[accent].text : "text-success-text";
  const cards = variant === "cards";
  return (
    <div className="section-summary-wrap min-w-0" data-section="summary">
      <dl
        aria-label={label}
        className={cx("section-summary grid text-sm", cards ? "gap-3" : "gap-x-4 gap-y-3 border-t border-edge-subtle pt-4", className)}
        data-count={items.length}
        data-testid={testId}
        data-variant={variant}
      >
        {items.map((item, index) => (
          <div className={cards ? "rounded-ds-lg border border-edge-subtle bg-surface-950/40 px-3 py-2.5" : undefined} data-testid={item.testId} key={index}>
            <dt className={cards ? "font-type-kicker text-type-kicker uppercase text-ink-muted" : "text-xs uppercase tracking-wider text-ink-muted"}>{item.label}</dt>
            <dd className={cx("mt-1", cards && "font-semibold", item.mono && "break-all font-mono text-xs", item.tabular && "tabular-nums", item.tone === "ok" ? okClass : item.tone ? tones[item.tone] : cards ? "text-ink" : "text-ink-body")} data-tone={item.tone}>{item.value}</dd>
            {item.detail && <dd className="mt-1 break-all font-mono text-type-micro text-ink-muted">{item.detail}</dd>}
          </div>
        ))}
      </dl>
    </div>
  );
}
