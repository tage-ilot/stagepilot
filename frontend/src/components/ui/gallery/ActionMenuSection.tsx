import { useState } from "react";

import { ActionMenu } from "../ActionMenu";
import { Section, StateRow } from "./Section";
import type { GalleryState } from "./states";

const items = [
  { value: "type", label: "Sunday Service" },
  { value: "youth", label: "Youth" },
  { value: "kids", label: "Kids and Family Weekend Gathering, extended long name" },
];

function Live({ state, forceOpen }: { state: GalleryState; forceOpen?: boolean }) {
  const [value, setValue] = useState("youth");
  if (state === "loading" || state === "error") {
    return <p data-gallery-unavailable className="text-sm text-ink-muted">Not applicable: the trigger has no {state} state (the caller shows progress or errors beside it).</p>;
  }
  return (
    <div className={forceOpen ? "min-h-72" : undefined}>
      <span data-gallery-target className="inline-block max-w-full">
        <ActionMenu
          label="Plan category"
          menuLabel="Plan categories"
          items={items}
          value={value}
          onSelect={setValue}
          disabled={state === "disabled"}
          {...(forceOpen && state !== "disabled" ? { open: true } : {})}
        />
      </span>
    </div>
  );
}

/** Real ActionMenu. The first row opens on click/ArrowDown; the second is pinned open to show the card. */
export function ActionMenuSection() {
  return (
    <Section name="actionmenu" title="ActionMenu">
      <StateRow label="closed trigger" render={(s) => <Live state={s} />} />
      <StateRow label="open, current item checked" render={(s) => <Live state={s} forceOpen />} />
    </Section>
  );
}
