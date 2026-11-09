import { useCallback, useSyncExternalStore } from "react";

import type { SettingsResponse } from "../types";

/** One calm line about the settings file: what the operator reads, and what it was about. */
export interface SettingsNoticeItem {
  /** Same text, same key: a different notice (another song) is a new notice, shown again. */
  key: string;
  kind: "notice" | "degraded";
  /** A short heading in plain words. */
  lead: string;
  text: string;
}

export const NOTICE_LEAD = "A song's lights need recording again";
export const DEGRADED_LEAD = "StagePilot could not read its saved settings";
export const NEWER_LEAD = "These settings are from a newer StagePilot";

/** Empty when there is nothing to say. The song name comes inside the backend's own sentence. */
export function settingsNotices(settings: SettingsResponse | null | undefined, only?: "notice"): SettingsNoticeItem[] {
  if (!settings) return [];
  const items: SettingsNoticeItem[] = [];
  if (settings.settings_degraded && only !== "notice") {
    const newer = /newer/i.test(settings.settings_degraded_reason ?? "");
    items.push(newer
      ? { key: "degraded:newer", kind: "degraded", lead: NEWER_LEAD, text: "StagePilot is using its default settings and will not change the saved file. Open the newer StagePilot to see your settings again." }
      : { key: "degraded:corrupt", kind: "degraded", lead: DEGRADED_LEAD, text: "It started with the default settings, so Planning Center, Lighting and the other saved settings are not loaded." });
  }
  const notice = settings.settings_notice?.trim();
  if (notice) items.push({ key: `notice:${notice}`, kind: "notice", lead: NOTICE_LEAD, text: notice });
  return items;
}

// "Dismissible per session": remembered until the window or tab is closed, never saved on disk. Where the
// banner stands (main screen, Lighting screen) is part of the key, so closing one does not hide the other.
const STORAGE_KEY = "stagepilot.dismissed-settings-notices";
const listeners = new Set<() => void>();
let dismissed: ReadonlySet<string> | null = null;

function read(): ReadonlySet<string> {
  if (dismissed) return dismissed;
  let stored: string[] = [];
  try {
    const parsed: unknown = JSON.parse(window.sessionStorage.getItem(STORAGE_KEY) ?? "[]");
    if (Array.isArray(parsed)) stored = parsed.filter((item): item is string => typeof item === "string");
  } catch {
    /* no session storage: the memory below still holds for this page */
  }
  dismissed = new Set(stored);
  return dismissed;
}

function dismissKey(scope: string, key: string) {
  return `${scope}|${key}`;
}

/** Tests: forget every dismissal. */
export function resetDismissedSettingsNotices() {
  dismissed = new Set();
  try { window.sessionStorage.removeItem(STORAGE_KEY); } catch { /* nothing to clear */ }
  listeners.forEach((listener) => listener());
}

export function useDismissedSettingsNotices(scope: string) {
  const snapshot = useSyncExternalStore(
    (listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    read,
  );
  const dismiss = useCallback((key: string) => {
    const next = new Set(read());
    next.add(dismissKey(scope, key));
    dismissed = next;
    try { window.sessionStorage.setItem(STORAGE_KEY, JSON.stringify([...next])); } catch { /* memory only */ }
    listeners.forEach((listener) => listener());
  }, [scope]);
  const isDismissed = useCallback((key: string) => snapshot.has(dismissKey(scope, key)), [scope, snapshot]);
  return { dismiss, isDismissed };
}
