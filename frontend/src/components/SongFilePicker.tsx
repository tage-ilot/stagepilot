import { useRef, type KeyboardEvent } from "react";

import { fileLabels } from "./addSongsWording";
import { Button } from "./ui/Button";

/**
 * The files of a song that ProPresenter has more than once, as one small button each, under the song's title on its card: a radio group
 * (`role="radiogroup"`, one tab stop; the arrow keys, Home and End move between the buttons, Space and Enter pick, so a key press never asks
 * StagePilot for anything by accident). Each button says the file's name without `.pro` (the library too only when two libraries share a
 * name); the one in use is checked and has a check mark (never colour alone). Pressing one asks for a new preview: that button shows the
 * loading sweep and nothing else on the card moves. At least 44 px tall, wrapping onto more lines on a narrow screen.
 */
export function SongFilePicker({ title, files, current, loadingFile, onPick }: {
  title: string;
  /** The files as the preview's `choices` writes them (`Songs / Firm Foundation.pro`). */
  files: readonly string[];
  /** The file in use, or `null` when none is (Pick one). */
  current: string | null;
  /** The file whose preview is being asked for. */
  loadingFile: string | null;
  onPick: (file: string) => void;
}) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const labels = fileLabels(files);
  const tabStop = Math.max(0, current ? files.indexOf(current) : 0);
  const move = (index: number) => buttons.current[(index + files.length) % files.length]?.focus();
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const to = event.key === "ArrowRight" || event.key === "ArrowDown" ? index + 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? index - 1 : event.key === "Home" ? 0 : event.key === "End" ? files.length - 1 : null;
    if (to === null) return;
    event.preventDefault();
    move(to);
  };
  return (
    <div aria-label={`${title}: use file`} className="flex flex-wrap gap-1.5" data-testid="add-songs-file-picker" role="radiogroup">
      {files.map((file, index) => {
        const checked = file === current;
        return (
          <Button
            aria-checked={checked}
            className="max-w-full"
            data-testid="add-songs-file"
            key={file}
            loading={loadingFile === file}
            onClick={() => onPick(file)}
            onKeyDown={(event) => onKeyDown(event, index)}
            ref={(node) => { buttons.current[index] = node; }}
            role="radio"
            size="touch"
            tabIndex={index === tabStop ? 0 : -1}
            tone="presentation"
            variant="tinted"
          >
            {checked && <span aria-hidden="true" className="mr-1.5 font-mono">{"\u2713"}</span>}
            <span className="min-w-0 break-words">{labels[index]}</span>
          </Button>
        );
      })}
    </div>
  );
}
