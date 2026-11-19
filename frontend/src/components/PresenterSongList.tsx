import { useId, useState } from "react";

import type { AddSongsController } from "../hooks/useAddSongs";
import { useKeyChangeClock } from "../hooks/useKeyChangeClock";
import { KeyChip, Stamp } from "./AddSongsParts";
import type { AddSongsRow } from "./addSongsSongs";
import {
  ACTION_STAMP, isStuck, NO_NUMBER, NO_NUMBER_WHY, NO_PLAN_SONGS, NOT_IN_PROPRESENTER, NOT_IN_PROPRESENTER_WHY, PLANNING_CENTER, SELECT_ALL, SELECT_NONE,
  selectedWords, SEVERAL_IN_PROPRESENTER, type SongStamp,
} from "./addSongsWording";
import { keyWas, librarySongCover } from "./playbackLibrary";
import { ActionButton, ActionRow, DisclosureButton } from "./sections";
import { SONG_CARD_LOOK, SONG_GRID_CHECK, SongCardBody, SongCardShell } from "./SongCard";

/** What the last preview (while it is still true) and the last import say about a song: the stamp of a song it stopped for, and whether a section was guessed. */
interface Marks { stuck: SongStamp | null; guessed: boolean }

function marksOf(add: AddSongsController): Map<string, Marks> {
  const marks = new Map<string, Marks>();
  const mark = (ref: string, next: Partial<Marks>) => {
    const row = add.previewRows.get(ref);
    if (!row) return;
    const was = marks.get(row.id) ?? { stuck: null, guessed: false };
    marks.set(row.id, { stuck: next.stuck ?? was.stuck, guessed: was.guessed || Boolean(next.guessed) });
  };
  if (add.plan && !add.stale) {
    for (const song of add.plan.songs) mark(song.ref_id, { stuck: isStuck(song) ? ACTION_STAMP[song.action] : null, guessed: song.mapping.some((m) => m.how === "guessed") || song.guesses.length > 0 });
  }
  for (const song of add.report?.songs ?? []) mark(song.ref_id, { guessed: song.guesses.length > 0 });
  return marks;
}

/** A small quiet chip in the key chip's shape: where a card's song comes from. */
function SourceChip({ children }: { children: string }) {
  return <span className="inline-flex flex-none items-center border border-edge-medium bg-surface-950 px-1.5 py-0.5 text-[0.75rem] text-ink-muted" data-testid="add-songs-source-chip">{children}</span>;
}

function SongCard({ add, row, busy, now, marks }: { add: AddSongsController; row: AddSongsRow; busy: boolean; now: number; marks: Marks | undefined }) {
  const [open, setOpen] = useState(false);
  const uid = useId();
  const block = add.blockOf(row);
  const ticked = add.selected.has(row.id);
  const playback = row.source === "playback";
  const hasOrder = row.sections.length > 0;
  const order = row.sections.join(", ");
  const why = block === "missing" ? NOT_IN_PROPRESENTER_WHY : block === "unnamed" ? NO_NUMBER_WHY : null;
  const facts = playback ? (
    <>
      {row.contentId !== null ? <>MultiTracks <span data-testid="add-songs-multitracks">{row.contentId}</span></> : "No MultiTracks id"}
      {" \u00B7 "}
      <span data-testid="add-songs-arrangement" title={hasOrder ? order : undefined}>{row.arrangement ?? "No arrangement name"}</span>
      {hasOrder && <span className="sr-only">{`, sections: ${order}`}</span>}
    </>
  ) : null;
  const chips = (
    <span className="contents" id={`${uid}-chips`}>
      {playback ? <KeyChip keyName={row.keyName} was={row.song ? keyWas(row.song, now) : null} /> : <SourceChip>{PLANNING_CENTER}</SourceChip>}
      {block && <Stamp mark="!" testId="add-songs-blocked" tone="warning" word={block === "missing" ? NOT_IN_PROPRESENTER : block === "several" ? SEVERAL_IN_PROPRESENTER : NO_NUMBER} />}
      {marks?.stuck && <Stamp mark={marks.stuck.mark} testId="add-songs-stuck" tone={marks.stuck.tone} word={marks.stuck.word} />}
      {marks?.guessed && <Stamp mark="?" testId="add-songs-guessed" tone="info" word="Guessed" />}
      {why && <span className="basis-full text-xs text-ink-body" data-testid="add-songs-why">{why}</span>}
    </span>
  );
  const body = (
    <SongCardBody chips={chips} cover={librarySongCover(row.song)} sections={row.song?.sections ?? []} title={row.title}>
      {facts && <span id={`${uid}-facts`}>{facts}</span>}
    </SongCardBody>
  );
  const item = { "data-blocked": block ?? undefined, "data-row": row.id, "data-source": row.source, "data-testid": block === "unnamed" ? "add-songs-unnamed" : "add-songs-row", title: hasOrder ? `${row.title}: ${order}` : undefined };
  // A song Playback gave no number is a card with nothing to tick: it cannot be imported, and says why.
  if (block === "unnamed") return <SongCardShell item={item} look={SONG_CARD_LOOK.empty} mode="static">{body}</SongCardShell>;
  return (
    <SongCardShell
      below={hasOrder && (
        <>
          <DisclosureButton controls={`${uid}-order`} onToggle={() => setOpen((value) => !value)} open={open}>Sections</DisclosureButton>
          {open && <p className="mt-1 break-words text-sm text-ink-body" data-testid="add-songs-order" id={`${uid}-order`}>{`${row.title} plays: ${order}.`}</p>}
        </>
      )}
      describedBy={[facts ? `${uid}-facts` : null, `${uid}-chips`].filter(Boolean).join(" ")}
      disabled={Boolean(block) || busy}
      item={item}
      label={row.title}
      look={block ? SONG_CARD_LOOK.empty : SONG_CARD_LOOK.plain}
      mode="check"
      onActivate={() => add.toggle(row.id)}
      selected={ticked && !block}
    >
      {body}
    </SongCardShell>
  );
}

/**
 * The ONE list of songs, as the same cards as the Lighting tab's songs (`SongCardShell`, check mode): Playback's setlist (MultiTracks id,
 * key, arrangement and its bar; Sections opens the order) or, until Playback has given the songs, the plan's Planning Center names (a
 * Planning Center chip). Every card starts selected; a selection belongs to the song (its title), so the swap between the two keeps it.
 * A song that cannot be imported is a grey dashed card that says why. No sentence explains the list: the chips do.
 */
export function PresenterSongList({ add, headingId }: { add: AddSongsController; headingId: string }) {
  const { rows, choices, selectedCount } = add;
  const busy = add.phase === "previewing" || add.phase === "applying";
  const names = !add.fromPlayback;
  // "(was G)" on a key lasts a day: while one shows, the cards look again every minute so it goes away by itself.
  const now = useKeyChangeClock((at) => rows.some((row) => row.song !== null && keyWas(row.song, at) !== null));
  const marks = marksOf(add);
  return (
    <section aria-labelledby={headingId} className="space-y-2" data-source={names ? "planning-center" : "playback"} data-testid="add-songs-songs">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
        <div className="flex items-baseline gap-x-3">
          <h3 className="font-semibold text-ink" id={headingId}>Songs</h3>
          <span aria-live="polite" className="text-sm tabular-nums text-ink-muted" data-testid="add-songs-count" role="status">{selectedWords(selectedCount, choices.length)}</span>
        </div>
        <ActionRow aria-label="Select songs">
          <ActionButton disabled={busy || choices.length === 0} label={SELECT_ALL} onClick={() => add.toggleAll(true)} tone="presentation" />
          <ActionButton disabled={busy || choices.length === 0} label={SELECT_NONE} onClick={() => add.toggleAll(false)} tone="presentation" />
        </ActionRow>
      </div>
      {rows.length === 0 ? <p className="text-sm text-ink-muted" data-testid="add-songs-empty">{NO_PLAN_SONGS}</p> : (
        <ul aria-label="Songs to import" className={SONG_GRID_CHECK} data-testid="add-songs-list">
          {rows.map((row) => <SongCard add={add} busy={busy} key={row.id} marks={marks.get(row.id)} now={now} row={row} />)}
        </ul>
      )}
    </section>
  );
}
