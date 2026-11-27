import { useEffect, useId, useMemo, useRef, useState } from "react";

import { useAddSongs } from "../hooks/useAddSongs";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { ServicePlan } from "../types";
import { presenterRows } from "./addSongsSongs";
import { finishedWords } from "./addSongsStatus";
import { canTryAgain, IMPORT_SONGS, IMPORTING, PLAYING_WORDS, presenterSummary, SKIP_AND_IMPORT, STALE_WORDS, stuckReason } from "./addSongsWording";
import { PlaylistPicker, PreviewBody, Progress, Reason, Result } from "./AddSongsParts";
import { PresenterSongList } from "./PresenterSongList";
import { PRESENTER_LINK_NAME, PRESENTER_LINK_OPEN_KEY } from "./proPresenterLinkState";
import { ActionButton, ActionRow, OptionalBand, SectionNotice } from "./sections";

/**
 * **Presenter Link**, in the ProPresenter (Presentation) panel: which playlist and which section of it, the songs as cards (Playback's, in
 * Playback's order, with MultiTracks id, key and arrangement; until Playback has given them, the plan's Planning Center names), and ONE
 * button, **Import songs to ProPresenter**, that makes the preview and, when nothing in it is blocked, imports the songs. It is drawn only
 * while the Presenter Link is ready (this computer or the helper); otherwise it draws nothing (the feature is optional, so a computer
 * without it shows no warning). Nothing in ProPresenter changes until the button is pressed, and never while a song is playing. The page
 * says only what a person must act on; docs/propresenter.md explains the rest.
 *
 * It is an `OptionalBand` (the fold Playback's Song order has) that STARTS FOLDED; its header says whether the songs can be imported
 * (`presenterSummary`). A job that is being made or has just ended, and a problem, open it whatever the person chose, and it stays open
 * until they press the toggle.
 */
export function PresenterLinkSection({ library, link, plan, playing = false }: { library?: PlaybackLibraryController; link?: ProPresenterLinkController; plan?: Pick<ServicePlan, "songs"> | null; playing?: boolean }) {
  if (!link) return null;
  return <PresenterLinkBody library={library} link={link} plan={plan} playing={playing} />;
}

/** Mounted whenever a controller exists, so a job that is running (or has just ended) is not lost if the Link blinks; it draws only while it may. */
function PresenterLinkBody({ library, link, plan, playing }: { library?: PlaybackLibraryController; link: ProPresenterLinkController; plan?: Pick<ServicePlan, "songs"> | null; playing: boolean }) {
  const rows = useMemo(() => presenterRows(link, library, plan), [link, library, plan]);
  const add = useAddSongs({ rows, link, playing });
  const ids = { heading: useId(), songs: useId(), preview: useId(), progress: useId(), result: useId(), reason: useId() };
  const { phase, plan: made, report } = add;
  const running = phase === "applying";
  const finished = phase === "finished";
  const working = phase === "previewing";

  // Keyboard and screen reader users follow the section: the new part takes the focus when it appears.
  const previous = useRef(phase);
  useEffect(() => {
    const was = previous.current;
    previous.current = phase;
    if (was === phase) return;
    const target = phase === "previewed" ? ids.preview : phase === "applying" ? ids.progress : phase === "finished" ? ids.result : null;
    const element = target ? document.getElementById(target) : null;
    element?.focus({ preventScroll: true });
    // The new part sits right under the button; scroll only as far as it needs to be seen.
    element?.scrollIntoView?.({ block: "nearest", behavior: "auto" });
  }, [phase, ids.preview, ids.progress, ids.result]);

  // Work in progress is never hidden. The preview being made and the import running keep it open whatever the person does; a preview that
  // stopped, a result and a problem open it and it stays open until they press the toggle (nothing closes by itself).
  const problem = add.problem?.message ?? null;
  const key = `${phase}|${problem ?? ""}`;
  const [foldedAt, setFoldedAt] = useState<string | null>(null);
  const locked = running || working;
  const forceOpen = locked || ((phase !== "choose" || problem !== null) && foldedAt !== key);

  // The section is always drawn. While the Link is not ready (no ProPresenter connection, or no Link) it shows, greyed, what it will
  // do and nothing in it can be pressed; only its own open/close header works. A job that is running or has ended is never locked.
  const notReady = !add.linkReady && !running && !finished;

  const reason = running ? "Importing: wait here" : finished ? null : add.blockedBy ?? add.stopped;
  const summary = presenterSummary({
    songs: add.rows.length, ready: add.choices.length, playing, importing: running || working, problem: problem !== null,
    result: finished ? finishedWords(report) : null,
    reason: phase === "previewed" && made ? (add.stale ? STALE_WORDS : stuckReason(made, false)) : null,
  });
  return (
    <div className="flex flex-col gap-3" data-phase={phase} data-testid="presenter-link-section">
      <p aria-live="polite" className="sr-only" data-testid="add-songs-live" role="status">{add.announcement}</p>
      <OptionalBand defaultOpen={false} forceOpen={forceOpen} headingId={ids.heading} onOpenChange={(next) => setFoldedAt(next ? null : key)} persistKey={PRESENTER_LINK_OPEN_KEY} summary={summary} summaryTestId="presenter-link-summary" title={PRESENTER_LINK_NAME} tone="presentation">
        <fieldset className={`m-0 min-w-0 space-y-4 border-0 p-0${notReady ? " opacity-60" : ""}`} data-locked={notReady || undefined} disabled={notReady}>
          <PlaylistPicker add={add} />
          {!add.linkReady && !notReady && <SectionNotice tone="warning"><span data-testid="add-songs-link-gone" title="Nothing was changed.">{`${PRESENTER_LINK_NAME} not ready: nothing changed`}</span></SectionNotice>}
          {/* The one line at the top when a song is playing; once the preview has stopped for it, the same words are the reason above the button. */}
          {playing && !running && !finished && reason !== PLAYING_WORDS && <SectionNotice tone="info"><span data-testid="add-songs-playing">{PLAYING_WORDS}</span></SectionNotice>}
          {add.problem && <SectionNotice tone="danger"><span data-code={add.problem.code ?? undefined} data-testid="add-songs-problem">{add.problem.message}</span></SectionNotice>}
          <PresenterSongList add={add} headingId={ids.songs} />
          {/* The reason is a small neutral pill BESIDE the button (it wraps under it on a phone), so it is not read as a heading. */}
          {!running && !finished ? (
            <ActionRow align="center">
              <ActionButton aria-describedby={reason ? ids.reason : undefined} busy={working} busyLabel={IMPORTING} disabled={Boolean(add.blockedBy) || working} label={IMPORT_SONGS} onClick={() => void add.run()} tone="presentation" variant="primary" />
              {add.canSkip && <ActionButton label={SKIP_AND_IMPORT} onClick={add.skipAndAdd} tone="presentation" />}
              <Reason id={ids.reason}>{reason}</Reason>
            </ActionRow>
          ) : (
            <div><Reason id={ids.reason}>{reason}</Reason></div>
          )}
          {phase === "previewed" && made && <PreviewBody add={add} headingId={ids.preview} plan={made} />}
          {running && <Progress add={add} headingId={ids.progress} plan={made} />}
          {finished && (
            <>
              <Result add={add} headingId={ids.result} />
              {report && canTryAgain(report.outcome) && <ActionRow><ActionButton label="Try again" onClick={add.tryAgain} tone="presentation" /></ActionRow>}
            </>
          )}
        </fieldset>
      </OptionalBand>
    </div>
  );
}
