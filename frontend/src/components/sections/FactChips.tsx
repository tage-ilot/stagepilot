import { Fragment, type ReactNode } from "react";

import { cx } from "../ui/cx";

/** What a fact's words say, by words AND colour (a colour alone never carries it). Neutral unless the words already carried a tone. */
export type FactTone = "neutral" | "success" | "warning" | "danger";

export interface Fact {
  /** The words on the chip. Also the spoken name unless `name` is given, and the hover text when they are long. */
  text: string;
  /** What a screen reader says for this fact when the chip's words alone are not enough (`Key Ab` for the chip `Ab`). */
  name?: string;
  /** The chip's own content when it holds more than plain text (a number with its own test id). Its words must be `text`. */
  content?: ReactNode;
  tone?: FactTone;
  /** The hover text. Left out: the full words, but only when they are long enough to be cut. */
  title?: string;
  testId?: string;
}

export interface FactChipsProps {
  facts: readonly Fact[];
  /** Names the list for a screen reader. Left out: the facts' names, joined (`Key Ab, length 4:37`). */
  label?: string;
  className?: string;
  testId?: string;
}

/** Words longer than this can be cut on a narrow card, so they carry themselves as the hover text. */
export const LONG_FACT = 24;

/** The chip: ONE look for every song fact on every page (the square chip the Presenter Link's key used to wear). */
const CHIP = "inline-flex min-w-0 max-w-full items-center border bg-surface-950 px-1.5 py-0.5 font-mono text-[0.8125rem] tabular-nums";
const TONES: Record<FactTone, string> = {
  neutral: "border-edge-medium text-ink",
  success: "border-success-soft text-success-text",
  warning: "border-warning/60 text-warning-text",
  danger: "border-danger/60 text-danger-text",
};

/**
 * The FACTS section: a song's small facts (key, length, BPM, mtID, capture status) as a row of square chips, one chip per fact, a small gap
 * between them and NO separator character (no dot, no bullet): the box is the separator. The row wraps onto a second line only when the
 * card is too narrow, and a long fact is cut inside its own chip with its full words as the hover text.
 *
 * Use it wherever a list of a song's facts was written as one dotted line. NOT for a sentence or a status line that merely has a dot
 * between clauses. The words are the facts' own (`Ab`, `4:37`, `65 BPM`, `mtID-23126`); a tone is only for words that already said it
 * (`Not captured`), never new colour. It is a list for a screen reader (`role="list"` on a span, so it is also valid inside a button
 * that is a whole card), named by the facts' own names. No facts: nothing is drawn.
 */
export function FactChips({ facts, label, className, testId }: FactChipsProps) {
  if (!facts.length) return null;
  const spoken = label ?? facts.map((fact) => fact.name ?? fact.text).join(", ");
  return (
    <span aria-label={spoken} className={cx("flex min-w-0 flex-wrap items-center gap-1", className)} data-part="facts" data-testid={testId} role="list">
      {facts.map((fact, index) => {
        const tone = fact.tone ?? "neutral";
        return (
          // The plain space between two chips is not drawn (flex items), but a reader or a copy gets `Ab 4:37`, not `Ab4:37`.
          <Fragment key={`${index}-${fact.text}`}>
            {index > 0 && " "}
            <span
              className={cx(CHIP, TONES[tone])}
              data-testid={fact.testId}
              data-tone={tone}
              role="listitem"
              title={fact.title ?? (fact.text.length > LONG_FACT ? fact.text : undefined)}
            >
              <span className="min-w-0 truncate">{fact.content ?? fact.text}</span>
            </span>
          </Fragment>
        );
      })}
    </span>
  );
}
