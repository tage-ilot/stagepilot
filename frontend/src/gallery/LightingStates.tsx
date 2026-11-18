import { lightkeyDown, lightkeyOf } from "./fixtures";
import { Lighting } from "./LightingSample";
import type { SampleState } from "./StateCard";

export const lightingStates: SampleState[] = [
  { id: "ready", label: "Ready", look: "Connected · 66 fixtures; Host, OSC UDP, API, Connect, Mode on top; the state line; Details closed.", render: () => <Lighting status={lightkeyOf()} /> },
  { id: "searching", label: "Searching / checking", look: "Looking for the Lighting computer by itself: the state line says so.", render: () => <Lighting status={lightkeyDown({ discovery: { state: "searching", found: [], refusing: [], message: "Looking for a Lighting computer on this network…" } })} /> },
  { id: "not-connected", label: "Not connected: helper set-up needed", look: "Red pill, the reason once, and the Details open by themselves with the one command.", render: () => <Lighting status={lightkeyDown()} /> },
  { id: "host-error", label: "Error on the Host field", look: "After a save was tried: the red border and the sentence under the row.", render: () => <Lighting attempted host="not a host!" status={lightkeyDown()} /> },
  { id: "held", label: "Held (unsaved) change on Host", look: "The warning border on Host and OSC UDP (saved only by the panel's Save bar).", render: () => <Lighting held host="192.0.2.77" status={lightkeyOf()} /> },
  { id: "closed", label: "Lightkey is closed", look: "Amber pill: the Lighting computer answers, Lightkey is not open.", render: () => <Lighting status={lightkeyOf({ link: "lightkey_closed", detail: "Lightkey is not running.", fixtures: undefined })} /> },
  { id: "update-by-hand", label: "Update available (needs the command)", look: "The helper is behind: the Details open with Update the helper and the amber note above them.", render: () => <Lighting status={lightkeyOf({ helper: { version: "0.4.0", outdated: true } })} /> },
  { id: "update-by-itself", label: "Update available (StagePilot does it)", look: "Details closed; the quiet line inside says what StagePilot does.", render: () => <Lighting status={lightkeyOf({ helper: { version: "0.6.0", outdated: false, updater: true, update: { mode: "automatic", version: "0.7.0", message: "A newer version is ready." } } })} /> },
  { id: "replay-off", label: "Replay is off", look: "The warning that saved lights will not play, with its button, stays in view.", render: () => <Lighting status={lightkeyOf({ replay: false })} /> },
  { id: "off", label: "Off", look: "Lighting is off: grey pill and no Details button (nothing to set up yet).", render: () => <Lighting status={lightkeyDown({ link: "off", detail: "Lighting is off." })} /> },
];
