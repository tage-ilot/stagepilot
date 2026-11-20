import { useCallback, useEffect, useRef, useState } from "react";

/** How long "Checked again." stays in the status line. */
export const RECHECK_NOTE_MS = 8_000;

/**
 * Connect on a block that is already connected only checks again. The status line says so for a few seconds, so the press is never
 * silent: `note` is the words (or null), `mark` starts them, `clear` ends them (a new scan, an edit).
 */
export function useRecheckNote(words = "Checked again."): { note: string | null; mark: () => void; clear: () => void } {
  const [on, setOn] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const clear = useCallback(() => {
    window.clearTimeout(timer.current);
    setOn(false);
  }, []);
  const mark = useCallback(() => {
    window.clearTimeout(timer.current);
    setOn(true);
    timer.current = window.setTimeout(() => setOn(false), RECHECK_NOTE_MS);
  }, []);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return { note: on ? words : null, mark, clear };
}
