import type { ReactNode } from "react";

import { cx } from "../ui/cx";
import { ScanConnectButtons, type ScanConnectProps } from "./ScanConnectButtons";

export interface ConnectionRowProps extends Omit<ScanConnectProps, "submit"> {
  /** The address input(s) and the port. The two buttons are NOT passed in: the row draws them itself, after the children. */
  children: ReactNode;
  /** `form`: the row is a form, so Enter in the address field presses Connect (Playback). */
  as?: "div" | "form";
  id?: string;
  /** A row inside another row (the Mode chooser beside Connect) takes no space above it and draws no buttons. */
  nested?: boolean;
  /** Something that belongs at the far end of the row, after the buttons (Lighting's Mode chooser). */
  after?: ReactNode;
  testId?: string;
  className?: string;
}

/**
 * The CONNECTION ROW of a `ConnectionSection`: one wrapping line, bottoms aligned, of the address, the port and the two buttons that
 * end every row, **Scan** and **Connect**, in that order, the same size in the same place in Lighting, Playback and Presentation. It is the
 * one place that says how far apart they sit and where they wrap, so the three blocks cannot drift apart.
 *
 * `onScan` / `onCancelScan` / `scanning`, `onConnect` / `connecting` and the two `...Disabled` flags are the only things a block says about
 * the buttons; their words, size and order are not up to the block. A block without a scan still shows both (Scan is switched off, with its hint).
 */
export function ConnectionRow({ children, as = "div", id, nested = false, after, testId, className, ...buttons }: ConnectionRowProps) {
  const shared = { className: cx(!nested && "mt-2", "flex flex-wrap items-end gap-x-2.5 gap-y-2", className), "data-part": "connection-row", "data-testid": testId, id };
  const pair = nested ? null : <ScanConnectButtons {...buttons} submit={as === "form"} />;
  if (as === "form") {
    return (
      <form {...shared} onSubmit={(event) => { event.preventDefault(); if (!buttons.connectDisabled && !buttons.connecting) buttons.onConnect?.(); }}>
        {children}
        {pair}
        {after}
      </form>
    );
  }
  return <div {...shared}>{children}{pair}{after}</div>;
}
