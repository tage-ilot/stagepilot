/**
 * What a scan looks like to the page, whichever block ran it: ONE shape for the Playback scan, the Lighting search and the ProPresenter
 * scan. Each block adapts its own backend's answer into this (a thin function beside its controller); the templates (`ScanProgress`,
 * `ScanChooser`, `ScanHelp`) read only this.
 */
export interface ScanCandidate {
  /** The address the computer answered at. What choosing it puts in the Host box. */
  host: string;
  /** The name to read in the chooser. */
  label: string;
  /** Muted words after the name (the address, the version, "Lightkey running"). */
  hint?: string;
  /** The API port it answered on (ProPresenter), when the answer says it. Choosing it also fills the API box. */
  port?: number;
}

export type ScanPhase = "idle" | "scanning" | "found" | "not_found";

/** `permission_denied`: macOS blocks the local network (the template shows the steps). `failed`: something else went wrong (the reason says what). */
export type ScanErrorKind = "permission_denied" | "failed";

export interface ScanState {
  state: ScanPhase;
  candidates: ScanCandidate[];
  /** Addresses looked at so far / to look at (0 when the scan cannot count). */
  current: number;
  total: number;
  /** What the scan is doing now ("This computer", "Connecting to 10.0.0.5…"). */
  phase?: string | null;
  /** Seconds since it started. */
  elapsed?: number;
  /** One plain sentence for what the last scan ended with, when it found nothing. */
  reason?: string | null;
  error?: ScanErrorKind | null;
  /** One address was tried, not a network (Connect to a typed or chosen computer). */
  typed?: boolean;
  networks?: string[];
  hostsProbed?: number;
  hostsTotal?: number;
  /** Diagnostic text the person may copy. Holds no secret. */
  details?: string | null;
  /** The macOS page to open when the local network is blocked. */
  settingsUrl?: string | null;
}

export const NO_SCAN: ScanState = { state: "idle", candidates: [], current: 0, total: 0 };

const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;

/** "Checked 120 of 254 addresses on 1 network (192.168.4.0/24) in 12.3s." */
export function scanSummary(scan: ScanState): string {
  const seconds = (scan.elapsed ?? 0).toFixed(1);
  if (scan.typed) return `Tried one address in ${seconds}s.`;
  const nets = scan.networks?.length ? `${plural(scan.networks.length, "network")} (${scan.networks.join(", ")})` : "no network";
  return `Checked ${scan.hostsProbed ?? 0} of ${scan.hostsTotal ?? 0} addresses on ${nets} in ${seconds}s.`;
}
