import type { LightkeyDiscovery } from "../types";
import type { ScanState, ScanWording } from "./sections";

/** The search for the Lighting computer, in the one shape every block's scan uses. */
export const LIGHTKEY_SCAN_WORDS: ScanWording = {
  looking: "Looking for a Lighting computer on this network\u2026",
  choose: (count) => `Found ${count} Lighting computers. Choose one:`,
  notFound: "No Lighting computer found on this network.",
  checklist: [],
};

/**
 * The backend's search (`status.discovery`) as the one scan shape. `stopped` is the person pressing "Scanning… (cancel)": the search keeps
 * its own schedule in the background (it is never switched off from here), but this block stops showing it until the next Scan.
 * The states that are about an address that went quiet (`offer`, `connected`) are not a scan result: they stay in the notice.
 */
export function lightkeyScanState(discovery: LightkeyDiscovery | undefined, stopped = false): ScanState {
  const none: ScanState = { state: "idle", candidates: [], current: 0, total: 0 };
  if (!discovery || stopped) return none;
  switch (discovery.state) {
    case "searching":
      return { ...none, state: "scanning" };
    case "choose":
      return {
        ...none,
        state: "found",
        candidates: discovery.found.map((found) => ({ host: found.address, label: found.address, hint: `Lightkey ${found.lightkey_running ? "running" : "not running"}` })),
      };
    case "none":
      return { ...none, state: "not_found" };
    default:
      return none;
  }
}
