import { useId, useState, type ReactNode } from "react";

import { CUE_ADDRESS_HELP } from "./liveLink";
import { DisclosureButton } from "./sections";

/**
 * The two quick actions of the Lighting Live Link block and the boxes they open: Send a test cue and How to find a cue's address. The
 * buttons go to the section's `actions` (beside the Details button) and the boxes to its `panels`; both boxes stay in the DOM, hidden
 * until opened, so the words are always readable.
 */
export function useLiveLinkTools(testCue?: ReactNode): { actions: ReactNode; panels: ReactNode } {
  const [cueOpen, setCueOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const cueId = useId();
  const helpId = useId();
  return {
    actions: (
      <>
        {testCue && <DisclosureButton controls={cueId} onToggle={() => setCueOpen(!cueOpen)} open={cueOpen}>Send a test cue</DisclosureButton>}
        <DisclosureButton controls={helpId} onToggle={() => setHelpOpen(!helpOpen)} open={helpOpen}>How to find a cue&apos;s address</DisclosureButton>
      </>
    ),
    panels: (
      <>
        <p className="mt-2 rounded-ds-lg border border-edge-subtle bg-surface-950 px-3 py-2 text-type-body-sm text-ink-body" data-testid="cue-address-help" hidden={!helpOpen} id={helpId}>{CUE_ADDRESS_HELP}</p>
        {testCue && (
          <div className="mt-2 rounded-ds-lg border border-edge bg-surface-900/60 p-3" data-testid="test-cue-box" hidden={!cueOpen} id={cueId}>
            {testCue}
          </div>
        )}
      </>
    ),
  };
}
