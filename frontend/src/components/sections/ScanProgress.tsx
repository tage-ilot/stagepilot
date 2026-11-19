import type { ScanState } from "./scanState";

export interface ScanWording {
  /** The progress line of a network search ("Searching the network for Playback…"); what the search is doing now follows it. */
  looking: string;
  /** The chooser's title for `count` computers ("Found 2 Playback computers. Choose one:"). Also its accessible name. */
  choose: (count: number) => string;
  /** What the help says when nothing answered. Left out: the scan's own reason. */
  notFound?: string;
  /** The checklist under "Make sure:". Empty: no checklist. */
  checklist: string[];
  /** The name of what the person types instead ("Enter address instead"). */
  other?: string;
}

/** A backend's own "Looking for X…" phase says what `looking` says already. */
const SAYS_LOOKING = /^Looking for /;

/**
 * The ONE progress line of a search, in every block: what it is doing, how far, how long (`Searching the network for Playback… This
 * computer — Checked 64 of 254 addresses (3s)`). A one-address try (Connect) says its own phase (`Connecting to 10.0.0.5…`). Drawn only while it runs.
 */
function scanProgressText(scan: ScanState, wording: ScanWording): string {
  const detail = scan.phase && !SAYS_LOOKING.test(scan.phase) ? scan.phase : null;
  const phase = scan.typed ? scan.phase ?? "Connecting…" : detail ? `${wording.looking} ${detail}` : wording.looking;
  const counts = scan.total > 1 ? ` Checked ${scan.current} of ${scan.total} addresses` : "";
  // A search that does not time itself (Lighting's) says no seconds.
  const seconds = scan.elapsed === undefined ? "" : ` (${Math.max(0, Math.floor(scan.elapsed))}s)`;
  return `${phase}${counts ? " —" + counts : ""}${seconds}`;
}

export function ScanProgress({ scan, wording, testId = "scan-progress", className }: { scan: ScanState; wording: ScanWording; testId?: string; className?: string }) {
  if (scan.state !== "scanning") return null;
  return <p className={className ?? "mt-2 text-[0.8125rem] tabular-nums text-ink-muted"} data-part="scan-progress" data-testid={testId}>{scanProgressText(scan, wording)}</p>;
}
