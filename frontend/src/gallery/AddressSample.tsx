import { useState } from "react";

import { AddressBox, ConnectionRow, ConnectionSection, LiveLinkField, SectionNotice, type ScannedAddress } from "../components/sections";
import { HeldMarker } from "../components/ui/Field";
import type { AddressBlock } from "./addressBlocks";
import { Scripted, type Step } from "./Scripted";

const noop = () => undefined;

export interface Setup {
  value?: string;
  recent?: string[];
  scanned?: ScannedAddress[];
  /** The list is drawn open (a room is kept under the box for it). */
  open?: boolean;
  /** Keys a person presses in the box, one after another (ArrowDown opens the list and moves through it). */
  keys?: string[];
  error?: boolean;
  held?: boolean;
  disabled?: boolean;
}


export function Box({ block, setup }: { block: AddressBlock; setup: Setup }) {
  const [value, setValue] = useState(setup.value ?? "");
  const marker = setup.held ? <HeldMarker /> : undefined;
  const steps: Step[] = (setup.keys ?? []).map((key) => ({ key: [block.label, key] }));
  return (
    <Scripted steps={steps}>
      <div className={setup.open ? "pb-72" : undefined}>
        <ConnectionSection
          className="!mt-0"
          connection={(
            <>
              <ConnectionRow connectDisabled={setup.disabled} onConnect={noop} onScan={noop} scanDisabled={setup.disabled} tone={block.tone}>
                <LiveLinkField caption={block.caption} className="live-link-host" marker={marker}>
                  <AddressBox
                    aria-label={block.label}
                    defaultOpen={setup.open}
                    disabled={setup.disabled}
                    error={setup.error}
                    held={setup.held}
                    onChange={setValue}
                    placeholder={block.placeholder}
                    recent={setup.recent ?? []}
                    scanned={setup.scanned}
                    scope={block.scope}
                    value={value}
                  />
                </LiveLinkField>
              </ConnectionRow>
              {setup.error && <SectionNotice className="mt-1" role="alert" size="xs" tone="danger" variant="line">Enter the computer's name or an address like 192.0.2.10 (no spaces).</SectionNotice>}
            </>
          )}
          title={block.title}
        />
      </div>
    </Scripted>
  );
}

