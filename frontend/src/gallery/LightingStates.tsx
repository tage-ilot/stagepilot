import { HOST_LABEL } from "../components/lightingHost";
import { lightkeyDown, lightkeyOf } from "./fixtures";
import { Lighting } from "./LightingSample";
import { Scripted } from "./Scripted";
import type { SampleState } from "./StateCard";

export const lightingStates: SampleState[] = [
  { id: "ready", label: "Ready", look: "Connected · 66 fixtures; Host, OSC UDP, API, Connect, Mode on top; the state line; Details closed.", render: () => <Lighting status={lightkeyOf()} /> },
  { id: "searching", label: "Scanning", look: "Looking for the Lighting computer: the progress line under the row, and the Scan button says Scanning… (cancel).", render: () => <Lighting status={lightkeyDown({ discovery: { state: "searching", found: [], refusing: [], message: "Looking for a Lighting computer on this network…" } })} /> },
  { id: "found-one", label: "Scan found one", look: "One Lighting computer answered: StagePilot connected to it by itself and says so under the row.", render: () => <Lighting status={lightkeyOf({ discovery: { state: "connected", found: [], refusing: [], message: "Found a Lighting computer at 192.0.2.10 and connected." } })} /> },
  { id: "found-many", label: "Scan found more than one", look: "The same chooser Playback uses, under the row: choosing one puts it in Host; Connect connects.", render: () => <Lighting status={lightkeyDown({ discovery: { state: "choose", found: [{ address: "192.0.2.10", lightkey_running: true }, { address: "192.0.2.34", lightkey_running: false }], refusing: [] } })} /> },
  { id: "address-list", label: "The Host arrow open: Scanned and Recent", look: "ArrowDown in Host (or the arrow) opens the list: the computers the search found, then what worked before. Choosing one fills Host; Connect connects.", render: () => <div className="pb-64"><Scripted patient steps={[{ key: [HOST_LABEL, "ArrowDown"] }]}><Lighting status={lightkeyDown({ discovery: { state: "choose", found: [{ address: "192.0.2.10", lightkey_running: true }, { address: "192.0.2.34", lightkey_running: false }], refusing: [] } })} /></Scripted></div> },
  { id: "not-found", label: "Scan found nothing", look: "One quiet line under the row. Scan and Connect are still both there.", render: () => <Lighting status={lightkeyDown({ discovery: { state: "none", found: [], refusing: [] } })} /> },
  { id: "not-connected", label: "Not connected: helper set-up needed", look: "Red pill, the reason once, and the Details open by themselves with the one command.", render: () => <Lighting status={lightkeyDown()} /> },
  { id: "host-error", label: "Error on the Host field", look: "After a save was tried: the red border and the sentence under the row.", render: () => <Lighting attempted host="not a host!" status={lightkeyDown()} /> },
  { id: "held", label: "Held (unsaved) change on Host", look: "The warning border on Host and OSC UDP (saved only by the panel's Save bar).", render: () => <Lighting held host="192.0.2.77" status={lightkeyOf()} /> },
  { id: "closed", label: "Lightkey is closed", look: "Amber pill: the Lighting computer answers, Lightkey is not open.", render: () => <Lighting status={lightkeyOf({ link: "lightkey_closed", detail: "Lightkey is not running.", fixtures: undefined })} /> },
  { id: "update-by-hand", label: "Update available (needs the command)", look: "The helper is behind: the Details open with Update the helper and the amber note above them.", render: () => <Lighting status={lightkeyOf({ helper: { version: "0.4.0", outdated: true } })} /> },
  { id: "update-by-itself", label: "Update available (StagePilot does it)", look: "Details closed; the quiet line inside says what StagePilot does.", render: () => <Lighting status={lightkeyOf({ helper: { version: "0.6.0", outdated: false, updater: true, update: { mode: "automatic", version: "0.7.0", message: "A newer version is ready." } } })} /> },
  { id: "replay-off", label: "Replay is off", look: "The warning that saved lights will not play, with its button, stays in view.", render: () => <Lighting status={lightkeyOf({ replay: false })} /> },
  { id: "off", label: "Off", look: "Lighting is off: grey pill and no Details button (nothing to set up yet).", render: () => <Lighting status={lightkeyDown({ link: "off", detail: "Lighting is off." })} /> },
];
