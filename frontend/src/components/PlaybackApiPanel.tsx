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
import { OptionalBand, PanelBody, SectionNotice } from "./sections";
import { Switch } from "./ui/Switch";
import { LIBRARY_BUTTON_HELP, LIBRARY_ORDER_HELP, SCAN_PLAYBACK_API, readFromLibraryText, songScanButton, SONG_ORDER_OPEN_KEY, songOrderSummary, DOUBTFUL_ORDER_HINT, LEARN_SWITCH_HELP, LEARN_SWITCH_LABEL, playbackHasDoubtfulSong, playbackOrderMode, playbackOrderModeText, playbackOrderNotice, playbackSongCountMismatch } from "./playbackStatus";

// Connecting automatically is always on (owner, 2026-10-16): there is no switch for it. `draft` holds the song arrangement
// (skipped songs) until Save configuration. The scan, the computer chooser and the Library Link are the first block, PlaybackLiveLink.
export function PlaybackApiPanel({ playback, draft, library, plan = null, timeZone = "UTC" }: { playback?: PlaybackController; draft?: PlaybackDraft; library?: PlaybackLibraryController; plan?: ServicePlan | null; timeZone?: string }) {
  const status = playback?.status;
  const [confirm, setConfirm] = useState(false);
  // Shown optimistically while the save runs; a failed save puts it back.
  const [learnDraft, setLearnDraft] = useState<boolean | null>(null);

  const running = status?.discovery === "running" || playback?.pending === "discover";
  // The Library Link answers (ready or outdated) with data that is not an old copy: the button reads it instead of stepping through Playback.
  const libraryUsable = Boolean(library?.status?.active) && !library?.status?.stale && Boolean(playback?.refreshFromLibrary);
  const reading = playback?.pending === "library";
  // A condition of the Library Link's order failed: its plain line is shown, and the best scan available is the Playback API's.
  const libraryNote = libraryUsable ? status?.order_library_note ?? null : null;
  const apiConnected = status?.sources.playback_api.connected ?? false;
  const reasonId = useId();
  const orderSaved = Boolean(status?.song_order.length) && !status?.stale && !status?.plan_changed;
  const libraryRead = playback?.libraryRead ?? null;
  const notice = status ? playbackOrderNotice(status) : null;
  const mismatch = status && orderSaved ? playbackSongCountMismatch(status) : null;
  // A dialog never outlives the state that allowed it (e.g. Playback started playing).
  // Reading the Library Link touches nothing in Playback, so it works while a song plays; stepping through the songs does not.
  const canStep = apiConnected && !status?.playing && !running && !reading;
  // ONE button, always drawn: the best scan there is, greyed out with its reason when none can run now.
  const scan = songScanButton({ libraryUsable, libraryNote, apiConnected, playing: Boolean(status?.playing), running, reading });
  const byLibrary = scan.kind === "library";
  useEffect(() => { if (!canStep) setConfirm(false); }, [canStep]);

  const learnSaved = status?.settings.learn_order !== false;
  const learnOn = learnDraft ?? learnSaved;
  const toggleLearn = async (next: boolean) => {
    if (!status) return;
    setLearnDraft(next);
    // The controller reports the failure (error line); either way the switch then follows the saved status.
    try { await playback?.save({ ...status.settings, learn_order: next }); } finally { setLearnDraft(null); }
  };

  const orderHeadline = status ? `Song order saved (${status.song_order.length} ${status.song_order.length === 1 ? "song" : "songs"})` : "";
  const songOrder = status && (
    <OptionalBand
      actions={(
        // A disabled button shows no tooltip of its own in every browser, so the reason is on the wrapper too; a screen reader hears it as the button's description.
        <span className="inline-flex" data-part="song-scan" data-scan={scan.kind} title={scan.reason ?? undefined}>
          <Button aria-describedby={scan.reason ? reasonId : undefined} disabled={Boolean(scan.reason)} loading={byLibrary && reading} onClick={() => byLibrary ? playback?.refreshFromLibrary?.() : setConfirm(true)} size="control" title={scan.reason ?? undefined} tone="playback" variant={orderSaved ? "tinted" : "primary"}>{scan.label}</Button>
          {scan.reason && <span className="sr-only" id={reasonId}>{scan.reason}</span>}
        </span>
      )}
      headingId="song-order-heading"
      persistKey={SONG_ORDER_OPEN_KEY}
      summary={songOrderSummary(status, running)}
      summaryTestId="song-order-summary"
      title="Song order"
    >
      <SectionNotice strong testId="order-mode" tone="accent" variant="line">{playbackOrderModeText(status)}</SectionNotice>
      {status.order_library_note && <SectionNotice testId="order-library-note" tone="info" variant="line">{status.order_library_note}</SectionNotice>}
      <SectionNotice className="empty:hidden" live role="status" strong testId="library-read-line" tone="accent" variant="line">{libraryRead !== null && libraryRead !== undefined && !reading && status.order_source === "library" ? readFromLibraryText(libraryRead) : null}</SectionNotice>
      {playbackOrderMode(status) !== "scanned" && status.order_note && <SectionNotice tone="warning" variant="line">{status.order_note}</SectionNotice>}
      {playbackHasDoubtfulSong(status) && <SectionNotice tone="warning" variant="line">{DOUBTFUL_ORDER_HINT}</SectionNotice>}
      <div>
        <Switch label={LEARN_SWITCH_LABEL} checked={learnOn} disabled={playback?.pending === "save"} onChange={(event) => void toggleLearn(event.target.checked)} />
        <SectionNotice quiet size="xs" tone="info" variant="line">{LEARN_SWITCH_HELP}</SectionNotice>
      </div>
      <div className="flex flex-wrap items-start gap-3"><div className="w-full min-w-0 sm:w-auto sm:flex-1">
        <ScanProgress state={running ? "running" : status.discovery} steps={status.discovery_song ?? 0} total={status.discovery_total ?? 0} previousDuration={status.discovery_duration_seconds} estimatedSongs={status.song_order.length}>
          {!apiConnected ? <SectionNotice tone="info" variant="line">Connect to Playback first.</SectionNotice>
            : byLibrary ? <>
              {orderSaved ? <SectionNotice tone="accent" variant="line">{orderHeadline}</SectionNotice>
                : notice && <SectionNotice tone="warning" variant="line">{notice}</SectionNotice>}
              <SectionNotice testId="order-help" tone="info" variant="line">{status.order_source === "library" && !status.order_library_note ? LIBRARY_ORDER_HELP : LIBRARY_BUTTON_HELP}</SectionNotice>
            </>
              : status.playing ? <SectionNotice tone="warning" variant="line">Stop Playback to scan Playback songs.</SectionNotice>
                : <>
                  {orderSaved ? <SectionNotice tone="accent" variant="line">{orderHeadline}</SectionNotice>
                    : <>{notice && <SectionNotice tone="warning" variant="line">{notice}</SectionNotice>}
                      <SectionNotice tone="info" variant="line">StagePilot steps through your Playback songs once to learn the order. Playback must be stopped.</SectionNotice></>}
                </>}
        </ScanProgress></div>
        <PlanningCenterLengths onStatus={playback?.acceptStatus} scanLabel={scan.label} status={status} />
      </div>
      {mismatch && <SectionNotice tone="info" variant="line">{mismatch}</SectionNotice>}
      <PlaybackArrangement draft={draft} library={library} status={status} />
      {status.discovery === "failed" && status.last_error && <SectionNotice role="alert" tone="danger" variant="line">{status.last_error}</SectionNotice>}
    </OptionalBand>
  );

  return (
    <>
      <PanelBody
        connection={<PlaybackLiveLink library={library} playback={playback} />}
        content={(library || status) ? (
          <>
            {library && <PlaybackLibrarySongs library={library} plan={plan} timeZone={timeZone} />}
            {songOrder}
          </>
        ) : undefined}
      />
      <Dialog
        open={confirm}
        dismissible={false}
        title={`${SCAN_PLAYBACK_API}?`}
        description="StagePilot will step through the songs in Playback to read their lengths, then go back to the song that was selected. Playback must stay stopped. Nothing will be played."
        actions={
          <>
            <Button autoFocus onClick={() => { setConfirm(false); playback?.discover(); }} size="touch" tone="playback" variant="tinted">{SCAN_PLAYBACK_API}</Button>
            <Button onClick={() => setConfirm(false)} size="touch" tone="playback" variant="tinted">Cancel</Button>
          </>
        }
      />
    </>
  );
}
