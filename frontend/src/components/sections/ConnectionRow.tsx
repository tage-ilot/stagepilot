import type { FormEventHandler, ReactNode } from "react";

import { cx } from "../ui/cx";

export interface ConnectionRowProps {
  /** The address input(s), the port and the section's one or two main buttons (Connect, Scan Network, Test connection). */
  children: ReactNode;
  /** `form`: the row is a form, so Enter in the address field presses Connect (Playback). */
  as?: "div" | "form";
  onSubmit?: FormEventHandler<HTMLFormElement>;
  id?: string;
  /** A row inside another row (the Mode chooser beside Connect) takes no space above it. */
  nested?: boolean;
  testId?: string;
  className?: string;
}

/**
 * The CONNECTION ROW of a `ConnectionSection`: one wrapping line, bottoms aligned, of the address, the port and the main buttons. It is the
 * one place that says how far apart they sit and where they wrap, so Lighting, Playback and Presentation cannot drift apart.
 */
export function ConnectionRow({ children, as = "div", onSubmit, id, nested = false, testId, className }: ConnectionRowProps) {
  const shared = { className: cx(!nested && "mt-2", "flex flex-wrap items-end gap-x-2.5 gap-y-2", className), "data-part": "connection-row", "data-testid": testId, id };
  return as === "form" ? <form {...shared} onSubmit={onSubmit}>{children}</form> : <div {...shared}>{children}</div>;
}
