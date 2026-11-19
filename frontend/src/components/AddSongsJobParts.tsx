import type { AddSongsController } from "../hooks/useAddSongs";
import type { AddSongsGuess, AddSongsJob, AddSongsJobStep, AddSongsPlan, AddSongsReport } from "../propresenterLink";
import { differenceLine, guessPhrase, resultHeading, resultWord, STATUS } from "./addSongsStatus";
import { backupName, guessLine, guessNote, isStuck, OPEN_LIBRARY_ONCE, resultCounts, SONG_STATUS, STEP_STATE, STEP_WORDS } from "./addSongsWording";
import { StatusChip, StatusRow } from "./StatusChips";
import { SectionNotice } from "./sections";
import { cx } from "./ui/cx";
import type { Tone } from "./ui/types";

// ---- the job and its result ---------------------------------------------------------------------------------------------------

const STEP_TONE: Record<AddSongsJobStep["state"], Tone> = { running: "info", done: "success", failed: "danger", note: "idle" };

/** The steps of the job: one short word each in a chip, with a spinner, a check or a cross. What the step said is the hover text and the screen-reader text. */
function StepChips({ steps, label }: { steps: AddSongsJobStep[]; label: string }) {
  return (
    <ol aria-label={label} className="flex flex-wrap gap-1.5" data-testid="add-songs-steps">
      {steps.map((step) => {
        const state = STEP_STATE[step.state];
        const said = [step.text, ...step.notes].filter(Boolean).join(" ");
        return (
          <li data-state={step.state} data-step={step.step} key={step.step}>
            <StatusChip mark={state.mark} sr={`: ${state.word}${said ? `. ${said}` : ""}`} spinner={step.state === "running"} title={said || undefined} tone={STEP_TONE[step.state]} word={STEP_WORDS[step.step]} />
          </li>
        );
      })}
    </ol>
  );
}

export function Progress({ add, plan, headingId }: { add: AddSongsController; plan: AddSongsPlan | null; headingId: string }) {
  const job: AddSongsJob | null = add.job;
  // Before the first answer the songs are the plan's: the list does not grow when the first answer arrives.
  const songs = job?.songs ?? (plan?.songs.filter((song) => !add.skipBlocked || !isStuck(song)).map((song) => ({ ref_id: song.ref_id, title: song.title, steps: [], result: null })) ?? []);
  const last = job?.events[job.events.length - 1];
  return (
    <section aria-labelledby={headingId} className="space-y-2" data-testid="add-songs-progress">
      <h3 className="font-semibold text-ink" id={headingId} tabIndex={-1}>Importing</h3>
      <p className="sr-only" data-testid="add-songs-now">{last ? `Now: ${last.text}` : "Starting\u2026"}</p>
      {add.lostContact && <SectionNotice tone="warning"><span title="StagePilot lost contact with itself for a moment. It keeps trying; the songs are still being imported.">Lost contact: still trying</span></SectionNotice>}
      {job && job.steps.length > 0 ? <StepChips label="Steps for the playlist" steps={job.steps} /> : <StatusChip mark="" spinner tone="info" word="Starting" />}
      <ol aria-label="Songs being imported" className="divide-y divide-dashed divide-edge-medium border-y border-edge-medium" data-testid="add-songs-job-songs">
        {songs.map((song) => {
          const done = song.result ? SONG_STATUS[song.result.status] : null;
          const word = song.result?.status === "failed" ? resultWord(song.result) : done?.word;
          return (
            <StatusRow
              chip={done ? <StatusChip column mark={done.mark} testId="add-songs-job-status" title={song.result?.text} tone={done.tone} word={word ?? done.word} /> : <span />}
              detail={song.result?.text}
              extra={song.steps.length > 0 ? <StepChips label={`Steps for ${song.title}`} steps={song.steps} /> : undefined}
              key={song.ref_id}
              testId="add-songs-job-song"
              title={song.title}
            />
          );
        })}
      </ol>
    </section>
  );
}

/** What the result tells about the sections that were guessed for a song: the `Check` chip and one short line per guess. Information only: nothing here stops or asks for anything. */
export function GuessNote({ guesses, title }: { guesses: AddSongsGuess[]; title: string }) {
  const note = guessNote(guesses);
  if (!note) return null;
  return (
    <div className="flex flex-wrap items-start gap-x-3 gap-y-1" data-testid="add-songs-guess-note">
      <StatusChip {...STATUS.check} title={note.heading} />
      <ul aria-label={`${note.heading} ${title}`} className="min-w-0 space-y-0.5 text-sm text-ink-body">
        {guesses.map((guess) => (
          <li className="break-words" key={guessLine(guess)} title={guessLine(guess)}>
            <span aria-hidden="true">{guessPhrase(guess)}</span>
            <span className="sr-only">{guessLine(guess)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

const HEADING_TONE: Record<string, string> = { success: "text-success-text", warning: "text-warning-text", danger: "text-danger-text", info: "text-info-text" };

export function Result({ add, headingId }: { add: AddSongsController; headingId: string }) {
  const report: AddSongsReport | null = add.report;
  if (!report) {
    return (
      <section aria-labelledby={headingId} className="space-y-1" data-outcome="stopped" data-testid="add-songs-result">
        <h3 className="font-semibold text-warning-text" id={headingId} tabIndex={-1}><span aria-hidden="true">! </span>Stopped</h3>
        <p className="text-sm text-ink-muted" data-testid="add-songs-result-message">{add.job?.error ?? "Look at the playlist in ProPresenter"}</p>
      </section>
    );
  }
  const heading = resultHeading(report);
  const backup = backupName(report.backup);
  const counts = resultCounts(report);
  return (
    <section aria-labelledby={headingId} className="space-y-2" data-outcome={report.outcome} data-testid="add-songs-result">
      <h3 className={cx("font-semibold", HEADING_TONE[heading.tone] ?? "text-ink")} id={headingId} tabIndex={-1}><span aria-hidden="true">{heading.mark} </span>{heading.text}</h3>
      <p className="sr-only" data-testid="add-songs-result-message">{report.message}</p>
      {report.outcome === "needs_library_open" && <SectionNotice tone="warning"><span data-testid="add-songs-open-library">{OPEN_LIBRARY_ONCE}</span></SectionNotice>}
      {counts.includes("\u00B7") && <p className="text-sm tabular-nums text-ink-muted" data-testid="add-songs-counts">{counts}</p>}
      <ul aria-label="Result for each song" className="divide-y divide-dashed divide-edge-medium border-y border-edge-medium" data-testid="add-songs-result-songs">
        {report.songs.map((song) => {
          const status = SONG_STATUS[song.status];
          return (
            <StatusRow
              attrs={{ "data-status": song.status }}
              chip={<StatusChip column mark={status.mark} title={song.text} tone={status.tone} word={song.status === "failed" ? resultWord(song) : status.word} />}
              detail={song.text}
              extra={song.guesses.length > 0 ? <GuessNote guesses={song.guesses} title={song.title} /> : undefined}
              key={song.ref_id}
              testId="add-songs-result-song"
              title={song.title}
            />
          );
        })}
      </ul>
      {report.differences.length > 0 && (
        <ul aria-label="Differences from the plan" className="space-y-0.5 text-sm text-ink-body" data-testid="add-songs-differences">
          {report.differences.map((difference) => <li className="break-words" key={difference} title={difference}>{differenceLine(difference)}</li>)}
        </ul>
      )}
      {backup && <p className="break-words text-xs text-ink-muted" data-testid="add-songs-backup">Backup <code className="font-mono text-ink">{backup}</code></p>}
      {report.version && <p className="text-xs text-ink-muted" data-testid="add-songs-version">{report.version}</p>}
    </section>
  );
}
