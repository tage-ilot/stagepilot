import type { ReactNode } from "react";

import { Button } from "../ui/Button";

const subButton = "!h-8 !min-h-8 !px-2.5 !font-black text-sm coarse:!h-11 coarse:!min-h-[44px]";

export function DisclosureButton({ open, controls, onToggle, children }: { open: boolean; controls: string; onToggle: () => void; children: ReactNode }) {
  return (
    <Button aria-controls={controls} aria-expanded={open} className={subButton} onClick={onToggle} variant="secondary">
      {children}
      {/* Conventional state arrow: down when open, right when closed. Decorative; aria-expanded carries the state. */}
      <span aria-hidden="true" className="ml-1.5 text-ink-muted" data-part="arrow">{open ? "\u25BE" : "\u25B8"}</span>
    </Button>
  );
}
