import type { ProPresenterScan } from "../propresenterScan";
import { searchWords } from "./playbackScan";
import type { ScanState, ScanWording } from "./sections";

/** The ProPresenter scan's words, in the one shape every block's scan uses. */
export const PROPRESENTER_SCAN_WORDS: ScanWording = {
  looking: "Searching the network for ProPresenter\u2026",
  choose: (count) => `Found ${count} ProPresenter computers. Choose one:`,
  checklist: ["ProPresenter is open on a computer", "The network API is turned on in ProPresenter (Settings > Network)", "Both computers are on the same network"],
};

/** The backend's ProPresenter scan, as the one scan shape. */
export function proPresenterScanState(scan: ProPresenterScan | null): ScanState {
  if (!scan) return { state: "idle", candidates: [], current: 0, total: 0 };
  return {
    state: scan.state,
    candidates: scan.candidates.map((candidate) => ({
      host: candidate.host,
      port: candidate.port,
      label: candidate.name,
      hint: `${candidate.host}:${candidate.port} · ${candidate.version}`,
    })),
    current: scan.current,
    total: scan.total,
    phase: scan.phase,
    elapsed: scan.elapsed,
    reason: searchWords(scan.reason),
    error: scan.error_class === "permission_denied" ? "permission_denied" : scan.error_class ? "failed" : null,
    networks: scan.networks,
    hostsProbed: scan.hosts_probed,
    hostsTotal: scan.hosts_total,
    details: scan.details,
    settingsUrl: scan.settings_url,
  };
}
