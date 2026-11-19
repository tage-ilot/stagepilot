import type { ReactNode } from "react";

/**
 * The room right under a connection row for what a scan has to say: the chooser, the help, the last message. Empty it takes no space.
 * Put it directly after the `ConnectionRow` (and the recent addresses).
 */
export function ScanArea({ children }: { children: ReactNode }) {
  return <div className="mt-2 space-y-3 empty:hidden" data-part="scan-area">{children}</div>;
}
