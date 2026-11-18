// Made-up data for the section gallery: nothing here is a real computer, key or song. No backend is asked.
import type { PlaybackController } from "../hooks/usePlaybackInput";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { LinkState, LinkStatus } from "../propresenterLink";
import type { HelperState, HelperUpdateStatus, LibraryStatus, LightkeyStatus, PlaybackScanResult, PlaybackStatusResponse, ProPresenterStatusResponse } from "../types";

const none = () => undefined;
const nobody = async () => true;

// ---- Presentation (ProPresenter) ----------------------------------------------------------------------------------------

export const BUNDLED = "0.1.1";
export const linkOf = (state: LinkState, over: Partial<LinkStatus> = {}): LinkStatus => ({
  mode: "helper", state, message: `The Presenter Link is ${state}.`, version: state === "ready" ? BUNDLED : state === "outdated" ? "0.0.9" : undefined, bundled_version: BUNDLED,
  can_import: state === "ready" || state === "outdated", libraries: state === "ready" ? ["Songs", "Playback"] : [], playback_library_exists: state === "ready",
  computer_address: "192.0.2.10", paired: state !== "not_installed", updater: state === "ready" || state === "outdated", stale: false, ...over,
});
export const localLink = (over: Partial<LinkStatus> = {}): LinkStatus => linkOf("ready", { mode: "local", version: BUNDLED, paired: false, updater: false, computer_address: undefined, ...over });
export const noComputerLink = (): LinkStatus => linkOf("not_installed", { mode: null, can_import: false, paired: false, updater: false });
export const updateOf = (mode: HelperUpdateStatus["mode"], message: string): HelperUpdateStatus => ({ mode, version: BUNDLED, message });
export const linkController = (status: LinkStatus | null): ProPresenterLinkController => ({ status, error: null, pending: null, check: none, watch: none });
export const presenterApi = (over: Partial<ProPresenterStatusResponse> = {}): ProPresenterStatusResponse => ({
  enabled: true, host: "192.0.2.10", port: 1025, timer_name: "Song Countdown", look_id: null, request_timeout_seconds: 3, connection_status: "connected", detail: null,
  timers: [], selected_timer_id: null, timer_found: true, looks: [], current_look_id: null, look_found: true, last_checked_at: null, ...over,
});

// ---- Playback ---------------------------------------------------------------------------------------------------------

const scanOf = (state: PlaybackScanResult["state"], over: Partial<PlaybackScanResult> = {}): PlaybackScanResult => ({ state, candidates: [], reason: null, current: 0, total: 0, ...over });
export const playbackOf = (over: Partial<PlaybackStatusResponse> = {}, scan: PlaybackScanResult = scanOf("idle")): PlaybackStatusResponse => ({
  selected: true, enabled: true, connected: true, active_source: "playback_api", reason: "Connected to Playback at 10.30.0.141.", scan,
  sources: { playback_api: { connected: true, reason: "Playback is connected." }, midi: { connected: false, reason: "MIDI is disconnected." } },
  host: "10.30.0.141", port: 8080, source: "lan", last_error: null, playing: false, setlist_cloud_version: 1, discovery: "done", progress: 0, discovery_song: 0, discovery_total: 0,
  song_lengths: [], lengths_measured_at: null, plan_songs: [], song_order: [], song_count: 0, plan_song_count: 0, captured_version: 1, captured_at: null, stale: false,
  settings: { enabled: true, host: "10.30.0.141", port: 8080, auto_scan: true, fast_transport: true }, ...over,
});
export const playbackDown = (scan: PlaybackScanResult = scanOf("idle")): PlaybackStatusResponse => playbackOf({
  connected: false, active_source: "none", reason: "Disconnected. Choose Scan Network to find Playback.", host: null,
  sources: { playback_api: { connected: false, reason: "Playback is disconnected." }, midi: { connected: false, reason: "MIDI is disconnected." } },
  settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true },
}, scan);
export { scanOf };
export const playbackController = (status: PlaybackStatusResponse | null, over: Partial<PlaybackController> = {}): PlaybackController => ({
  status, events: [], error: null, message: null, pending: null, save: nobody, saveArrangement: nobody, selectSource: nobody, scan: none, cancelScan: none, discover: none, ...over,
});
export const libraryOf = (state: HelperState, over: Partial<LibraryStatus["helper"]> = {}): LibraryStatus => {
  const active = state === "ready" || state === "outdated";
  return {
    enabled: active || state === "unreachable", active,
    helper: { state, version: state === "ready" ? "0.4.0" : state === "outdated" ? "0.3.0" : null, bundled_version: "0.4.0", message: `The Library Link is ${state}.`, paired: state !== "not_installed", updater: active, ...over },
    address: "http://10.30.0.141", state: active ? "ready" : state === "unreachable" ? "problem" : "off", message: active ? "Playback library 0.4.0 is ready: 12 songs in 3 setlists." : `The Library Link is ${state}.`,
    version: active ? "0.4.0" : null, songs: active ? 12 : null, setlists: active ? 3 : null, stale: false, checked_at: null,
  };
};
export const libraryController = (status: LibraryStatus | null): PlaybackLibraryController => ({
  target: "10.30.0.141", address: "", status, plan: null, error: null, pending: null, saveAddress: nobody, chooseSetlist: nobody, check: none, watch: none,
});

// ---- Lighting (Lightkey) ----------------------------------------------------------------------------------------------

export const lightkeyOf = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://192.0.2.10:8770", link: "connected", detail: null, active_count: 11, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null,
  message: null, mode: "auto", auto: { capturing: false, reason: "Auto: replaying (nothing playing)", song: null }, helper: { version: "0.6.0", outdated: false, updater: true },
  project: "Sunday set", fixtures: { link: "live", detail: null, fixtures: 66, lost_packets: 0, recording: false }, holding_fixtures: 12, known_fixtures: 66, ...over,
});
export const lightkeyDown = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => lightkeyOf({
  link: "unreachable", detail: "Lightkey API did not answer.", fixtures: undefined, holding_fixtures: undefined, known_fixtures: undefined, helper: undefined, project: undefined,
  auto: { capturing: false, reason: "Auto: replaying (no connection)", song: null }, ...over,
});
