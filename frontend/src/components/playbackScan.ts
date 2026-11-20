import type { PlaybackScanResult } from "../types";
import type { ScanState, ScanWording } from "./sections";

/** The Playback scan's words, in the one shape every block's scan uses. */
export const PLAYBACK_SCAN_WORDS: ScanWording = {
  looking: "Looking for Playback…",
  choose: () => "Choose which Playback",
  checklist: ["Playback is open on a computer", "Remote Connections is turned on in Playback", "Both computers are on the same network"],
};

/** The backend's Playback scan, as the one scan shape. */
export function playbackScanState(scan: PlaybackScanResult): ScanState {
  return {
    state: scan.state,
    candidates: scan.candidates.map((candidate) => ({ host: candidate.host, label: candidate.name, hint: candidate.host })),
    current: scan.current,
    total: scan.total,
    phase: scan.phase,
    elapsed: scan.elapsed ?? 0,
    reason: scan.reason,
    error: scan.error_class === "permission_denied" ? "permission_denied" : scan.error_class ? "failed" : null,
    typed: scan.typed,
    networks: scan.networks,
    hostsProbed: scan.hosts_probed,
    hostsTotal: scan.hosts_total,
    details: scan.details,
    settingsUrl: scan.settings_url,
  };
}
