import { Disclosure, type DisclosureTone } from "../Disclosure";
import { Section } from "./Section";

const tones: DisclosureTone[] = ["services", "presentation", "playback", "lights"];
const long = "Endpoint 192.0.2.10 · Port 8080 · Channel 1 · Auto reconnect on · Timeout 30 seconds · Retry every 5 seconds";

/** Real Disclosure. Closed and open rendered via defaultOpen; interaction states are driven by Playwright on the real target. */
export function DisclosureSection() {
  return (
    <Section name="disclosure" title="Disclosure, Advanced and Activity">
      {tones.map((tone) => (
        <div key={tone} data-gallery-row={`advanced ${tone}`} className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <Disclosure variant="advanced" tone={tone} title="Advanced" summary="Port 8080 · Channel 1" data-gallery-target>
            <p>Extra options for {tone}.</p>
          </Disclosure>
          <Disclosure variant="advanced" tone={tone} title="Advanced" summary="Port 8080 · Channel 1" defaultOpen>
            <p>Extra options for {tone}.</p>
          </Disclosure>
        </div>
      ))}
      <div data-gallery-row="activity" className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <Disclosure variant="activity" title="Recent activity" count={12}><p>Read-only event list.</p></Disclosure>
        <Disclosure variant="activity" title="Recent activity" count={12} defaultOpen><p>Read-only event list.</p></Disclosure>
      </div>
      <div data-gallery-row="without summary and long summary" className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <Disclosure variant="advanced" tone="services" title="Advanced"><p>No summary.</p></Disclosure>
        <Disclosure variant="advanced" tone="playback" title="Advanced" summary={long}><p>Long summary truncates.</p></Disclosure>
      </div>
    </Section>
  );
}
