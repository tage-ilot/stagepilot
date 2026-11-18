import { useEffect, useId, useState } from "react";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import type { PlaybackDraft } from "./PlaybackSaveFooter";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ServicePlan } from "../types";
import { PlaybackLibrarySongs } from "./PlaybackLibrarySongs";
import { PlaybackLiveLink } from "./PlaybackLiveLink";
import { PlanningCenterLengths } from "./PlanningCenterLengths";
import { ScanProgress } from "./ScanProgress";
import { PlaybackArrangement } from "./PlaybackArrangement";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import { Disclosure } from "./ui/Disclosure";
import { cx } from "./ui/cx";
import { usePersistedFlag } from "../hooks/usePersistedFlag";
import { Switch } from "./ui/Switch";
import { SCAN_PLAYBACK_SONGS, SONG_ORDER_OPEN_KEY, songOrderSummary, DOUBTFUL_ORDER_HINT, LEARN_SWITCH_HELP, LEARN_SWITCH_LABEL, playbackActivityText, playbackHasDoubtfulSong, playbackOrderMode, playbackOrderModeText, playbackOrderNotice, playbackSongCountMismatch } from "./playbackStatus";

// Connecting automatically is always on (owner, 2026-10-16): there is no switch for it. `draft` holds the song arrangement
// (skipped songs) until Save configuration. The scan, the computer chooser and the Library Link are the first block, PlaybackLiveLink.
export function PlaybackApiPanel({ playback, draft, library, plan = null, timeZone = "UTC" }: { playback?: PlaybackController; draft?: PlaybackDraft; library?: PlaybackLibraryController; plan?: ServicePlan | null; timeZone?: string }) {
  const status = playback?.status;
  const [confirm, setConfirm] = useState(false);
  const [orderOpen, setOrderOpen] = usePersistedFlag(SONG_ORDER_OPEN_KEY, true);
  const orderBodyId = useId();
  // Shown optimistically while the save runs; a failed save puts it back.
  const [learnDraft, setLearnDraft] = useState<boolean | null>(null);

  const running = status?.discovery === "running" || playback?.pending === "discover";
  const apiConnected = status?.sources.playback_api.connected ?? false;
  const orderSaved = Boolean(status?.song_order.length) && !status?.stale && !status?.plan_changed;
  const notice = status ? playbackOrderNotice(status) : null;
  const mismatch = status && orderSaved ? playbackSongCountMismatch(status) : null;
  // A dialog never outlives the state that allowed it (e.g. Playback started playing).
  const canDiscover = apiConnected && !status?.playing && !running;
  useEffect(() => { if (!canDiscover) setConfirm(false); }, [canDiscover]);

  const learnSaved = status?.settings.learn_order !== false;
  const learnOn = learnDraft ?? learnSaved;
  const toggleLearn = async (next: boolean) => {
    if (!status) return;
    setLearnDraft(next);
    // The controller reports the failure (error line); either way the switch then follows the saved status.
    try { await playback?.save({ ...status.settings, learn_order: next }); } finally { setLearnDraft(null); }
  };

  return (
    <div className="mt-6 space-y-4">

      <PlaybackLiveLink library={library} playback={playback} />

      {status && (
        <section className="space-y-2 rounded-ds-xl bg-integration-playback-soft/5 p-4" aria-labelledby="song-order-heading">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2" data-song-order-header>
            <button
              aria-controls={orderBodyId}
              aria-expanded={orderOpen}
              className="flex min-h-ds-touch-target min-w-0 flex-1 touch-manipulation items-center gap-3 rounded-ds-lg text-left can-hover:bg-surface-900/40"
              onClick={() => setOrderOpen(!orderOpen)}
              type="button"
            >
              <svg aria-hidden="true" className={cx("size-3.5 shrink-0 text-integration-playback-text transition-transform duration-[140ms] motion-reduce:transition-none", orderOpen && "rotate-90")} data-part="chevron" viewBox="0 0 12 12">
                <path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.75" />
              </svg>
              <h3 id="song-order-heading" className="shrink-0 font-bold text-ink">Song order</h3>
              <span className="min-w-0 truncate text-sm text-ink-body" data-part="summary" data-testid="song-order-summary">{songOrderSummary(status, running)}</span>
            </button>
            {canDiscover && <Button onClick={() => setConfirm(true)} size="control" tone="playback" variant={orderSaved ? "tinted" : "primary"}>{SCAN_PLAYBACK_SONGS}</Button>}
          </div>
          <div className={cx("grid transition-[grid-template-rows,opacity] duration-300 ease-out motion-reduce:transition-none", orderOpen ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0")} data-part="region" data-state={orderOpen ? "open" : "closed"} id={orderBodyId}>
          <div className="min-h-0 space-y-2 overflow-hidden" inert={!orderOpen}>
          <p className="text-sm font-semibold text-integration-playback-text" data-testid="order-mode">{playbackOrderModeText(status)}</p>
          {status.order_library_note && <p className="text-sm text-ink-body" data-testid="order-library-note">{status.order_library_note}</p>}
          {playbackOrderMode(status) !== "scanned" && status.order_note && <p className="text-sm text-warning-text">{status.order_note}</p>}
          {playbackHasDoubtfulSong(status) && <p className="text-sm text-warning-text">{DOUBTFUL_ORDER_HINT}</p>}
          <div>
            <Switch label={LEARN_SWITCH_LABEL} checked={learnOn} disabled={playback?.pending === "save"} onChange={(event) => void toggleLearn(event.target.checked)} />
            <p className="text-xs text-ink-muted">{LEARN_SWITCH_HELP}</p>
          </div>
          <div className="flex flex-wrap items-start gap-3"><div className="w-full min-w-0 sm:w-auto sm:flex-1">
          <ScanProgress state={running ? "running" : status.discovery} steps={status.discovery_song ?? 0} total={status.discovery_total ?? 0} previousDuration={status.discovery_duration_seconds} estimatedSongs={status.song_order.length}>
          {!apiConnected ? <p className="text-sm text-ink-body">Connect to Playback first.</p>
              : status.playing ? <p className="text-sm text-warning-text">Stop Playback to scan Playback songs.</p>
                : <>
                  {orderSaved ? <p className="text-sm text-integration-playback-text">Song order saved ({status.song_order.length} {status.song_order.length === 1 ? "song" : "songs"})</p>
                    : <>{notice && <p className="text-sm text-warning-text">{notice}</p>}
                      <p className="text-sm text-ink-body">StagePilot steps through your Playback songs once to learn the order. Playback must be stopped.</p></>}
                </>}
          </ScanProgress></div>
          <PlanningCenterLengths status={status} onStatus={playback?.acceptStatus} />
          </div>
          {mismatch && <p className="text-sm text-ink-body">{mismatch}</p>}
          <PlaybackArrangement draft={draft} library={library} status={status} />
          {status.discovery === "failed" && status.last_error && <p role="alert" className="text-sm text-danger-text">{status.last_error}</p>}
          </div>
          </div>
        </section>
      )}
      {library && <PlaybackLibrarySongs library={library} plan={plan} timeZone={timeZone} />}
      <Dialog
        open={confirm}
        dismissible={false}
        title="Scan Playback Songs?"
        description="StagePilot will step through the songs in Playback to read their lengths, then go back to the song that was selected. Playback must stay stopped. Nothing will be played."
        actions={
          <>
            <Button autoFocus onClick={() => { setConfirm(false); playback?.discover(); }} size="touch" tone="playback" variant="tinted">Scan Playback Songs</Button>
            <Button onClick={() => setConfirm(false)} size="touch" tone="playback" variant="tinted">Cancel</Button>
          </>
        }
      />


      <Disclosure title="Recent activity" variant="activity" count={playback?.events.length ?? 0}>
        {!playback?.events.length ? <p className="text-sm text-ink-muted">Nothing yet.</p> : <ul className="max-h-56 overflow-auto overscroll-contain text-sm text-ink-body" aria-label="Recent activity">
          {[...playback.events].reverse().map(({ event, discovery, at }, index) => {
            const position = event.song_id === null ? -1 : status?.song_order.indexOf(event.song_id) ?? -1;
            const known = !status?.stale && position >= 0 ? position : -1;
            return <li key={`${event.timestamp}-${index}`} className="flex gap-3 py-0.5"><span className="whitespace-nowrap tabular-nums text-ink-muted">{at ? new Date(at).toLocaleTimeString() : ""}</span><span>{playbackActivityText(event.type, known)}{discovery ? " (while scanning Playback songs)" : ""}</span></li>;
          })}
        </ul>}
      </Disclosure>
    </div>
  );
}
