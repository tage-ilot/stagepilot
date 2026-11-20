import { useEffect, useId, useMemo, useRef } from "react";

import { useAddSongs } from "../hooks/useAddSongs";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { ServicePlan } from "../types";
import { presenterRows } from "./addSongsSongs";
import { ADD_TO_PROPRESENTER, asTheyAreSentence, canTryAgain, importsSentence, SKIP_AND_ADD } from "./addSongsWording";
import { PlaylistPicker, PreviewBody, Progress, Reason, Result } from "./AddSongsParts";
import { PresenterSongList } from "./PresenterSongList";
import { PRESENTER_LINK_NAME } from "./proPresenterLinkState";
import { ActionButton, ActionRow, SectionNotice } from "./sections";

/**
 * **Presenter Link**, in the ProPresenter (Presentation) panel: which playlist and which section of it, the songs (Playback's, in Playback's
 * order, with MultiTracks id, key and arrangement; until Playback has given them, the plan's Planning Center names), and ONE button that makes the
 * preview and, when nothing in it is blocked, adds the songs. It is drawn only while the Presenter Link is ready (this computer or the
 * helper); otherwise it draws nothing (the feature is optional, so a computer without it shows no warning). Nothing in ProPresenter changes
 * until the button is pressed, and never while a song is playing.
 */
export function PresenterLinkSection({ library, link, plan, playing = false }: { library?: PlaybackLibraryController; link?: ProPresenterLinkController; plan?: Pick<ServicePlan, "songs"> | null; playing?: boolean }) {
  if (!link) return null;
  return <PresenterLinkBody library={library} link={link} plan={plan} playing={playing} />;
}

/** Mounted whenever a controller exists, so a job that is running (or has just ended) is not lost if the Link blinks; it draws only while it may. */
function PresenterLinkBody({ library, link, plan, playing }: { library?: PlaybackLibraryController; link: ProPresenterLinkController; plan?: Pick<ServicePlan, "songs"> | null; playing: boolean }) {
  const rows = useMemo(() => presenterRows(link, library, plan), [link, library, plan]);
  const add = useAddSongs({ rows, link, playing });
  const ids = { songs: useId(), preview: useId(), progress: useId(), result: useId(), reason: useId() };
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

  // The section never outlives the state that allowed it, unless it is running a job (it stays to say how it ended).
  if (!add.linkReady && !running && !finished) return null;

  const playlistName = add.playlist?.name ?? null;
  const reason = running ? "Adding songs. Wait here until it says it is finished." : finished ? null : add.blockedBy ?? add.stopped;
  return (
    <div className="space-y-4" data-phase={phase} data-testid="presenter-link-section">
      <p aria-live="polite" className="sr-only" data-testid="add-songs-live" role="status">{add.announcement}</p>
      <PlaylistPicker add={add} title={PRESENTER_LINK_NAME} />
      {!add.linkReady && <SectionNotice tone="warning"><span data-testid="add-songs-link-gone">{`The ${PRESENTER_LINK_NAME} is not ready now. Nothing was changed.`}</span></SectionNotice>}
      {playing && !running && !finished && <SectionNotice tone="info"><span data-testid="add-songs-playing">A song is playing. You can look at a preview now, but songs can only be added when it has ended.</span></SectionNotice>}
      {add.problem && <SectionNotice tone="danger"><span data-code={add.problem.code ?? undefined} data-testid="add-songs-problem">{add.problem.message}</span></SectionNotice>}
      <PresenterSongList add={add} headingId={ids.songs} />
      <div className="space-y-3">
        {(reason || (!running && !finished)) && <Reason id={ids.reason} note={!running && !finished ? (add.fromPlayback ? importsSentence : asTheyAreSentence)(playlistName) : undefined}>{reason}</Reason>}
        {!running && !finished && (
          <ActionRow>
            <ActionButton aria-describedby={reason ? ids.reason : undefined} busy={working} busyLabel={"Making the preview\u2026"} disabled={Boolean(add.blockedBy) || working} label={ADD_TO_PROPRESENTER} onClick={() => void add.run()} tone="presentation" variant="primary" />
            {add.canSkip && <ActionButton label={SKIP_AND_ADD} onClick={add.skipAndAdd} tone="presentation" />}
          </ActionRow>
        )}
      </div>
      {phase === "previewed" && made && <PreviewBody add={add} headingId={ids.preview} plan={made} />}
      {running && <Progress add={add} headingId={ids.progress} plan={made} />}
      {finished && (
        <>
          <Result add={add} headingId={ids.result} />
          {report && canTryAgain(report.outcome) && <ActionRow><ActionButton label="Try again" onClick={add.tryAgain} tone="presentation" /></ActionRow>}
        </>
      )}
    </div>
  );
}
