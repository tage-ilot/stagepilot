import { PlaybackLiveLink } from "../components/PlaybackLiveLink";
import type { HelperState } from "../types";
import { libraryController, libraryOf, playbackController, playbackDown, playbackOf, scanOf } from "./fixtures";
import type { SampleState } from "./StateCard";

const block = (library: HelperState | null, status = playbackOf(), over: Partial<ReturnType<typeof libraryOf>["helper"]> = {}) => (
  <PlaybackLiveLink library={library ? libraryController(libraryOf(library, over)) : undefined} playback={playbackController(status)} />
);

export const playbackStates: SampleState[] = [
  { id: "ready", label: "Ready", look: "Both pills green; Scan Network and the chooser on top; one status line; Details closed.", render: () => block("ready") },
  { id: "scanning", label: "Scanning", look: "The progress line above the row; the button says Scanning… (cancel).", render: () => block("ready", playbackOf({ reason: "Looking for Playback…" }, scanOf("scanning", { current: 84, total: 254, phase: "network" }))) },
  { id: "not-connected", label: "Not connected (nothing found)", look: "The checklist under the row: it stays in view, it is part of the connection.", render: () => block("not_installed", playbackDown(scanOf("not_found", { reason: "No Playback answered." }))) },
  { id: "error", label: "Could not connect (error on the address)", look: "The red notice with the reason, the buttons and 'Enter address instead'.", render: () => block("ready", playbackDown(scanOf("not_found", { error_class: "refused", typed: true, reason: "Playback at 10.30.0.99 refused the connection.", hosts_probed: 1, hosts_total: 1 }))) },
  { id: "found-many", label: "Found more than one", look: "The chooser group under the row.", render: () => block("ready", playbackDown(scanOf("found", { candidates: [{ name: "Playback (front)", host: "10.30.0.141" }, { name: "Playback (stage)", host: "10.30.0.152" }] }))) },
  { id: "not-installed", label: "Library Link not installed: set-up needed", look: "The Details open by themselves; nothing is a warning, the line says Not installed in grey.", render: () => block("not_installed") },
  { id: "update-by-hand", label: "Update available (needs the command)", look: "The button says Update the Library Link; the Details are open.", render: () => block("outdated", playbackOf(), { update: { mode: "manual", version: "0.4.0", message: "The Library Link could not update itself. Run the setup command on the Playback computer to update it." } }) },
  { id: "update-by-itself", label: "Update available (StagePilot does it)", look: "Details closed; the line inside says StagePilot updates it.", render: () => block("outdated", playbackOf(), { update: { mode: "automatic", version: "0.4.0", message: "StagePilot updates the Library Link by itself when nothing is playing." } }) },
  { id: "needs-command", label: "Needs the command again", look: "Warning pill; the open Details.", render: () => block("needs_key") },
  { id: "not-answering", label: "Not answering", look: "Warning pill; the open Details.", render: () => block("unreachable") },
  { id: "no-link", label: "No Library Link offered", look: "Only the Playback API line and pill; no Library Link line, pill or Details button.", render: () => block(null) },
];
