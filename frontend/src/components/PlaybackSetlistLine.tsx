import { useState } from "react";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { LibrarySetlist } from "../types";

const songWord = (n: number) => `${n} ${n === 1 ? "song" : "songs"}`;
const when = (setlist: LibrarySetlist) => setlist.date ?? "no date";

const choice = "flex min-h-ds-touch-target w-full touch-manipulation items-center justify-between gap-3 border-2 border-edge-medium bg-surface-950/40 px-3 py-1.5 text-left text-sm text-ink can-hover:border-integration-playback-soft can-hover:bg-integration-playback-soft/10 disabled:cursor-not-allowed disabled:opacity-60";

/**
 * "Playback setlist: ...": which Playback setlist the matchup reads (the one on the loaded plan's date; the newest if there are
 * several), and, when Playback has none for that date, a small chooser of its other setlists. Never a guess: until one is chosen
 * nothing is paired. The choice lasts for this plan while StagePilot runs and changes nothing in Playback or Planning Center.
 * The line is a polite live region, so a switch is announced.
 */
export function PlaybackSetlistLine({ library }: { library: PlaybackLibraryController }) {
  const plan = library.plan;
  const [more, setMore] = useState(false);
  const [failed, setFailed] = useState(false);
  if (!plan?.plan_date) return null;
  const busy = library.pending === "setlist";
  const none = plan.state === "no_setlist";
  const current = plan.state === "ready" ? plan.setlist : null;
  if (!none && !current) return null;
  const others = plan.setlists.filter((setlist) => setlist.id !== null && setlist.id !== current?.id);
  const pick = async (id: number | null) => {
    const done = await library.chooseSetlist(id);
    setFailed(!done);
    if (done) setMore(false);
  };
  const showChoices = none || more;
  return (
    <div className="arr-setlist space-y-2 border-2 border-edge-medium bg-surface-panel p-3" data-testid="playback-setlist" data-state={none ? "none" : plan.chosen ? "chosen" : "date"}>
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <p className="min-w-0 text-sm text-ink-body" role="status">
          <span className="font-type-badge text-[0.6875rem] uppercase tracking-[0.1em] text-integration-playback-text">Playback setlist</span>{" "}
          {none
            ? <span className="text-warning-text">{plan.message ?? `Playback has no setlist for ${plan.plan_date}.`}</span>
            : <><b className="break-words text-ink">{current!.title}</b>{current!.date && !current!.title.includes(current!.date) ? <>, <span className="tabular-nums">{current!.date}</span></> : null}, <span className="tabular-nums">{songWord(current!.songs)}</span>. {plan.chosen ? `Chosen by you for this plan (the plan's date is ${plan.plan_date}).` : "It is the one on the plan's date."}</>}
        </p>
        {plan.chosen && (
          <button className="min-h-ds-touch-target touch-manipulation text-sm text-integration-playback-text underline disabled:opacity-60" disabled={busy} onClick={() => void pick(null)} type="button">Use the setlist for {plan.plan_date}</button>
        )}
        {!none && plan.chosen && others.length > 0 && (
          <button aria-expanded={more} className="min-h-ds-touch-target touch-manipulation text-sm text-integration-playback-text underline" onClick={() => setMore(!more)} type="button">{more ? "Hide other setlists" : "Choose another setlist"}</button>
        )}
      </div>
      {none && <p className="text-[0.8125rem] text-ink-muted">Nothing is lined up until you choose one. StagePilot never picks a setlist for you, and choosing changes nothing in Playback or Planning Center.</p>}
      {showChoices && others.length > 0 && (
        <ul aria-label="Other Playback setlists" className="m-0 grid max-h-72 list-none gap-1.5 overflow-y-auto overscroll-contain p-0 sm:grid-cols-2">
          {others.map((setlist) => (
            <li key={setlist.id}>
              <button aria-label={`Use ${setlist.title}, ${when(setlist)}, ${songWord(setlist.songs)}`} className={choice} disabled={busy} onClick={() => void pick(setlist.id)} type="button">
                <span className="min-w-0 break-words font-semibold">{setlist.title}</span>
                <span className="flex-none text-xs tabular-nums text-ink-muted">{when(setlist)} · {songWord(setlist.songs)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {none && others.length === 0 && <p className="text-[0.8125rem] text-ink-muted">Playback did not list any other setlists.</p>}
      {busy && <p className="text-[0.8125rem] text-ink-muted" role="status">Switching…</p>}
      {failed && <p className="text-sm text-danger-text" role="alert">StagePilot could not switch the Playback setlist. Try again.</p>}
    </div>
  );
}
