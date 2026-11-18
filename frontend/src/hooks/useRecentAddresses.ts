import { useEffect, useState } from "react";

/** The three places that ask for a computer's address and so remember the last few that worked. */
export type RecentScope = "playback" | "lighting" | "presentation";

const MAX_RECENT = 5;
const CHANGED = "stagepilot-recent-addresses";
const keyOf = (scope: RecentScope) => `stagepilot.recent-addresses.${scope}`;

/** A bare host name or address, like the settings accept: no spaces, no scheme, no port, no path. */
const plausible = (value: unknown): value is string =>
  typeof value === "string" && value.length > 0 && value.length <= 255 && !/[\s/?#@]/.test(value) && !value.includes("://");

export function readRecentAddresses(scope: RecentScope): string[] {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(keyOf(scope)) ?? "[]");
    if (!Array.isArray(parsed)) return [];
    return [...new Set(parsed.filter(plausible))].slice(0, MAX_RECENT);
  } catch {
    return [];
  }
}

/**
 * Remembers an address that worked, newest first, at most five, no duplicates (a name is the same name in any letter case). Kept in
 * this browser's localStorage like the other window choices; if storage is unavailable it silently does nothing and the scan still works.
 */
export function rememberAddress(scope: RecentScope, address: string | null | undefined): void {
  const next = address?.trim();
  if (!next || !plausible(next)) return;
  const current = readRecentAddresses(scope);
  if (current[0]?.toLowerCase() === next.toLowerCase()) return;
  const updated = [next, ...current.filter((item) => item.toLowerCase() !== next.toLowerCase())].slice(0, MAX_RECENT);
  try {
    window.localStorage.setItem(keyOf(scope), JSON.stringify(updated));
    window.dispatchEvent(new Event(CHANGED));
  } catch {
    // Best effort.
  }
}

export function useRecentAddresses(scope: RecentScope): string[] {
  const [recent, setRecent] = useState(() => readRecentAddresses(scope));
  useEffect(() => {
    const refresh = () => setRecent(readRecentAddresses(scope));
    refresh();
    window.addEventListener(CHANGED, refresh);
    window.addEventListener("storage", refresh);
    return () => { window.removeEventListener(CHANGED, refresh); window.removeEventListener("storage", refresh); };
  }, [scope]);
  return recent;
}
