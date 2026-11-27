import { useId, useMemo, useState } from "react";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { PresenterFollowController } from "../hooks/usePresenterFollow";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import { AUTO_PRESENTER_OPEN_KEY, followLine, followLock, FOLLOW_LABEL, libraryLinkState, lockWords, MARK_IDLE, shortLine, type FollowLine } from "../presenterFollow";
import type { ServicePlan } from "../types";
import { followByRow, presenterLinkReady, presenterRows } from "./addSongsSongs";
import { AutoPresenterSongs } from "./AutoPresenterSongs";
import { AutoPresenterTiming } from "./AutoPresenterTiming";
import { ActionButton, ActionRow, OptionalBand, SectionNotice } from "./sections";
import { Switch } from "./ui/Switch";
import { cx } from "./ui/cx";

/** The summary and the long line are body ink; only a state that means something (done, look, fault) takes its tone, on the mark and the word that names it. */
const LINE_TONE = {
  success: "text-success-text",
  idle: "text-ink-body",
  warning: "text-warning-text",
  danger: "text-danger-text",
  info: "text-info-text",
} as const;

/** A line of the section: its mark, then its words in body ink; the leading word (`Paused:`, `Following:`) wears the tone of a state that means something. */
function Line({ line, text, size = "" }: { line: FollowLine; text: string; size?: string }) {
  const toned = line.tone !== "idle";
  const colon = text.indexOf(": ");
  const lead = toned ? (colon > 0 ? text.slice(0, colon + 1) : text) : "";
  return (
    <span className={cx("flex min-w-0 items-start gap-1.5 text-ink-body", size)}>
      <span aria-hidden="true" className={cx("w-3 flex-none text-center font-mono", LINE_TONE[line.tone])}>{line.mark}</span>
      <span className="min-w-0 break-words">
        {lead ? <><span className={cx("font-semibold", LINE_TONE[line.tone])}>{lead}</span>{text.slice(lead.length)}</> : text}
      </span>
    </span>
  );
}

/** A small padlock for the locked state (a drawn mark, so it is the same on every system). */
function LockMark() {
  return (
    <svg aria-hidden="true" className="size-4 flex-none" data-part="lock" viewBox="0 0 16 16">
      <rect fill="none" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.5" width="10" x="3" y="7.5" />
      <path d="M5.25 7.5V5a2.75 2.75 0 0 1 5.5 0v2.5" fill="none" stroke="currentColor" strokeLinecap="round" strokeWidth="1.5" />
    </svg>
  );
}

/** The states that need a person: they open the section by themselves (until the person folds it). A click in ProPresenter only holds one song and needs nothing. */
const NEEDS_ATTENTION = new Set<FollowLine["state"]>(["paused", "not_followed", "error"]);

/**
 * **Auto-Presenter**: its own section of the ProPresenter (Presentation) panel, beside the Presenter Link and in the same card (an
 * `OptionalBand` in the Presentation colour: rail, soft wash, bold title with ONE summary line under it, chevron, a rule under the header).
 * The master switch is on the header row. The summary says what it is doing (`Off`, `Following: Chorus, slide 3`, `Paused: someone changed the
 * slide in ProPresenter`, `Needs the Playback Library Link`), always with a mark, never by colour alone. Folded and remembered on this device;
 * it starts folded while it is off and opens by itself when it needs the person (paused by failures, a song that cannot be followed, a switch
 * that is on but locked).
 *
 * It is always drawn, and it can only be turned on while the Presenter Link is ready AND Playback's Library Link answers with current data:
 * otherwise the switch and everything inside are locked and ONE plain reason names the link to set up and where. A switch that was saved as on
 * and is then locked says `Waiting for ...` and nothing else claims it is following (it can still be turned off). Inside: the plan's songs
 * with whether each is followed, Try again only while paused by failures, and the folded Timing group. Nothing here ever triggers ProPresenter
 * by itself; the backend follows only while the switch is on.
 */
export function AutoPresenterSection({ follow, library, link, plan }: { follow?: PresenterFollowController; library?: PlaybackLibraryController; link?: ProPresenterLinkController; plan?: Pick<ServicePlan, "songs"> | null }) {
  const ids = { heading: useId(), songs: useId(), live: useId() };
  const rows = useMemo(() => presenterRows(link, library, plan), [link, library, plan]);
  const followed = useMemo(() => followByRow(rows, follow?.status?.songs), [rows, follow?.status?.songs]);
  const lock = followLock(presenterLinkReady(link), libraryLinkState(library?.status));
  const enabled = follow?.enabled ?? false;
  const line = followLine(follow?.status ?? null, enabled, follow?.error ?? null);
  const busy = follow?.pending != null;

  // The ONE line: while locked it says which link is missing (`Waiting for ...` once the switch was saved as on), and nothing claims it follows.
  const shown: FollowLine = lock
    ? { text: lockWords(lock, enabled), mark: enabled ? "!" : MARK_IDLE, tone: enabled ? "warning" : "idle", state: enabled ? "waiting" : "off" }
    : line;
  const summary = shortLine(shown.text);
  const canTryAgain = enabled && !lock && Boolean(follow?.status?.can_try_again);
  // The long form of the line (the reason in brackets, what happens next), shown inside only when the summary had to leave something out.
  const detail = !lock && shown.text !== summary ? shown : null;

  // Opens by itself while something needs the person; only the person's press folds it again (their remembered choice is left alone).
  const attention = enabled && (lock !== null || NEEDS_ATTENTION.has(shown.state));
  const key = `${shown.state}|${shown.text}`;
  const [foldedAt, setFoldedAt] = useState<string | null>(null);
  const forceOpen = attention && foldedAt !== key;

  const turn = (on: boolean) => { if (on && lock) return; void follow?.setEnabled(on); };
  const locked = lock !== null || !follow;

  const toggle = (
    <div className="ml-auto flex items-center gap-3 pb-1 [&_label]:font-semibold [&_label]:text-ink" data-part="auto-presenter-switch">
      {lock !== null && (
        <span className="inline-flex items-center gap-1.5 text-sm font-semibold text-ink-body" data-testid="presenter-follow-locked"><LockMark />Locked</span>
      )}
      <Switch aria-describedby={ids.live} aria-label={FOLLOW_LABEL} checked={enabled} data-testid="presenter-follow-switch" disabled={!follow || busy || (lock !== null && !enabled)} label={enabled ? "On" : "Off"} onChange={(event) => turn(event.target.checked)} />
    </div>
  );
  return (
    <div className="flex min-w-0 flex-col gap-3" data-locked={locked || undefined} data-state={shown.state} data-testid="auto-presenter">
      {/* The polite live line is OUTSIDE the band, so it is read while the section is folded. */}
      <p aria-live="polite" className="sr-only" data-testid="presenter-follow-status" id={ids.live} role="status">{lock ? `${lockWords(lock, enabled)}. ${lock.where}` : shown.text}</p>
      <OptionalBand
        actions={toggle}
        defaultOpen={enabled}
        forceOpen={forceOpen}
        headingId={ids.heading}
        onOpenChange={(next) => setFoldedAt(next ? null : key)}
        persistKey={AUTO_PRESENTER_OPEN_KEY}
        summary={(
          <Line line={shown} text={summary} />
        )}
        summaryTestId="auto-presenter-summary"
        title={FOLLOW_LABEL}
        tone="presentation"
        wrapSummary
      >
        {lock && (
          <SectionNotice tone={enabled ? "warning" : "info"} title={lockWords(lock, enabled)}><span data-testid="presenter-follow-reason">{lock.where}</span></SectionNotice>
        )}
        <fieldset aria-describedby={ids.live} className="m-0 min-w-0 space-y-4 border-0 p-0" data-locked={locked || undefined} disabled={locked}>
          {detail && (
            <p data-state={detail.state} data-testid="auto-presenter-detail"><Line line={detail} size="text-sm" text={detail.text} /></p>
          )}
          {canTryAgain && (
            <ActionRow>
              <ActionButton busy={follow?.pending === "retry"} busyLabel="Trying again…" label="Try again" onClick={() => follow?.tryAgain()} tone="presentation" />
            </ActionRow>
          )}
          <AutoPresenterSongs follow={followed} headingId={ids.songs} lock={lock} rows={rows} />
        </fieldset>
        {/* Timing is a setting like any other: it stays readable and usable while the list above is locked. */}
        <div className="pt-2"><AutoPresenterTiming follow={follow} /></div>
      </OptionalBand>
    </div>
  );
}
