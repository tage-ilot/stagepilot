import { Fragment, type ReactNode } from "react";

import { cx } from "../ui/cx";
import { Panel, type PanelIdentity } from "../ui/Panel";
import { bodyOrder } from "./panelOrder";

/** The slots of a panel's body, in the order they are drawn. A page leaves a slot out to skip it; nothing else moves. */
export interface PanelBodySlots {
  /** `ConnectionSection` (the Live Link block), when the page has one. */
  connection?: ReactNode;
  /** One or more `SettingsSection`s. */
  settings?: ReactNode;
  /** `ActionRow`: the buttons that belong with the settings above (Refresh, Test, Load). */
  actions?: ReactNode;
  /** `OptionalSection`s: advanced settings, then optional features (DESIGN.md: Advanced always after the general settings). */
  optional?: ReactNode;
  /** What only this page has (the song list, the MIDI monitor, the Connection Access switch): placed after the actions, before the notices. */
  content?: ReactNode;
  /** `SectionNotice`s: why Save is off, the last failure or message. */
  notices?: ReactNode;
  /** The Save bar, always last of what changes something. */
  saveBar?: ReactNode;
  /** `StatusSummary`. */
  summary?: ReactNode;
  /** `bottom` (default): the summary closes the page. `top`: it opens it (Backend's Status cards are the first thing read there). */
  summaryAt?: "top" | "bottom";
}

const filled = (slot: ReactNode) => slot !== undefined && slot !== null && slot !== false;

/**
 * The BODY of a configuration panel: the order every page follows (`BODY_SLOTS`). Top to bottom: the connection section, the settings
 * section(s), the action row, the optional sections, the page's own content, the notices, the Save bar, the summary. The gap between
 * them is one `space-y-4`, so no page sets margins of its own on a section.
 */
export function PanelBody({ summaryAt = "bottom", ...slots }: PanelBodySlots) {
  return (
    <div className="mt-6 space-y-4" data-panel-body>
      {bodyOrder(summaryAt).map((name) => (filled(slots[name]) ? <Fragment key={name}>{slots[name]}</Fragment> : null))}
    </div>
  );
}

export interface PanelLayoutProps extends PanelBodySlots {
  /** The panel's id (`propresenter-configuration`): the dashboard scrolls to it. */
  id: string;
  /** The connection the panel belongs to (its colour). */
  tone: PanelIdentity;
  /** Controls wear another colour than the panel's own (Lighting's MIDI view, Playback's MIDI view). */
  accent?: PanelIdentity;
  /** `SetupPanelHeader`, always first. */
  header: ReactNode;
  /** Lines of news under the header: loading, connecting, the last failure or message. */
  alerts?: ReactNode;
  "aria-labelledby"?: string;
  "aria-busy"?: boolean;
  /** One id around the alert lines and the body, for a page whose view is addressed as a whole (the MIDI view of the Playback page). */
  bodyId?: string;
  /**
   * A body the page already built from `PanelBody` (the Playback panel is one, shown inside the MIDI / Playback page). Given, the slots
   * above are not used.
   */
  children?: ReactNode;
  /** Under the body: what a page with its own body still needs after it (the Save bar), and the dialogs. */
  below?: ReactNode;
  className?: string;
}

/**
 * The PANEL: the one frame and order of a configuration page: the `Panel` in the connection's colour, the header, the alert lines, then
 * the body in the fixed order of `PanelBody`. A page fills the slots with templates and page-specific content; it never lays the panel
 * out itself.
 */
export function PanelLayout({ id, tone, accent, header, alerts, children, below, className, bodyId, "aria-labelledby": labelledBy, "aria-busy": busy, ...slots }: PanelLayoutProps) {
  const inner = (
    <>
      {filled(alerts) && <div className="mt-4 space-y-3 empty:hidden" data-slot="alerts">{alerts}</div>}
      {children ?? <PanelBody {...slots} />}
    </>
  );
  return (
    <Panel accent={accent} aria-busy={busy} aria-labelledby={labelledBy} className={cx("mt-5", className)} data-panel-layout id={id} tone={tone} variant="setup">
      {header}
      {bodyId ? <div id={bodyId}>{inner}</div> : inner}
      {filled(below) && below}
    </Panel>
  );
}
