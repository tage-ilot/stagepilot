import { linkOf, localLink, noComputerLink, presenterApi, updateOf } from "./fixtures";
import { Presentation } from "./PresentationSample";
import type { SampleState } from "./StateCard";

export const presentationStates: SampleState[] = [
  { id: "ready", label: "Ready", look: "Both pills green; one status line; Details closed; Host, Port, Test connection on top.", render: () => <Presentation link={linkOf("ready")} /> },
  { id: "checking", label: "Searching / checking", look: "Connecting and Checking in grey/blue words; nothing is asked of the person.", render: () => <Presentation api={presenterApi({ connection_status: "connecting" })} link={linkOf("checking", { state: "checking" })} /> },
  { id: "not-connected", label: "Not connected", look: "No ProPresenter computer saved yet: Test connection waits; the Link says Not installed in grey.", render: () => <Presentation api={presenterApi({ enabled: false, connection_status: "disconnected", host: "" })} link={noComputerLink()} /> },
  { id: "host-error", label: "Error on the Host field", look: "The red border and the sentence under the row, which the Host box points at.", render: () => <Presentation error="Enter the computer's name or an address like 192.0.2.10 (no spaces)." link={linkOf("ready")} /> },
  { id: "held", label: "Held (unsaved) change on Host", look: "'Not saved yet' beside the caption, with the warning border: words and border, never colour alone.", render: () => <Presentation held link={linkOf("ready")} /> },
  { id: "not-installed", label: "Link not installed: set-up needed", look: "The Details open by themselves: the optional Link's one command.", render: () => <Presentation link={linkOf("not_installed")} /> },
  { id: "update-by-hand", label: "Update available (needs the command)", look: "The button says Update the Presenter Link and the Details are open.", render: () => <Presentation link={linkOf("outdated", { update: updateOf("manual", "The Presenter Link could not update itself. Run the setup command on the ProPresenter computer to update it.") })} /> },
  { id: "update-by-itself", label: "Update available (StagePilot does it)", look: "Details closed; one calm line inside says StagePilot updates it by itself.", render: () => <Presentation link={linkOf("outdated", { update: updateOf("automatic", "StagePilot updates the Presenter Link by itself when nothing is playing.") })} /> },
  { id: "needs-command", label: "Needs the command again", look: "Warning words in the status line, pill and the open Details.", render: () => <Presentation link={linkOf("needs_command")} /> },
  { id: "not-answering", label: "Not answering", look: "The one state that also shows in the panel header; here: the warning pill and the open Details.", render: () => <Presentation link={linkOf("not_answering")} /> },
  { id: "import-blocked", label: "Import blocked", look: "Limited pill; the sentence with the fix sits in the closed Details (open them).", render: () => <Presentation link={linkOf("ready", { can_import: false, import_blocker: "Allow the Presenter Link in System Settings > Privacy & Security > Accessibility, then try again." })} /> },
  { id: "this-computer", label: "On this computer", look: "'Ready, on this computer'; the Details hold the 'nothing to set up' line.", render: () => <Presentation link={localLink()} /> },
  { id: "no-link", label: "No Presenter Link offered", look: "Only the ProPresenter API line, pill and boxes: no Presenter Link line, pill or Details button.", render: () => <Presentation /> },
];
