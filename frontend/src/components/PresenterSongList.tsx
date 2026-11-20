import { useId, useState } from "react";

import type { AddSongsController } from "../hooks/useAddSongs";
import { KeyChip, linkButton, Stamp } from "./AddSongsParts";
import type { AddSongsRow } from "./addSongsSongs";
import { KEY_NOTE, NO_PLAN_SONGS, NOT_IN_PROPRESENTER, NOT_IN_PROPRESENTER_WHY, PC_NAMES_NOTE, PLAYBACK_NOTE, SEVERAL_IN_PROPRESENTER } from "./addSongsWording";
import { DisclosureButton, SectionNotice } from "./sections";
import { cx } from "./ui/cx";

/** A cell with nothing to say yet (a Planning Center name has no number, key or arrangement): a dash for the eye, words for a screen reader. */
const Nothing = () => (<><span aria-hidden="true" className="text-ink-muted">{"\u2013"}</span><span className="sr-only">not known yet</span></>);

// One row is a table row from 560 px; narrower it is a card: the checkbox and the title on top, the facts under it with their names.
const ROW = "min-[560px]:table-row max-[559px]:grid max-[559px]:grid-cols-[auto_minmax(0,1fr)] max-[559px]:gap-x-3 max-[559px]:gap-y-1 max-[559px]:py-2";
const CELL = "min-[560px]:table-cell min-[560px]:px-2 min-[560px]:py-2 min-[560px]:align-middle";
const FACT = "max-[559px]:col-start-2 max-[559px]:flex max-[559px]:flex-wrap max-[559px]:items-center max-[559px]:gap-x-2";

function SongRow({ add, row, busy }: { add: AddSongsController; row: AddSongsRow; busy: boolean }) {
  const [open, setOpen] = useState(false);
  const uid = useId();
  const block = add.blockOf(row);
  const ticked = add.selected.has(row.id);
  const why = block === "missing" ? NOT_IN_PROPRESENTER_WHY : block === "several" ? SEVERAL_IN_PROPRESENTER : null;
  const hasOrder = row.sections.length > 0;
  return (
    <>
      <tr className={cx(ROW, "border-t border-dashed border-edge-medium", block && "text-ink-muted")} data-blocked={block ?? undefined} data-row={row.id} data-source={row.source} data-testid="add-songs-row">
        <td className={cx(CELL, "min-[560px]:w-10")}>
          <input aria-describedby={block ? `${uid}-why` : undefined} checked={ticked} className="size-5 flex-none accent-integration-presentation" disabled={Boolean(block) || busy} id={`${uid}-box`} onChange={() => add.toggle(row.id)} type="checkbox" />
        </td>
        <th className={cx(CELL, "min-w-0 break-words text-left font-semibold", block ? "text-ink-muted" : "text-ink")} scope="row">
          <label className="cursor-pointer" htmlFor={`${uid}-box`}>{row.title}</label>
          {block && (
            <span className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 font-normal" id={`${uid}-why`}>
              <Stamp mark="!" tone="warning" word={block === "missing" ? NOT_IN_PROPRESENTER : SEVERAL_IN_PROPRESENTER} />
              {why && <span className="text-sm">{why}</span>}
            </span>
          )}
        </th>
        <td className={cx(CELL, FACT, "tabular-nums text-sm")} data-label="MultiTracks id">
          <span className="min-[560px]:hidden text-ink-muted">MultiTracks id</span>
          {row.contentId !== null ? <span data-testid="add-songs-multitracks">{row.contentId}</span> : <Nothing />}
        </td>
        <td className={cx(CELL, FACT)}>
          {row.source === "playback" ? <KeyChip keyName={row.keyName} /> : <Nothing />}
        </td>
        <td className={cx(CELL, FACT, "text-sm text-ink-body")}>
          <span className="min-[560px]:hidden text-ink-muted">Arrangement</span>
          {row.source === "playback" ? <span data-testid="add-songs-arrangement" title={hasOrder ? row.sections.join(", ") : undefined}>{row.arrangement ?? "No arrangement name"}</span> : <Nothing />}
          {hasOrder && <DisclosureButton controls={`${uid}-order`} onToggle={() => setOpen((value) => !value)} open={open}>Sections</DisclosureButton>}
        </td>
      </tr>
      {hasOrder && open && (
        <tr className="max-[559px]:block">
          <td className="min-[560px]:px-2 min-[560px]:pb-2" colSpan={5} id={`${uid}-order`}>
            <p className="break-words text-sm text-ink-body" data-testid="add-songs-order">{`${row.title} plays: ${row.sections.join(", ")}.`}</p>
          </td>
        </tr>
      )}
    </>
  );
}

/**
 * The ONE list of songs, whichever source it comes from: Playback's setlist (title, MultiTracks id, key, arrangement; Sections shows the order)
 * or, until Playback has given the songs, the plan's Planning Center names. The columns and the row heights are the same in both, so the list
 * does not jump when the data arrives or is lost; a tick belongs to the song, so it survives the swap. A name ProPresenter does not have cannot be
 * ticked and says why.
 */
export function PresenterSongList({ add, headingId }: { add: AddSongsController; headingId: string }) {
  const { rows, choices, selectedCount } = add;
  const all = selectedCount === choices.length;
  const busy = add.phase === "previewing" || add.phase === "applying";
  const names = !add.fromPlayback;
  const listed = rows.filter((row) => !(row.source === "playback" && row.unnamed));
  const unnamed = rows.filter((row) => row.source === "playback" && row.unnamed);
  return (
    <section aria-labelledby={headingId} data-source={names ? "planning-center" : "playback"} data-testid="add-songs-songs">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <h3 className="font-semibold text-ink" id={headingId}>Songs</h3>
        <div className="flex flex-wrap items-center gap-x-3">
          <span className="text-sm tabular-nums text-ink-muted" data-testid="add-songs-count">{selectedCount} of {choices.length} ticked</span>
          <button className={linkButton} disabled={busy || choices.length === 0} onClick={() => add.toggleAll(!all)} type="button">{all ? "Untick all" : "Tick all"}</button>
        </div>
      </div>
      <SectionNotice className="mt-1" live role="status" testId="add-songs-source" tone="info" variant="line">{names ? PC_NAMES_NOTE : PLAYBACK_NOTE}</SectionNotice>
      {listed.length === 0 && unnamed.length === 0 ? <p className="mt-2 text-sm text-ink-muted" data-testid="add-songs-empty">{NO_PLAN_SONGS}</p> : (
        <table aria-label="Songs to add" className="mt-1 w-full border-y border-edge-medium text-left min-[560px]:table-fixed" data-testid="add-songs-list">
          <thead className="max-[559px]:sr-only">
            <tr className="min-[560px]:table-row">
              <th className="px-2 py-1.5 text-type-kicker font-type-kicker uppercase text-ink-muted min-[560px]:w-10" scope="col"><span className="sr-only">Add</span></th>
              <th className="px-2 py-1.5 font-type-kicker text-type-kicker uppercase text-ink-muted" scope="col">Song</th>
              <th className="px-2 py-1.5 font-type-kicker text-type-kicker uppercase text-ink-muted min-[560px]:w-32" scope="col">MultiTracks id</th>
              <th className="px-2 py-1.5 font-type-kicker text-type-kicker uppercase text-ink-muted min-[560px]:w-24" scope="col">Key</th>
              <th className="px-2 py-1.5 font-type-kicker text-type-kicker uppercase text-ink-muted min-[560px]:w-[40%]" scope="col">Arrangement</th>
            </tr>
          </thead>
          <tbody>{listed.map((row) => <SongRow add={add} busy={busy} key={row.id} row={row} />)}</tbody>
        </table>
      )}
      {unnamed.length > 0 && (
        <ul aria-label="Songs without a number" className="mt-1 divide-y divide-dashed divide-edge-medium border-b border-edge-medium">
          {unnamed.map((row) => (
            <li className="flex min-h-ds-touch-target flex-wrap items-center gap-x-3 gap-y-1 py-2 text-ink-muted" data-testid="add-songs-unnamed" key={row.id}>
              <span className="min-w-0 flex-[1_1_12rem] break-words">{row.title}</span>
              <span className="text-sm">Playback gave this song no number, so it cannot be added.</span>
            </li>
          ))}
        </ul>
      )}
      {!names && <p className="mt-2 text-sm text-ink-muted" data-testid="add-songs-key-note">{KEY_NOTE}</p>}
    </section>
  );
}
