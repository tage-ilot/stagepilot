import { useCallback, useState } from "react";

/**
 * A true/false setting that is remembered between launches (the Backend panel's Remote access section stays collapsed or open as
 * the user left it). Kept in localStorage like the dashboard layout; if storage is unavailable or holds something unexpected it
 * simply falls back to the default and the choice lasts for the current session only.
 */
export function usePersistedFlag(key: string, initial: boolean): [boolean, (next: boolean) => void] {
  const [value, setValue] = useState<boolean>(() => {
    try {
      const saved = window.localStorage.getItem(key);
      return saved === "true" ? true : saved === "false" ? false : initial;
    } catch {
      return initial;
    }
  });
  const set = useCallback((next: boolean) => {
    setValue(next);
    try {
      window.localStorage.setItem(key, String(next));
    } catch {
      // Best effort: the choice still applies until the app closes.
    }
  }, [key]);
  return [value, set];
}
