import { useCallback, useState } from "react";

import { CUE_TYPES, type CueTypeId } from "./lightingCueTypes";

const STORAGE_KEY = "stagepilot.lighting.hiddenCueTypes";

const readHidden = (): CueTypeId[] => {
  try {
    const parsed: unknown = JSON.parse(window.sessionStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((id): id is CueTypeId => typeof id === "string" && id in CUE_TYPES) : [];
  } catch {
    return [];
  }
};

export interface HiddenCueTypes {
  hidden: ReadonlySet<CueTypeId>;
  toggle: (type: CueTypeId) => void;
  showAll: () => void;
}

/**
 * Which cue types the operator has hidden. View-only: it lives in this browser tab's session storage and is never
 * part of the saved cue map, so it cannot change what a song replays.
 */
export function useHiddenCueTypes(): HiddenCueTypes {
  const [hidden, setHidden] = useState<ReadonlySet<CueTypeId>>(() => new Set(readHidden()));
  const commit = useCallback((next: Set<CueTypeId>) => {
    setHidden(next);
    try { window.sessionStorage.setItem(STORAGE_KEY, JSON.stringify([...next])); } catch { /* private mode: keep it in memory */ }
  }, []);
  const toggle = useCallback((type: CueTypeId) => {
    const next = new Set(hidden);
    if (next.has(type)) next.delete(type); else next.add(type);
    commit(next);
  }, [hidden, commit]);
  const showAll = useCallback(() => commit(new Set()), [commit]);
  return { hidden, toggle, showAll };
}
