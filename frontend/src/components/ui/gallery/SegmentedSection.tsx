import { useState } from "react";

import { SegmentedControl, type SegmentedOption } from "../SegmentedControl";
import type { Tone } from "../types";
import { Section } from "./Section";

const tones: Tone[] = ["services", "presentation", "playback", "lights"];
const main: SegmentedOption = { value: "a", label: "Playback connection", caption: "Main" };
const alt: SegmentedOption = { value: "b", label: "MIDI", caption: "Alternate" };
const two: SegmentedOption[] = [main, alt];
const three: SegmentedOption[] = [...two, { value: "c", label: "Network", caption: "Alternate" }];
const long: SegmentedOption[] = [
  { value: "a", label: "Playback connection (recommended for most venues)", caption: "Main" },
  { value: "b", label: "MIDI over a long named hardware interface", caption: "Alternate" },
];

function Live({ options, tone, initial = "a", ...rest }: { options: SegmentedOption[]; tone: Tone; initial?: string; disabled?: boolean; error?: string; legend?: string }) {
  const [value, setValue] = useState(initial);
  return <SegmentedControl legend={rest.legend ?? "Connection method"} value={value} onChange={setValue} options={options} tone={tone} disabled={rest.disabled} error={rest.error} />;
}

/** Real SegmentedControl. Hover/focus/active are driven by Playwright on the first target. */
export function SegmentedSection() {
  return (
    <Section name="segmented" title="SegmentedControl, Connection method">
      {tones.map((tone, i) => (
        <div key={tone} data-gallery-row={`${tone} 2 and 3 options`} className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <div {...(i === 0 ? { "data-gallery-target": true } : {})}><Live tone={tone} options={two} legend={`Connection method (${tone}, 2)`} /></div>
          <Live tone={tone} options={three} initial="b" legend={`Connection method (${tone}, 3)`} />
        </div>
      ))}
      <div data-gallery-row="disabled, error, disabled option, long labels" className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <Live tone="playback" options={two} disabled legend="Whole group disabled" />
        <Live tone="playback" options={two} error="Choose how this connects." legend="Error" />
        <Live tone="playback" options={[main, { ...alt, disabled: true }]} legend="One option disabled" />
        <Live tone="services" options={long} legend="Long labels" />
      </div>
    </Section>
  );
}
