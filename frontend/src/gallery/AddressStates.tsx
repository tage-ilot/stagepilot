import { BLOCKS } from "./addressBlocks";
import { Box, type Setup } from "./AddressSample";
import type { ScannedAddress } from "../components/sections";
import type { SampleState } from "./StateCard";

const RECENT = ["192.0.2.12", "192.0.2.7", "198.51.100.40"];
const SCANNED: ScannedAddress[] = [{ host: "192.0.2.21", label: "Front MacBook (192.0.2.21)" }, { host: "192.0.2.34", label: "Stage Mac mini (192.0.2.34)" }];
const MANY: ScannedAddress[] = Array.from({ length: 12 }, (_, index) => ({ host: `192.0.2.${60 + index}`, label: `Computer ${index + 1} in the hall (192.0.2.${60 + index})` }));

/** One state of the box, drawn in all three blocks one under the other: the same look in every one. */
const trio = (setup: Setup) => (
  <div className="space-y-3">
    {BLOCKS.map((block) => <Box block={block} key={block.scope} setup={setup} />)}
  </div>
);

export const addressTemplates: SampleState[] = [
  { id: "empty", label: "Address box: empty", look: "A text box with a small arrow inside its right edge. The list is closed; the placeholder shows the shape of an address.", render: () => trio({}) },
  { id: "typed", label: "Address box: a typed address", look: "Typing, pasting and editing always work, with the list closed.", render: () => trio({ value: "192.0.2.99", recent: RECENT }) },
  { id: "picked-recent", label: "Address box: the address is one picked from Recent", look: "The list is open: Recent only, newest first; the address that is in the box has a check (and the words '(in the box now)' for a screen reader).", render: () => trio({ value: "192.0.2.7", recent: RECENT, open: true }) },
  { id: "both", label: "Address box: Scanned and Recent", look: "Two labelled groups. Scanned: the computers the last scan found, 'Name (address)'. Recent: what worked, newest first. An address in both is shown once, under Scanned.", render: () => trio({ value: "192.0.2.21", recent: ["192.0.2.21", ...RECENT], scanned: SCANNED, open: true }) },
  { id: "only-recent", label: "Address box: only Recent", look: "No recent scan: the Scanned group is not drawn at all.", render: () => trio({ recent: RECENT, open: true }) },
  { id: "only-scanned", label: "Address box: only Scanned", look: "A scan found computers and nothing has connected yet: no Recent group.", render: () => trio({ scanned: SCANNED, open: true }) },
  { id: "none", label: "Address box: nothing yet", look: "The arrow is still there. The list says 'No recent or scanned addresses yet.' in a calm line (not an error).", render: () => trio({ open: true }) },
  { id: "keyboard", label: "Address box: the keyboard is in the list", look: "ArrowDown opened the list and moved to the second option: it has a bar around it (not colour alone) and is the box's active option. Enter would choose it; typing would edit the box.", render: () => trio({ recent: RECENT, scanned: SCANNED, keys: ["ArrowDown", "ArrowDown"] }) },
  { id: "long", label: "Address box: a long list scrolls", look: "Twelve scanned computers and three recent: the list is as wide as the box, stops at a set height and scrolls.", render: () => trio({ recent: RECENT, scanned: MANY, open: true }) },
  { id: "error", label: "Address box: error", look: "The red border and the sentence under the row, which the box points at. The arrow still works.", render: () => trio({ value: "192.0.2.10 ", recent: RECENT, error: true }) },
  { id: "held", label: "Address box: held (unsaved) change", look: "'Not saved yet' beside the caption with the warning border: words and border, never colour alone.", render: () => trio({ value: "192.0.2.77", recent: RECENT, held: true }) },
  { id: "disabled", label: "Address box: disabled", look: "While a save or a connect runs the box, its arrow and the buttons wait; the list cannot open.", render: () => trio({ value: "192.0.2.10", recent: RECENT, disabled: true }) },
];
