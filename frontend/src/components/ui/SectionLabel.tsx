import type { ComponentPropsWithoutRef } from "react";

import { cx } from "./cx";

/**
 * A section heading inside a configuration panel (DESIGN.md "Panels read like the guide"): the small uppercase,
 * widely tracked kicker look, not a bold sentence. Still a real heading element for screen readers.
 */
export function SectionLabel({ className, ...rest }: ComponentPropsWithoutRef<"h3">) {
  return <h3 className={cx("mt-6 font-type-kicker text-type-kicker uppercase text-ink-muted", className)} {...rest} />;
}
