import type { EventSummary, PlaybackConnection, PlaybackMonitorEntry, PlaybackStatusResponse } from "../types";

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

/**
 * The backend's sentences, in the buttons' words. Playback says "Choose Scan Network to find Playback" (that sentence is part of the
 * contract the backend is held to), but the button is now Search: the page says the button's name.
 */
export const playbackWords = (text: string): string => text
  .replace("Choose Scan Network to", "Choose Search to")
  // One word for that state (DESIGN.md section 10): the backend's sentences say Disconnected, the page says Not connected.
  .replace(/^Disconnected\./, "Not connected.")
  .replace(/\b(Playback|MIDI) is disconnected\./, "$1 is not connected.");

// Shared wording: connection health is not song-order/action readiness.
export const playbackConnectionDetail = (status: PlaybackConnection) => playbackWords(status.reason);
export const STALE_SETLIST_NOTICE = "Your Playback setlist changed. Scan the setlist again before updating Planning Center.";
export const STALE_PLAN_NOTICE = "Your Services plan changed since the last scan. Scan the setlist again before updating Planning Center.";
export const LEARN_SWITCH_LABEL = "Learn the order as songs play";
export const DOUBTFUL_ORDER_HINT = "Check the order: a song's length differs from the plan. Run a scan.";

// Learning is on unless the saved setting says off. Older backends send no order_mode: they only ever have a scanned order.
const learnsOrder = (status: PlaybackStatusResponse) => status.settings.learn_order !== false;
export const playbackOrderMode = (status: PlaybackStatusResponse): "scanned" | "learned" | "none" =>
  status.order_mode ?? (status.song_order.length > 0 ? "scanned" : "none");

// One sentence for the order, used by the panel (and anything else that names the order).
export function playbackOrderModeText(status: PlaybackStatusResponse): string {
  const mode = playbackOrderMode(status);
  if (mode === "learned") return `Learning the order as songs play (${status.learned_songs?.length ?? 0} of ${status.plan_song_count} placed)`;
  if (mode === "scanned") return status.order_source === "library" ? "Order from the Library Link" : "Order scanned";
  return learnsOrder(status) && status.order_mode !== undefined ? "No order yet - set Up next or scan" : "No order yet - scan to set it up";
}

export const playbackHasDoubtfulSong = (status: PlaybackStatusResponse) => playbackOrderMode(status) === "learned" && Boolean(status.learned_songs?.some((song) => song.doubtful));

// Blocking notice for the dashboard / setup checklist. A learned order counts down, so it is not a blocker and is never called scanned.
export const playbackOrderNotice = (status: PlaybackStatusResponse) => {
  const mode = playbackOrderMode(status);
  if (mode === "learned") return null;
  if (mode === "none" && status.order_mode !== undefined && learnsOrder(status)) return null;
  return status.stale && status.song_order.length > 0 ? STALE_SETLIST_NOTICE
    : status.plan_changed && status.song_order.length > 0 ? STALE_PLAN_NOTICE
    : status.song_order.length === 0 ? "Song order isn't set up yet." : null;
};

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
    : playbackWords(status.sources.midi.reason);

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

/** Playback's song events as rows of the dashboard's Event stream. Only the ones a person can read as news (a song started, paused, ended...); the state snapshots a reconnect sends are not. */
export function playbackStreamRows(events: PlaybackMonitorEntry[] | undefined, status: Pick<PlaybackStatusResponse, "song_order" | "stale"> | null | undefined): EventSummary[] {
  return (events ?? []).flatMap(({ event, discovery, at }, index) => {
    if (!(event.type in EVENT_TEXT) && event.type !== "transport.reverted") return [];
    const stamp = at ? Date.parse(at) : Number.NaN;
    if (Number.isNaN(stamp)) return [];
    const position = event.song_id === null ? -1 : status?.song_order.indexOf(event.song_id) ?? -1;
    const known = !status?.stale && position >= 0 ? position : -1;
    return [{ id: `playback-${at}-${index}`, type: `playback.${event.type}`, timestamp: new Date(stamp).toISOString(), source: `${playbackActivityText(event.type, known)}${discovery ? " (while scanning Playback songs)" : ""}` }];
  });
}


export const SONG_ORDER_OPEN_KEY = "stagepilot.playback.song-order-open";
/** The ONE song-scan button of the Song order section names the best scan there is: the Library Link (read, nothing is stepped) or the Playback API (stepping through the songs). */
export const SCAN_PLAYBACK_LIBRARY = "Scan Playback Library";
export const SCAN_PLAYBACK_API = "Scan Playback API";
/** Why the button is greyed out (its title, and the words a screen reader hears with it). */
export const SCAN_NEEDS_API = "Connect to the Playback API first.";
export const SCAN_NEEDS_STOPPED = "Stop Playback to scan songs.";
export const SCAN_IS_RUNNING = "A scan is running.";

export interface SongScanButton {
  /** `library`: reads the Library Link (no confirmation, works while a song plays). `api`: the confirmation, then stepping through Playback. */
  kind: "library" | "api";
  label: typeof SCAN_PLAYBACK_LIBRARY | typeof SCAN_PLAYBACK_API;
  /** Why it cannot be pressed now; null when it can. The button is never hidden. */
  reason: string | null;
}

/**
 * The best song scan available. The Library Link when it is usable (answering, not an old copy) and its order has no failed condition;
 * otherwise the Playback API (also when the Library Link is usable but its order failed: then its plain line says why). `reading`: a
 * Library Link read is in flight (that button shows its own loading).
 */
export function songScanButton({ libraryUsable, libraryNote, apiConnected, playing, running, reading }: {
  libraryUsable: boolean; libraryNote: string | null | undefined; apiConnected: boolean; playing: boolean; running: boolean; reading: boolean;
}): SongScanButton {
  const library = libraryUsable && !libraryNote;
  const kind = library ? "library" : "api";
  const reason = !apiConnected ? SCAN_NEEDS_API
    : running || (!library && reading) ? SCAN_IS_RUNNING
      : !library && playing ? SCAN_NEEDS_STOPPED
        : null;
  return { kind, label: library ? SCAN_PLAYBACK_LIBRARY : SCAN_PLAYBACK_API, reason };
}
export const readFromLibraryText = (songs: number): string => `Read ${songs} ${songs === 1 ? "song" : "songs"} from the Library Link`;

const orderIsSaved = (status: PlaybackStatusResponse) => Boolean(status.song_order.length) && !status.stale && !status.plan_changed;

/**
 * The neutral chip in the Song order header while StagePilot learns the order as songs play (`Learning order (0 of 4)`), or null. Learning
 * works as designed, so it is a state, not a warning; a saved scan wins over it.
 */
export function learningOrderChip(status: PlaybackStatusResponse): string | null {
  if (orderIsSaved(status) || playbackOrderMode(status) !== "learned") return null;
  return `Learning order (${status.learned_songs?.length ?? 0} of ${status.plan_song_count})`;
}

/** One line for the Song order header: THE fact about the order, so the open section says nothing twice and a folded one still says something. */
export function songOrderSummary(status: PlaybackStatusResponse, running: boolean): string {
  if (running) return "Scanning Playback songs…";
  const songs = `${status.song_order.length} ${status.song_order.length === 1 ? "song" : "songs"}`;
  // Backends that say where the order came from get the words for it; older ones keep the old summary.
  if (orderIsSaved(status) && status.order_source === "library") return `Order from the Library Link (${songs})`;
  if (orderIsSaved(status) && status.order_source === "scan") return `Order from a scan (${songs})`;
  if (orderIsSaved(status)) return `Order saved (${songs})`;
  return status.song_order.length > 0 ? "Needs a new scan" : "No order yet";
}
