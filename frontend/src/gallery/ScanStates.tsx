import { LIGHTKEY_SCAN_WORDS } from "../components/lightkeyScan";
import { PLAYBACK_SCAN_WORDS } from "../components/playbackScan";
import { PROPRESENTER_SCAN_WORDS } from "../components/proPresenterScan";
import { ScanArea, ScanChooser, ScanHelp, ScanProgress, type ScanState, type ScanWording } from "../components/sections";
import type { Tone } from "../components/ui/types";
import { Panel, type PanelIdentity } from "../components/ui/Panel";
import { FRONT_MAC, STAGE_MAC, presenterScan } from "./fixtures";
import type { SampleState } from "./StateCard";

const noop = () => undefined;
const URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";

const TONES: { tone: Tone & PanelIdentity; name: string; words: ScanWording; many: ScanState["candidates"] }[] = [
  { tone: "playback", name: "Playback", words: PLAYBACK_SCAN_WORDS, many: [{ host: "10.30.0.141", label: "Playback (front)", hint: "10.30.0.141" }, { host: "10.30.0.152", label: "Playback (stage)", hint: "10.30.0.152" }] },
  { tone: "lighting", name: "Lighting", words: LIGHTKEY_SCAN_WORDS, many: [{ host: "192.0.2.10", label: "192.0.2.10", hint: "Lightkey running" }, { host: "192.0.2.34", label: "192.0.2.34", hint: "Lightkey not running" }] },
  { tone: "presentation", name: "Presentation", words: PROPRESENTER_SCAN_WORDS, many: [FRONT_MAC, STAGE_MAC] },
];

/** The three scan templates by themselves, in each connection's colour: the same pieces every Live Link block draws under its row. */
const piece = (tone: PanelIdentity & Tone, words: ScanWording, scan: ScanState) => (
  <Panel tone={tone} variant="setup">
    <ScanProgress scan={scan} wording={words} />
    <ScanArea>
      <ScanHelp onCopy={noop} onOther={noop} scan={scan} tone={tone} wording={words} />
      <ScanChooser onChoose={noop} scan={scan} tone={tone} wording={words} />
    </ScanArea>
  </Panel>
);

export const scanTemplates: SampleState[] = TONES.flatMap(({ tone, name, words, many }): SampleState[] => [
  { id: `${tone}-scanning`, label: `${name}: searching`, look: "ScanProgress: what it is doing, how far, how long. Nothing else is drawn while it searches.", render: () => piece(tone, words, presenterScan("scanning", { current: 84, total: 255, phase: words.looking, elapsed: 6 })) },
  { id: `${tone}-found-many`, label: `${name}: found several`, look: "ScanChooser: one button per computer, the name and its address. One computer needs no chooser.", render: () => piece(tone, words, presenterScan("found", { candidates: many })) },
  { id: `${tone}-not-found`, label: `${name}: found nothing`, look: "ScanHelp: the plain words and the checklist (a quiet line where there is no checklist).", render: () => piece(tone, words, presenterScan("not_found", { reason: "Nobody answered.", hostsProbed: 255, hostsTotal: 255, networks: ["192.0.2.0/24"], elapsed: 18.4 })) },
  { id: `${tone}-blocked`, label: `${name}: macOS blocks the local network`, look: "ScanHelp: the red notice with the steps, the settings link and the diagnostic details. The Playback and ProPresenter scans report this; the Lighting search does not yet, so it is shown here as the template only.", render: () => piece(tone, words, presenterScan("not_found", { error: "permission_denied", reason: "macOS is blocking StagePilot's local-network access.", details: "{\"outcome\":\"blocked\"}", settingsUrl: URL })) },
]);
