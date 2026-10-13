import type { PlaybackConnection, PlaybackScanResult, PlaybackStatusResponse } from "../types";

// The backend accepts a bare host, not a URL. Keep port at the existing configured value.
export function validPlaybackHost(host: string): boolean {
  if (!host) return true;
  if (host.length > 255 || /[\s/?#]/.test(host)) return false;
  if (host.includes(":")) {
    try { new URL(`http://[${host}]/`); return true; }
    catch { return false; }
  }
  if (/^[\d.]+$/.test(host)) {
    const parts = host.split(".");
    return parts.length === 4 && parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255);
  }
  return host.split(".").every((part) => /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/i.test(part) && part.length <= 63);
}

// Shared wording: connection health is not song-order/action readiness.
export const playbackConnectionDetail = (status: PlaybackConnection) => status.reason;
export const STALE_SETLIST_NOTICE = "Your Playback setlist changed. Scan the setlist again before updating Planning Center.";
export const STALE_PLAN_NOTICE = "Your Services plan changed since the last scan. Scan the setlist again before updating Planning Center.";
export const playbackOrderNotice = (status: PlaybackStatusResponse) =>
  status.stale && status.song_order.length > 0 ? STALE_SETLIST_NOTICE
    : status.plan_changed && status.song_order.length > 0 ? STALE_PLAN_NOTICE
    : status.song_order.length === 0 ? "Song order isn't set up yet." : null;

export function playbackSongCountMismatch(status: PlaybackStatusResponse): string | null {
  const playback = status.song_count;
  const plan = status.plan_song_count;
  if (!Number.isInteger(playback) || !Number.isInteger(plan) || playback < 0 || plan < 0 || playback === plan) return null;
  const extra = Math.abs(playback - plan);
  const first = Math.min(playback, plan) + 1;
  const range = extra === 1 ? `${first}` : `${first}-${Math.max(playback, plan)}`;
  const counts = `Playback has ${playback} song${playback === 1 ? "" : "s"}, this service plan has ${plan}.`;
  return playback > plan
    ? `${counts} Playback song${extra === 1 ? "" : "s"} ${range} ${extra === 1 ? "is" : "are"} ignored.`
    : `${counts} Service plan song${extra === 1 ? "" : "s"} ${range} ${extra === 1 ? "has" : "have"} no Playback song.`;
}

export type BannerTone = "connected" | "searching" | "midi" | "idle";
// The sentence always comes from the backend `reason`; only the colour is chosen here.
export const playbackBannerTone = (status: PlaybackConnection & { scan?: { state: string } }): BannerTone =>
  status.scan?.state === "scanning" ? "searching"
    : status.active_source === "playback_api" ? "connected"
      : status.active_source === "midi" ? "midi" : "idle";

export const playbackMidiNote = (status: PlaybackConnection) =>
  status.sources.midi.connected
    ? status.active_source === "playback_api" ? "MIDI is connected and standing by." : "MIDI is connected."
    : status.sources.midi.reason;

const EVENT_TEXT: Record<string, string> = {
  "song.started": "started", "song.resumed": "resumed", "song.paused": "Paused", "song.stopped": "Stopped",
  "song.ended": "ended", "song.selected": "selected", "song.changed": "changed",
};
export function playbackActivityText(type: string, position: number): string {
  if (type === "transport.reverted") return "Playback did not confirm the command; transport reverted.";
  const verb = EVENT_TEXT[type] ?? type;
  const generic = verb === "Paused" || verb === "Stopped";
  if (generic) return verb;
  return position < 0 ? `A song ${verb}` : `Song ${position + 1} ${verb}`;
}

const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;

export function scanProgressText(scan: PlaybackScanResult): string {
  const seconds = Math.max(0, Math.floor(scan.elapsed ?? 0));
  const phase = scan.phase ?? (scan.typed ? "Connecting…" : "Looking for Playback…");
  const counts = scan.total > 1 ? ` Checked ${scan.current} of ${scan.total} addresses` : "";
  return `${phase}${counts ? " —" + counts : ""} (${seconds}s)`;
}

export function scanSummary(scan: PlaybackScanResult): string {
  const seconds = (scan.elapsed ?? 0).toFixed(1);
  if (scan.typed) return `Tried one address in ${seconds}s.`;
  const nets = scan.networks?.length ? `${plural(scan.networks.length, "network")} (${scan.networks.join(", ")})` : "no network";
  return `Checked ${scan.hosts_probed ?? 0} of ${scan.hosts_total ?? 0} addresses on ${nets} in ${seconds}s.`;
}
