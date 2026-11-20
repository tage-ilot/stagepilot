import { Fragment, useId } from "react";

import type { AddSongsController } from "../hooks/useAddSongs";
import { useKeyChangeClock } from "../hooks/useKeyChangeClock";
import type { AddSongsRow } from "./addSongsSongs";
import { songStatus, STATUS, type StatusWords } from "./addSongsStatus";
import {
  isStuck, NO_NUMBER, NO_NUMBER_PHRASE, NO_NUMBER_WHY, NO_PLAN_SONGS, NOT_IN_PROPRESENTER, NOT_IN_PROPRESENTER_PHRASE, NOT_IN_PROPRESENTER_WHY, PLANNING_CENTER,
  SELECT_ALL, SELECT_NONE, selectedWords, SEVERAL_IN_PROPRESENTER, SEVERAL_IN_PROPRESENTER_WHY,
} from "./addSongsWording";
import { StatusChip } from "./StatusChips";
import { keyWas, librarySongCover } from "./playbackLibrary";
import { ActionButton, ActionRow, FactChips } from "./sections";
import { SONG_CARD_LOOK, SONG_GRID_CHECK, SongCardBody, SongCardShell } from "./SongCard";

/** What the last preview (while it is still true) and the last import say about a song: the chip of a song it stopped for, and whether a section was guessed. */
interface Marks { stuck: StatusWords | null; guessed: boolean }

function marksOf(add: AddSongsController): Map<string, Marks> {
  const marks = new Map<string, Marks>();
  const mark = (ref: string, next: Partial<Marks>) => {
    const row = add.previewRows.get(ref);
    if (!row) return;
    const was = marks.get(row.id) ?? { stuck: null, guessed: false };
    marks.set(row.id, { stuck: next.stuck ?? was.stuck, guessed: was.guessed || Boolean(next.guessed) });
  };
  if (add.plan && !add.stale) {
    for (const song of add.plan.songs) mark(song.ref_id, { stuck: isStuck(song) ? songStatus(song) : null, guessed: song.mapping.some((m) => m.how === "guessed") || song.guesses.length > 0 });
  }
  for (const song of add.report?.songs ?? []) mark(song.ref_id, { guessed: song.guesses.length > 0 });
  return marks;
}

/** A small quiet chip in the status chip's shape: where a card's song comes from. */
function SourceChip({ children }: { children: string }) {
  return <span className="inline-flex flex-none items-center border border-edge-medium bg-surface-950 px-2 py-0.5 text-[0.8125rem] text-ink-muted" data-testid="add-songs-source-chip">{children}</span>;
}

/** Why a card cannot be ticked, as a chip, one short phrase, and the full sentence for hover and screen readers. */
const BLOCK_CHIP = {
  missing: { word: NOT_IN_PROPRESENTER, phrase: NOT_IN_PROPRESENTER_PHRASE, why: NOT_IN_PROPRESENTER_WHY, tone: "danger" as const },
  several: { word: SEVERAL_IN_PROPRESENTER, phrase: "Several files", why: SEVERAL_IN_PROPRESENTER_WHY, tone: "warning" as const },
  unnamed: { word: NO_NUMBER, phrase: NO_NUMBER_PHRASE, why: NO_NUMBER_WHY, tone: "danger" as const },
};

function SongCard({ add, row, busy, now, marks }: { add: AddSongsController; row: AddSongsRow; busy: boolean; now: number; marks: Marks | undefined }) {
  const uid = useId();
  const block = add.blockOf(row);
  const ticked = add.selected.has(row.id);
  const playback = row.source === "playback";
  const hasOrder = row.sections.length > 0;
  const order = row.sections.join(", ");
  const was = playback && row.song ? keyWas(row.song, now) : null;
  const stopped = block ? BLOCK_CHIP[block] : null;
  // The key and `mtID 23126`, one square chip each (`FactChips`, no dot between). The number is the MultiTracks Song Master ID (what ProPresenter shows as MTID), never Playback's own song number.
  const keyWords = row.keyName ? `${row.keyName}${was ? ` (was ${was})` : ""}` : "No key";
  const mtid = row.contentId !== null ? String(row.contentId) : "\u2014";
  const facts = playback ? (
    <FactChips facts={[
      { text: keyWords, name: row.keyName ? `Key ${keyWords}` : keyWords, testId: "add-songs-key" },
      { text: `mtID ${mtid}`, content: <>mtID <span data-testid="add-songs-mtid">{mtid}</span></> },
    ]} />
  ) : null;
  const chips = (
    <span className="contents" id={`${uid}-chips`}>
      {!playback && <SourceChip>{PLANNING_CENTER}</SourceChip>}
      {stopped && block && <><StatusChip mark="!" testId="add-songs-blocked" title={stopped.why} tone={stopped.tone} word={stopped.word} /><span className="text-xs text-ink-body" data-testid="add-songs-why" title={stopped.why}>{stopped.phrase}</span></>}
      {marks?.stuck && <StatusChip {...marks.stuck} testId="add-songs-stuck" />}
      {marks?.guessed && <StatusChip {...STATUS.check} testId="add-songs-guessed" />}
    </span>
  );
  // The order Playback plays, always on the card (no button, no heading), UNDER the colored bar: the section names, wrapping onto as many lines as it needs.
  // Each name stays whole (Verse 2 never splits across two lines); the dot goes with the name before it.
  const orderText = hasOrder ? (
    <span className="mt-1 block break-words text-xs text-ink-body" data-testid="add-songs-order" id={`${uid}-order`}>
      {row.sections.map((name, index) => <Fragment key={index}>{index > 0 && " "}<span className="whitespace-nowrap">{index < row.sections.length - 1 ? `${name} \u00B7` : name}</span></Fragment>)}
    </span>
  ) : null;
  const body = (
    <SongCardBody below={orderText} chips={chips} cover={librarySongCover(row.song)} facts={facts && <span className="block" id={`${uid}-facts`}>{facts}</span>} sections={row.song?.sections ?? []} title={row.title} />
  );
  const item = { "data-blocked": block ?? undefined, "data-row": row.id, "data-source": row.source, "data-testid": block === "unnamed" ? "add-songs-unnamed" : "add-songs-row", title: hasOrder ? `${row.title}: ${order}` : undefined };
  // A song Playback gave no number is a card with nothing to tick: it cannot be imported, and says why.
  if (block === "unnamed") return <SongCardShell item={item} look={SONG_CARD_LOOK.empty} mode="static">{body}</SongCardShell>;
  return (
    <SongCardShell
      describedBy={[facts ? `${uid}-facts` : null, hasOrder ? `${uid}-order` : null, `${uid}-chips`].filter(Boolean).join(" ")}
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
 * The ONE list of songs, as the same cards as the Lighting tab's songs (`SongCardShell`, check mode): Playback's setlist (the key and
 * `mtID` on one plain line, the order Playback plays written out, and the arrangement bar) or, until Playback has given the songs, the plan's Planning Center names (a
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
