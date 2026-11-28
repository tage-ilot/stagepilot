import { PlaybackLiveLink } from "../components/PlaybackLiveLink";
import type { HelperState, PlaybackRemote } from "../types";
import { Scripted } from "./Scripted";
import { libraryController, libraryOf, playbackController, playbackDown, playbackOf, scanOf } from "./fixtures";
import type { SampleState } from "./StateCard";

const block = (library: HelperState | null, status = playbackOf(), over: Partial<ReturnType<typeof libraryOf>["helper"]> = {}) => (
  <PlaybackLiveLink library={library ? libraryController(libraryOf(library, over)) : undefined} playback={playbackController(status)} />
);

// Playback refuses the connection while the Library Link is working and says what it is doing about Remote Connections.
const refused = () => ({ ...playbackDown(), last_error: "Playback refused the connection; remote connections may be off" });
const remote = (state: PlaybackRemote["state"], message: string | null = null) => ({ playback_remote: { listening: false, auto: true, state, message, last_attempt_utc: null } });

export const playbackStates: SampleState[] = [
  { id: "ready", label: "Ready", look: "Both pills green; Search and Connect on top, the computer chooser; one status line; Details closed.", render: () => block("ready") },
  { id: "scanning", label: "Searching", look: "The progress line above the row; the button says Searching… (cancel).", render: () => block("ready", playbackOf({ reason: "Looking for Playback…" }, scanOf("scanning", { current: 84, total: 254, phase: "network" }))) },
  { id: "not-connected", label: "Not connected (nothing found)", look: "The checklist under the row: it stays in view, it is part of the connection.", render: () => block("not_installed", playbackDown(scanOf("not_found", { reason: "No Playback answered." }))) },
  { id: "error", label: "Could not connect (error on the address)", look: "The red notice with the reason, the buttons and 'Enter address instead'.", render: () => block("ready", playbackDown(scanOf("not_found", { error_class: "refused", typed: true, reason: "Playback at 10.30.0.99 refused the connection.", hosts_probed: 1, hosts_total: 1 }))) },
  { id: "found-many", label: "Found more than one", look: "The chooser group under the row.", render: () => block("ready", playbackDown(scanOf("found", { candidates: [{ name: "Playback (front)", host: "10.30.0.141" }, { name: "Playback (stage)", host: "10.30.0.152" }] }))) },
  { id: "found-one", label: "Search found one", look: "One Playback answered: StagePilot connected to it by itself; the status line says where.", render: () => block("ready", playbackOf({ reason: "Connected to Playback at 10.30.0.141." }, scanOf("found", { candidates: [{ name: "Playback (front)", host: "10.30.0.141" }] }))) },
  { id: "address-list", label: "The computer arrow open: Found and Recent", look: "ArrowDown in the box (or the arrow) opens the list: the computers the search found, then what worked before. The box is always open to typing; there is no Other computer.", render: () => <div className="pb-64"><Scripted steps={[{ key: ["Playback host", "ArrowDown"] }]}>{block("ready", playbackDown(scanOf("found", { candidates: [{ name: "Playback (front)", host: "10.30.0.141" }, { name: "Playback (stage)", host: "10.30.0.152" }] })))}</Scripted></div> },
  { id: "blocked", label: "macOS blocks the local network", look: "The red notice with the steps, Open Local Network settings and Copy diagnostic details.", render: () => block("ready", playbackDown(scanOf("not_found", { error_class: "permission_denied", reason: "macOS is blocking StagePilot's local-network access.", details: "{\"outcome\":\"blocked\"}", settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork" }))) },
  { id: "connecting", label: "Connecting", look: "Connect says Connecting… (a one-address try); Search waits.", render: () => block("ready", playbackOf({ reason: "Connecting to 10.30.0.141…" }, scanOf("scanning", { typed: true, phase: "Connecting to 10.30.0.141…", current: 0, total: 1 }))) },
  { id: "not-installed", label: "Library Link not installed: set-up needed", look: "The Details open by themselves; nothing is a warning, the line says Not installed in grey.", render: () => block("not_installed") },
  { id: "update-by-hand", label: "Update available (needs the command)", look: "The button says Update the Library Link; the Details are open.", render: () => block("outdated", playbackOf(), { update: { mode: "manual", version: "0.4.0", message: "The Library Link could not update itself. Run the setup command on the Playback computer to update it." } }) },
  { id: "update-by-itself", label: "Update available (StagePilot does it)", look: "Details closed; the line inside says StagePilot updates it.", render: () => block("outdated", playbackOf(), { update: { mode: "automatic", version: "0.4.0", message: "StagePilot updates the Library Link by itself when nothing is playing." } }) },
  { id: "needs-command", label: "Needs the command again", look: "Warning pill; the open Details.", render: () => block("needs_key") },
  { id: "not-answering", label: "Not answering", look: "Warning pill; the open Details.", render: () => block("unreachable") },
  { id: "remote-switching", label: "Library Link switching on Remote Connections", look: "The Playback API line says the Library Link is switching it on and it takes a few seconds; the pills keep their words and tones.", render: () => block("ready", refused(), remote("switching")) },
  { id: "remote-permission", label: "Library Link needs the Mac's permission", look: "The Playback API line says what to allow in System Settings and the by-hand step until then; the pills keep their words and tones.", render: () => block("ready", refused(), remote("needs_permission")) },
  { id: "remote-busy", label: "Playback is being used", look: "The Playback API line says the Library Link will switch Remote Connections on when Playback is quiet; the pills keep their words and tones.", render: () => block("ready", refused(), remote("user_busy")) },
  { id: "no-link", label: "No Library Link offered", look: "Only the Playback API line and pill; no Library Link line, pill or Details button.", render: () => block(null) },
];
