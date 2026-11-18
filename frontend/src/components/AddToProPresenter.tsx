import { useEffect, useId, useRef, useState } from "react";

import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import { useAddSongs } from "../hooks/useAddSongs";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import { addSongsRows, addToProPresenterAvailable, type AddSongsRow } from "./addSongsSongs";
import { ADD_TO_PROPRESENTER, canTryAgain, importsSentence, NOTHING_CHANGES } from "./addSongsWording";
import { PlaylistPicker, PreviewBody, Progress, Reason, Result, SongPicker } from "./AddSongsParts";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import { InlineAlert } from "./ui/InlineAlert";

/**
 * **Add to ProPresenter**, beside the songs Playback plays. It is drawn only when the ProPresenter Link is ready (this computer or the
 * helper), the Playback Library Link answers, and the loaded plan is paired with a Playback setlist; otherwise it draws nothing (the
 * feature is optional, so a computer without it shows no warning). It opens the window; nothing in ProPresenter changes until a person
 * has looked at the preview and pressed Add songs.
 */
export function AddToProPresenter({ library, link, playing = false }: { library?: PlaybackLibraryController; link?: ProPresenterLinkController; playing?: boolean }) {
  const [open, setOpen] = useState(false);
  const available = addToProPresenterAvailable(link, library);
  // The window never outlives the state that allowed it, unless it is the one that is running a job (it stays to say how it ended).
  if (!link || !library || !(available || open)) return null;
  const rows = addSongsRows(library.plan);
  return (
    <>
      {available && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 px-0.5 pt-1" data-testid="add-to-propresenter">
          <Button aria-haspopup="dialog" onClick={() => setOpen(true)} size="control" tone="presentation" variant="tinted">{ADD_TO_PROPRESENTER}</Button>
        </div>
      )}
      {open && <AddToProPresenterDialog link={link} onClose={() => setOpen(false)} playing={playing} rows={rows} />}
    </>
  );
}

// One fixed height (never taller than the window): the list grows inside it and scrolls, so nothing above or below it moves; the buttons
// stay at the bottom edge.
const CARD = "flex flex-col !max-w-[62rem] h-[min(40rem,calc(100dvh-2rem))] min-[481px]:!w-[44rem] min-[900px]:!w-[62rem] [&_[data-dialog-actions]]:sticky [&_[data-dialog-actions]]:bottom-[-1.25rem] [&_[data-dialog-actions]]:mt-auto [&_[data-dialog-actions]]:-mx-5 [&_[data-dialog-actions]]:-mb-5 max-[480px]:[&_[data-dialog-actions]]:grid max-[480px]:[&_[data-dialog-actions]]:grid-cols-[auto_auto_minmax(0,1fr)] max-[480px]:[&_[data-dialog-actions]]:gap-2 [&_[data-dialog-actions]]:flex-none [&_[data-dialog-actions]]:border-t [&_[data-dialog-actions]]:border-edge-subtle [&_[data-dialog-actions]]:bg-surface-950 [&_[data-dialog-actions]]:px-5 [&_[data-dialog-actions]]:py-3";

/** The window itself: mounted while it is open, so every opening starts clean (all songs ticked, no preview). */
export function AddToProPresenterDialog({ rows, link, playing, onClose }: { rows: AddSongsRow[]; link: ProPresenterLinkController; playing: boolean; onClose: () => void }) {
  const add = useAddSongs({ rows, link, playing });
  const ids = { songs: useId(), preview: useId(), progress: useId(), result: useId(), reason: useId() };
  const { phase, plan, report } = add;
  const running = phase === "applying";
  const finished = phase === "finished";
  const working = phase === "previewing";
  const gone = !add.linkReady;

  // Keyboard and screen reader users follow the window: the new part takes the focus when it appears.
  const previous = useRef(phase);
  useEffect(() => {
    const was = previous.current;
    previous.current = phase;
    if (was === phase) return;
    const target = phase === "previewed" ? ids.preview : phase === "applying" ? ids.progress : phase === "finished" ? ids.result : null;
    const element = target ? document.getElementById(target) : null;
    element?.focus({ preventScroll: true });
    // One column: the new part may be far below, so it is brought to the top. Two columns: it is already beside the choice.
    if (window.matchMedia?.("(max-width: 899px)").matches) element?.scrollIntoView?.({ block: "start", behavior: "auto" });
  }, [phase, ids.preview, ids.progress, ids.result]);

  const previewHint = `Press Preview to see what would happen to each song. ${NOTHING_CHANGES}`;
  const reason = running ? "Adding songs. Keep this window open until it says it is finished."
    : finished ? null
      : !plan || add.stale ? add.previewBlocked ?? add.addBlocked
        : add.addBlocked;

  return (
    <Dialog
      actions={(
        <>
          {(reason || (!running && !finished)) && <Reason id={ids.reason} note={!running && !finished ? importsSentence(add.playlist?.name ?? null) : undefined}>{reason}</Reason>}
          {!running && <Button onClick={onClose} size="touch" variant="secondary">Close</Button>}
          {finished && report && canTryAgain(report.outcome) && <Button onClick={add.tryAgain} size="touch" tone="presentation" variant="tinted">Try again</Button>}
          {!running && !finished && (
            <>
              <Button aria-describedby={add.previewBlocked ? ids.reason : undefined} disabled={Boolean(add.previewBlocked)} loading={working} loadingLabel={"Making the preview\u2026"} onClick={() => void add.preview()} size="touch" tone="presentation" variant={plan && !add.stale ? "tinted" : "primary"}>Preview</Button>
              <Button aria-describedby={add.addBlocked ? ids.reason : undefined} disabled={Boolean(add.addBlocked) || working} onClick={() => void add.add()} size="touch" variant={plan && !add.stale ? "primary" : "secondary"}>Add songs</Button>
            </>
          )}
        </>
      )}
      className={CARD}
      description="Put this service's Playback songs into a ProPresenter playlist, in Playback's order and key."
      dismissible={!running}
      onClose={onClose}
      open
      title={ADD_TO_PROPRESENTER}
    >
      <div className="space-y-4" data-phase={phase} data-testid="add-songs-window">
        <p aria-live="polite" className="sr-only" data-testid="add-songs-live" role="status">{add.announcement}</p>
        {gone && <InlineAlert tone="warning"><span data-testid="add-songs-link-gone">The ProPresenter Link is not ready now. Nothing was changed.</span></InlineAlert>}
        {playing && !running && !finished && <InlineAlert tone="info"><span data-testid="add-songs-playing">A song is playing. You can look at a preview now, but songs can only be added when it has ended.</span></InlineAlert>}
        {add.problem && <InlineAlert tone="danger"><span data-code={add.problem.code ?? undefined} data-testid="add-songs-problem">{add.problem.message}</span></InlineAlert>}
        {!running && !finished && (
          // Wide: the choice on the left, what it would do on the right, so nothing has to be scrolled to be seen. Narrow: one column.
          <div className="grid gap-x-6 gap-y-4 min-[900px]:grid-cols-[minmax(0,24rem)_minmax(0,1fr)]">
            <div className="min-w-0 space-y-4 min-[900px]:sticky min-[900px]:top-0 min-[900px]:self-start">
              <SongPicker add={add} headingId={ids.songs} />
              <PlaylistPicker add={add} />
            </div>
            <div className="min-w-0">
              {plan
                ? <PreviewBody add={add} headingId={ids.preview} plan={plan} />
                : <p className="text-sm text-ink-muted" data-testid="add-songs-no-preview">{previewHint}</p>}
            </div>
          </div>
        )}
        {running && <Progress add={add} headingId={ids.progress} plan={plan} />}
        {finished && <Result add={add} headingId={ids.result} />}
      </div>
    </Dialog>
  );
}
