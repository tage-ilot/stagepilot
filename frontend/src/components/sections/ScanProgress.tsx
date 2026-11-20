import type { ScanState } from "./scanState";

export interface ScanWording {
  /** The progress line before the scan says more ("Looking for Playback…"). */
  looking: string;
  /** The chooser's title for `count` computers ("Choose which Playback"). Also its accessible name. */
  choose: (count: number) => string;
  /** What the help says when nothing answered. Left out: the scan's own reason. */
  notFound?: string;
  /** The checklist under "Make sure:". Empty: no checklist. */
  checklist: string[];
  /** The name of what the person types instead ("Enter address instead"). */
  other?: string;
}

/** The ONE progress line of a scan, in every block: what it is doing, how far, how long (`Looking for Playback… — Checked 64 of 254 addresses (3s)`). Drawn only while it scans. */
function scanProgressText(scan: ScanState, wording: ScanWording): string {
  const phase = scan.phase ?? (scan.typed ? "Connecting…" : wording.looking);
  const counts = scan.total > 1 ? ` Checked ${scan.current} of ${scan.total} addresses` : "";
  // A search that does not time itself (Lighting's) says no seconds.
  const seconds = scan.elapsed === undefined ? "" : ` (${Math.max(0, Math.floor(scan.elapsed))}s)`;
  return `${phase}${counts ? " —" + counts : ""}${seconds}`;
}

export function ScanProgress({ scan, wording, testId = "scan-progress", className }: { scan: ScanState; wording: ScanWording; testId?: string; className?: string }) {
  if (scan.state !== "scanning") return null;
  return <p className={className ?? "mt-2 text-[0.8125rem] tabular-nums text-ink-muted"} data-part="scan-progress" data-testid={testId}>{scanProgressText(scan, wording)}</p>;
}
