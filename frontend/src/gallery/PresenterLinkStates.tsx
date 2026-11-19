import { linkController, linkOf, localLink } from "./fixtures";
import {
  JOB_DONE, JOB_DONE_NAMES, JOB_LIBRARY, JOB_LIVE, JOB_RUNNING, NAMES_IN_PROPRESENTER, PREVIEW_NAMES, PREVIEW_PLAYBACK_CLEAN, PREVIEW_PLAYBACK_STOPPED,
} from "./presenterLinkData";
import { Sample, Swap } from "./PresenterLinkSample";
import type { Step } from "./Scripted";
import type { SampleState } from "./StateCard";

const sunday: Step[] = [{ pick: ["Playlist", "PL-SUNDAY"] }, { pick: ["Section", "Worship"] }];
const press: Step = { press: "Add to ProPresenter" };

export const presenterLinkStates: SampleState[] = [
  { id: "no-playlists", label: "No playlists yet", look: "The Playlist box has nothing to choose and the Section box waits; the one button is off and says why (Choose a playlist). The sentence above it says what pressing it does.", render: () => <Sample scenario={{ playlists: [], titles: NAMES_IN_PROPRESENTER }} /> },
  { id: "options", label: "Options chosen", look: "Playlist and Section picked (After \"Worship\"); the names are ticked; the button is on. Nothing has been asked of ProPresenter's playlist yet.", render: () => <Sample steps={sunday} /> },
  { id: "names", label: "Planning Center names only", look: "No Playback data: a plain list of the plan's names with a line saying so, dashes where there is no id, key or arrangement. \"Firm Foundation\" is Not in ProPresenter: it cannot be ticked and says Playback data is needed.", render: () => <Sample scenario={{ titles: NAMES_IN_PROPRESENTER }} steps={sunday} /> },
  { id: "playback", label: "Playback data", look: "Playback's songs in its order, ticked: title, MultiTracks id, key, arrangement; Sections shows each song's section order. Same columns and row heights as the names, so nothing jumps.", render: () => <Sample playback scenario={{ previews: [PREVIEW_PLAYBACK_STOPPED] }} steps={[...sunday, { press: "Sections" }]} /> },
  { id: "playback-stopped", label: "Playback data: one is blocked", look: "One press made the preview and it STOPPED: Firm Foundation cannot be added (its reason is in the preview), Battle Belongs has a guess line, Gratitude is already there. The reason is above the buttons and Skip those and add the rest is offered. Nothing was added.", render: () => <Sample playback scenario={{ previews: [PREVIEW_PLAYBACK_STOPPED] }} steps={[...sunday, press]} /> },
  { id: "swap", label: "The swap from names to Playback data", look: "Untick Battle Belongs, then press the button on top: the list becomes Playback's songs in the same place and Battle Belongs stays unticked (it is in both lists). Press it again: the names come back, still unticked.", render: () => <Swap steps={[...sunday, { toggle: "Battle Belongs" }, { press: "Playback gives" }]} /> },
  { id: "previewing", label: "Making the preview", look: "The button says Making the preview… and is off; the options and ticks are off too; a polite line says the same for a screen reader.", render: () => <Sample playback scenario={{ previews: ["hang"] }} steps={[...sunday, press]} /> },
  { id: "progress", label: "Adding: progress", look: "Under the button, in the section: the steps for the playlist and, per song, what was done. The button is gone while it runs; the options are off.", render: () => <Sample playback scenario={{ previews: [PREVIEW_PLAYBACK_CLEAN], jobs: [JOB_RUNNING] }} steps={[...sunday, press]} /> },
  { id: "result", label: "Result", look: "Every song Added, the counts, the backup's name; Battle Belongs carries the 'Check in ProPresenter:' guess note. A finished job offers no Try again.", render: () => <Sample playback scenario={{ previews: [PREVIEW_PLAYBACK_CLEAN], jobs: [JOB_DONE] }} steps={[...sunday, press]} /> },
  { id: "result-names", label: "Result: Planning Center names", look: "Three songs added as they are (no copy was made).", render: () => <Sample scenario={{ titles: NAMES_IN_PROPRESENTER, previews: [PREVIEW_NAMES], jobs: [JOB_DONE_NAMES] }} steps={[...sunday, press]} /> },
  { id: "live", label: "Refusal: ProPresenter is showing something", look: "The headline says it stopped and nothing was changed, with ProPresenter's own sentence and Try again right under the result.", render: () => <Sample playback scenario={{ previews: [PREVIEW_PLAYBACK_CLEAN], jobs: [JOB_LIVE] }} steps={[...sunday, press]} /> },
  { id: "library", label: "ProPresenter has not loaded the songs", look: "The warning that says to open the Playback library once, then Try again.", render: () => <Sample playback scenario={{ previews: [PREVIEW_PLAYBACK_CLEAN], jobs: [JOB_LIBRARY] }} steps={[...sunday, press]} /> },
  { id: "try-again", label: "Try again", look: "The first try was refused (ProPresenter showing a slide); Try again was pressed, a new preview was made and applied, and the second job is running. Same section, no window.", render: () => <Sample playback scenario={{ previews: [PREVIEW_PLAYBACK_CLEAN], jobs: [JOB_LIVE, JOB_RUNNING] }} steps={[...sunday, press, { press: "Try again" }]} gap={120} /> },
  { id: "this-computer", label: "ProPresenter on this computer", look: "The same section when StagePilot runs on the ProPresenter computer (no helper).", render: () => <Sample link={linkController(localLink())} scenario={{ titles: NAMES_IN_PROPRESENTER }} steps={sunday} /> },
  { id: "not-ready", label: "Presenter Link not ready", look: "Nothing is drawn below this line: the feature is optional, so there is no warning.", render: () => <Sample link={linkController(linkOf("not_installed"))} /> },
];
