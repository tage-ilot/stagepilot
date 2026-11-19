import { useEffect, useMemo, useState } from "react";

import { titleKey } from "../components/addSongsSongs";
import type { TitleFound } from "../propresenterLink";
import { usePresenterApi } from "./presenterApi";

/** What is known of a title: ProPresenter's Songs library has it (`found`), has none or two (`missing`, `several`), or has not been asked yet or did not answer (`unknown`: the preview will say). */
export type TitleState = TitleFound | "unknown";

/**
 * Which of the Planning Center names ProPresenter's Songs library has, by file name only (nothing is written, the screen is not
 * touched). Asked once for a set of titles while `enabled`; the answer for the old set stays on screen until the new one arrives, so
 * the list does not jump. A failed look is not an error: the rows are `unknown` and the preview is the judge.
 */
export function useTitleLookup(titles: string[], enabled: boolean): { stateOf: (title: string) => TitleState; looking: boolean } {
  const stagepilot = usePresenterApi();
  const signature = JSON.stringify(titles);
  const [known, setKnown] = useState<Map<string, TitleFound>>(new Map());
  const [asked, setAsked] = useState<string | null>(null);
  useEffect(() => {
    if (!enabled || titles.length === 0) return undefined;
    let cancelled = false;
    void stagepilot.titles(titles)
      .then((answer) => {
        if (cancelled) return;
        setKnown(new Map(answer.titles.map((entry) => [titleKey(entry.title), entry.state])));
        setAsked(signature);
      })
      .catch(() => { if (!cancelled) { setKnown(new Map()); setAsked(signature); } });
    return () => { cancelled = true; };
    // `titles` is carried by its signature: a new array with the same words is not a new question.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, signature, stagepilot]);
  return useMemo(() => ({
    stateOf: (title: string) => known.get(titleKey(title)) ?? "unknown",
    looking: enabled && titles.length > 0 && asked !== signature,
  }), [known, asked, signature, enabled, titles.length]);
}
