import { useId, type ReactNode } from "react";

import { BusyContext } from "../ui/busyContext";
import { cx } from "../ui/cx";

/** How many columns the fields take, by the width of the panel they sit in (a container, never the window). */
export type SettingsColumns =
  /** 1, 2 or 4 across (28rem, 56rem), never 3 + 1: the default. */
  | "standard"
  /** The same 1, 2 or 4, for a group inside a padded row (Advanced), switching where the summary cards above it do. */
  | "inset"
  /** Two side by side from 30rem. */
  | "pair"
  /** Three across from 40rem (never 2 + 1). */
  | "triple"
  /** Three across from 30rem, the first field twice as wide. */
  | "lead"
  /** One column, however wide. */
  | "single";

type DataAttributes = { [key: `data-${string}`]: string | boolean | undefined };

export interface SettingsSectionProps extends DataAttributes {
  /** A small uppercase heading over the group. Left out (the usual case) the fields simply follow the previous section. */
  title?: ReactNode;
  /** The fields: each one a `Field` holding exactly one control. Held change, hint and error line belong to `Field`; this section lays them out. */
  children: ReactNode;
  columns?: SettingsColumns;
  /** `narrow` keeps a lone field from stretching across a wide panel (max 28rem). */
  width?: "full" | "narrow";
  /** A rule above the group and a little room under it: it starts a new part of the page (DESIGN.md: a divider and the fields, no sub-heading). */
  divided?: boolean;
  /** Drawn as its own bordered box (DESIGN.md: only form fields, Advanced rows, scroll regions, the Save bar and the two Remote-access groups get a border). */
  boxed?: boolean;
  /** A request is running: every Input and Select inside is disabled, so no page passes `disabled` field by field. */
  busy?: boolean;
  /** Under the fields: the one note that belongs to the whole group (what takes effect after a restart, how a value is used). */
  note?: ReactNode;
  noteId?: string;
  className?: string;
  /** Hook for a page's existing test or label (`aria-label` names the group for a screen reader when there is no visible title). */
  testId?: string;
  "aria-label"?: string;
}

/**
 * The SETTINGS section: a group of fields in the standard responsive grid. The order never changes: the optional kicker title, the
 * fields, then the one note. Held-change markers, hints and error lines stay on each `Field`, so a change to how they look is one edit
 * in `ui/Field`; the grid is drawn here and in `index.css` (`.section-fields`).
 */
export function SettingsSection({ title, children, columns = "standard", width = "full", divided = false, boxed = false, busy = false, note, noteId, className, testId, "aria-label": ariaLabel, ...data }: SettingsSectionProps) {
  const headingId = useId();
  return (
    <BusyContext.Provider value={busy}>
      <div
        {...data}
        aria-label={ariaLabel}
        aria-labelledby={!ariaLabel && title ? headingId : undefined}
        className={cx("section-fields-wrap min-w-0", width === "narrow" && "max-w-md", divided && "border-t border-edge-subtle pt-4", boxed && "rounded-ds-lg border border-edge-subtle bg-surface-950/40 p-4", className)}
        data-section="settings"
        data-testid={testId}
        role={title || ariaLabel ? "group" : undefined}
      >
        {title && <h3 className="mb-3 font-type-kicker text-type-kicker uppercase text-ink-muted" data-part="title" id={headingId}>{title}</h3>}
        <div className="section-fields grid gap-4" data-columns={columns} data-part="fields">
          {children}
        </div>
        {note && <p className="mt-2 text-type-body-sm text-ink-muted" data-part="note" id={noteId}>{note}</p>}
      </div>
    </BusyContext.Provider>
  );
}
