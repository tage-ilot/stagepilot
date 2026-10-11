import { SaveBar, type SaveBarState } from "../SaveBar";
import { Section } from "./Section";

const rows: { label: string; state: SaveBarState }[] = [
  { label: "1. Nothing changed", state: { kind: "clean" } },
  { label: "2. Saved automatically", state: { kind: "auto", at: "9:41 AM" } },
  { label: "3. Saving", state: { kind: "saving" } },
  { label: "4. Waiting (held changes)", state: { kind: "held", count: 2 } },
  { label: "4b. Waiting, one change", state: { kind: "held", count: 1 } },
  { label: "5. Couldn't save", state: { kind: "error", message: "Couldn't save the service plan." } },
];
const longMessage =
  "Couldn't save: the connection to the planning service timed out after several attempts, so nothing was changed. Check the network and try again.";

/** Real SaveBar. Sticky is off here so each state is visible in the page flow. */
export function SaveBarSection() {
  return (
    <Section name="savebar" title="SaveBar, five states">
      {rows.map(({ label, state }, i) => (
        <div key={label} data-gallery-row={label} className="space-y-2">
          <h3 className="font-semibold text-ink-body">{label}</h3>
          <div {...(i === 0 ? { "data-gallery-target": true } : {})}>
            <SaveBar state={state} stickyOnPhone={false} />
          </div>
        </div>
      ))}
      <div data-gallery-row="Long error message" className="space-y-2">
        <h3 className="font-semibold text-ink-body">Long error message</h3>
        <SaveBar state={{ kind: "error", message: longMessage }} stickyOnPhone={false} />
      </div>
      <div data-gallery-row="Phone sticky" className="space-y-2">
        <h3 className="font-semibold text-ink-body">Sticky at the bottom below lg (scroll box stands in for the screen)</h3>
        <div data-gallery-sticky className="h-56 overflow-y-auto rounded-ds-lg bg-surface-950 p-3">
          <p className="h-40 text-ink-muted">Panel content scrolls under the bar…</p>
          <SaveBar state={{ kind: "held", count: 2 }} />
        </div>
      </div>
    </Section>
  );
}
