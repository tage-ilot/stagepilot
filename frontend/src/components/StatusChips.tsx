import type { ReactNode } from "react";

import type { StatusWords } from "./addSongsStatus";
import { cx } from "./ui/cx";
import type { Tone } from "./ui/types";

/**
 * The ONE status chip of the Presenter Link (preview, progress, result and the song cards): an icon and a word, in a square outline whose
 * colour is the status token's. The word is the status (colour never carries it alone); the long sentence is only the hover text and the
 * screen-reader text. Tokens only (DESIGN.md).
 */
const CHIP_TONE: Partial<Record<Tone, string>> = {
  success: "border-success-soft text-success-text",
  info: "border-info/60 text-info-text",
  warning: "border-warning/60 text-warning-text",
  danger: "border-danger/60 text-danger-text",
  idle: "border-edge-medium text-ink-muted",
};

export interface StatusChipProps extends StatusWords {
  /** The long sentence: hover text. */
  title?: string;
  testId?: string;
  /** In a list row: as wide as the chip column, so the chips line up. */
  column?: boolean;
  /** The icon is a small spinner (a step that is running). */
  spinner?: boolean;
  /** Said after the word for a screen reader only (`: Working`). */
  sr?: string;
}

export function StatusChip({ word, mark, tone, title, testId, column = false, spinner = false, sr }: StatusChipProps) {
  return (
    <span
      className={cx("inline-flex min-w-0 items-center gap-1.5 border bg-surface-950 px-2 py-0.5 text-[0.8125rem] font-semibold leading-snug", column ? "w-fit max-w-full sm:w-full" : "flex-none", CHIP_TONE[tone] ?? CHIP_TONE.idle)}
      data-testid={testId}
      data-tone={tone}
      title={title}
    >
      {spinner
        ? <span aria-hidden="true" className="inline-block size-3 flex-none animate-spin rounded-full border-2 border-current border-t-transparent motion-reduce:animate-none" data-part="spinner" />
        : <span aria-hidden="true" className="w-3 flex-none text-center font-mono">{mark}</span>}
      <span className="min-w-0 break-words">{word}</span>
      {sr && <span className="sr-only">{sr}</span>}
    </span>
  );
}

/** The columns of a status row on a wide screen: the song, its chip, its phrase. On a narrow one they stack: title, chip, then the phrase. */
export const STATUS_ROW = "grid gap-x-3 gap-y-1 py-2 sm:grid-cols-[minmax(10rem,16rem)_8.5rem_minmax(0,1fr)] sm:items-start";

/** One song with its status: the title in bold, the chip, and under or beside it at most one short phrase (and for Pick one its files, one per line). */
export function StatusRow({ title, chip, phrase, files = [], detail, extra, testId, attrs }: {
  title: string;
  chip: ReactNode;
  phrase?: string | null;
  files?: readonly string[];
  /** The long sentences, for a screen reader only. */
  detail?: string;
  /** More under the row, across its whole width (a song's steps, what to check). */
  extra?: ReactNode;
  testId: string;
  attrs?: Record<string, string | undefined>;
}) {
  return (
    <li className={STATUS_ROW} data-testid={testId} {...attrs}>
      <span className="min-w-0 break-words font-semibold text-ink">{title}</span>
      {chip}
      {(phrase || files.length > 0) && (
        <div className="min-w-0 space-y-1 text-sm text-ink-body">
          {phrase && <p className="break-words" data-testid="add-songs-phrase">{phrase}</p>}
          {files.length > 0 && (
            <ul aria-label={`Files for ${title}`} className="space-y-1" data-testid="add-songs-files">
              {files.map((file) => <li className="w-fit max-w-full break-all border border-edge-medium bg-surface-950 px-1.5 py-0.5 font-mono text-xs text-ink" key={file}>{file}</li>)}
            </ul>
          )}
        </div>
      )}
      {detail && <span className="sr-only" data-testid="add-songs-song-sentence">{detail}</span>}
      {extra && <div className="min-w-0 sm:col-span-3">{extra}</div>}
    </li>
  );
}
