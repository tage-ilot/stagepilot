import type { LightkeyStatus } from "../types";

/** Shown on the disabled Read fixtures button when the helper is older than 0.5.0. */
export const UPDATE_HELPER_TIP = "Update the helper to use this.";

/** The line under the connection pill: `Project: Show.lightkeyproj · 66 fixtures`, or plain words while nothing is known. */
export function projectLine(status: LightkeyStatus): string {
  const name = status.project?.trim();
  if (!name) return "Project not reported yet";
  const count = status.fixtures?.fixtures ?? status.helper?.fixtures;
  if (count === undefined) return `Project: ${name}`;
  return `Project: ${name} \u00B7 ${count} fixture${count === 1 ? "" : "s"}`;
}

/** "Captured with X. Lightkey has Y open." only when both projects are known and differ; an old capture (no project) never warns. */
export function projectMismatchText(captured: string | null | undefined, current: string | null | undefined): string | null {
  const was = captured?.trim();
  const now = current?.trim();
  if (!was || !now || was === now) return null;
  return `Captured with ${was}. Lightkey has ${now} open.`;
}

/** The helper reports a version older than 0.5.0 (a helper that reports none counts as older). */
export const helperIsOutdated = (status: LightkeyStatus): boolean => status.helper?.outdated === true;

/** The countdown as `m:ss`. */
export function countdownText(ms: number): string {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
