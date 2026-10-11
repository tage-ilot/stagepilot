import { useState } from "react";

import { Button } from "../Button";
import { Dialog, type DialogLayer } from "../Dialog";
import { Section } from "./Section";

const longTitle = "Update Planning Center song times for the next upcoming service plan, including every skipped item?";
const longBody = Array.from({ length: 14 }, (_, i) => `Song ${i + 1} · 3:${String(10 + i).padStart(2, "0")} → 4:${String(20 + i).padStart(2, "0")}`);

function Frame({ children }: { children: React.ReactNode }) {
  // A transformed box makes `fixed` descendants position inside it, so each dialog is visible in page flow.
  return <div className="relative h-[28rem] overflow-hidden rounded-ds-lg border border-edge-subtle bg-surface-900 [transform:translateZ(0)]">{children}</div>;
}

function Example({ label, layer, danger, title, body, dismissible = true }: { label: string; layer?: DialogLayer; danger?: boolean; title: string; body?: React.ReactNode; dismissible?: boolean }) {
  return (
    <div data-gallery-row={label} className="space-y-2">
      <h3 className="font-semibold text-ink-body">{label}</h3>
      <Frame>
        <Dialog
          open
          layer={layer}
          tone={danger ? "danger" : "default"}
          dismissible={dismissible}
          title={title}
          description={body ? undefined : "Nothing will be played."}
          actions={
            <>
              <Button size="touch" variant="secondary">Cancel</Button>
              <Button size="touch" variant={danger ? "destructive" : "primary"}>{danger ? "Discard" : "Confirm"}</Button>
            </>
          }
        >
          {body}
        </Dialog>
      </Frame>
    </div>
  );
}

/** Real Dialog. Open dialogs sit inside transformed frames so several show at once. */
export function DialogSection() {
  const [live, setLive] = useState(false);
  return (
    <Section name="dialog" title="Dialog">
      <Example label="Default action" title="Set up song order?" />
      <Example label="Danger action" danger title="Discard unsaved changes?" />
      <Example label="Long title" title={longTitle} />
      <Example
        label="Long body scrolls"
        title="Update Planning Center?"
        body={<ul className="divide-y divide-edge-subtle">{longBody.map((l) => <li key={l} className="py-2 tabular-nums">{l}</li>)}</ul>}
      />
      <Example label="Layer: confirm (z 50)" layer="confirm" title="Confirm layer" />
      <Example label="Layer: update (z 100)" layer="update" title="Update layer" dismissible={false} />
      <Example label="Layer: fatal (z 110)" layer="fatal" title="Fatal layer" dismissible={false} />
      <div data-gallery-row="Live (real overlay)" className="space-y-2">
        <h3 className="font-semibold text-ink-body">Live: opens a real full-screen dialog</h3>
        <span data-gallery-target><Button onClick={() => setLive(true)}>Open dialog</Button></span>
        <Dialog
          open={live}
          onClose={() => setLive(false)}
          title="Set up song order?"
          description="Escape, backdrop click and Cancel close this."
          actions={<><Button size="touch" variant="secondary" onClick={() => setLive(false)}>Cancel</Button><Button size="touch" onClick={() => setLive(false)}>Confirm</Button></>}
        />
      </div>
    </Section>
  );
}
