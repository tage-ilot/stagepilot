import type { ReactNode } from "react";

import { usePersistedFlag } from "../../hooks/usePersistedFlag";
import { Disclosure, type DisclosureTone, type DisclosureVariant } from "../ui/Disclosure";

interface Shared {
  /** The words on the closed row. */
  title: ReactNode;
  /** One line of what is inside, shown while closed (`Port 8765 · Logging info`). */
  summary?: ReactNode;
  /** `feature`: a second line, always shown, saying what is inside. */
  detail?: ReactNode;
  /** `feature`: a small tag at the right of the header. */
  aside?: ReactNode;
  /** `activity`: a count badge (`12 events`). */
  count?: number;
  countNoun?: string;
  /** The connection's colour (the panel's tone). Needed for `advanced` and `feature`. */
  tone?: DisclosureTone;
  /** `advanced`: extra options (after the general settings). `activity`: read-only or rarely used (Remote access). `feature`: a major optional feature (Song lighting timeline). */
  variant?: DisclosureVariant;
  /** Open while something needs the person (an invalid field inside, a question being asked), whatever they chose; their choice is left alone. */
  forceOpen?: boolean;
  className?: string;
  testId?: string;
  children: ReactNode;
}

export interface OptionalSectionProps extends Shared {
  /** Keeps the person's choice between launches under this localStorage key (DESIGN.md: persist the collapse state of optional sections). */
  persistKey?: string;
  /** Without `persistKey`: closed unless told (the choice lasts while the section is on screen). */
  defaultOpen?: boolean;
  /** A page that holds the state itself (it must open when a field inside is invalid). */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

function Persisted({ persistKey, defaultOpen, forceOpen, onOpenChange, testId, ...rest }: Shared & { persistKey: string; defaultOpen: boolean; onOpenChange?: (open: boolean) => void }) {
  const [saved, setSaved] = usePersistedFlag(persistKey, defaultOpen);
  return <Disclosure data-testid={testId} {...rest} onOpenChange={(next) => { setSaved(next); onOpenChange?.(next); }} open={saved || Boolean(forceOpen)} variant={rest.variant ?? "advanced"} />;
}

function Plain({ defaultOpen, open, forceOpen, onOpenChange, testId, ...rest }: Shared & { defaultOpen: boolean; open?: boolean; onOpenChange?: (open: boolean) => void }) {
  const props = open === undefined && !forceOpen ? { defaultOpen } : { open: Boolean(open) || Boolean(forceOpen) };
  return <Disclosure data-testid={testId} {...rest} {...props} onOpenChange={onOpenChange} variant={rest.variant ?? "advanced"} />;
}

/**
 * The OPTIONAL section: a block some people never use, folded away behind one row. Advanced settings, a read-only activity group or a
 * whole optional feature, each the `Disclosure` the app already has (so the look, the 500ms motion and `aria-expanded`/`aria-controls`
 * are one edit in `ui/Disclosure`); this adds the rule of DESIGN.md: the open flag is remembered (`persistKey`) and a question or an
 * invalid field inside opens it (`forceOpen`) without changing what the person chose.
 */
export function OptionalSection({ persistKey, defaultOpen = false, open, onOpenChange, ...rest }: OptionalSectionProps) {
  return persistKey
    ? <Persisted {...rest} defaultOpen={defaultOpen} onOpenChange={onOpenChange} persistKey={persistKey} />
    : <Plain {...rest} defaultOpen={defaultOpen} onOpenChange={onOpenChange} open={open} />;
}
