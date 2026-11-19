import type { RecentScope } from "../hooks/useRecentAddresses";

/** Made-up addresses that "worked before", so a real block's arrow has a Recent group to show. Only where nothing is remembered yet. */
const MADE_UP: Record<RecentScope, string[]> = {
  playback: ["10.30.0.141", "10.30.0.7"],
  lighting: ["192.0.2.10", "192.0.2.5"],
  presentation: ["192.0.2.10", "192.0.2.40"],
};

export function seedRecentAddresses(): void {
  try {
    for (const [scope, addresses] of Object.entries(MADE_UP)) {
      const key = `stagepilot.recent-addresses.${scope}`;
      if (window.localStorage.getItem(key) === null) window.localStorage.setItem(key, JSON.stringify(addresses));
    }
  } catch { /* storage unavailable: the lists simply say nothing is remembered yet */ }
}
