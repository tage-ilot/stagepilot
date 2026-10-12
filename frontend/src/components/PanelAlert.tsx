import { Button } from "./ui/Button";
import { InlineAlert } from "./ui/InlineAlert";

/**
 * The one inline alert a setup panel shows for a failed load or save (or a status message).
 * `onRetry` is passed only when the panel has nothing to show yet (a failed load); save
 * failures already have `Try again` in the SaveBar. Values in the form are never touched here.
 */
export function PanelAlert({
  error,
  message,
  onRetry,
  className,
  messageTone = "info",
  dot = false,
}: {
  error: string | null;
  message: string | null;
  onRetry?: () => void;
  className?: string;
  messageTone?: "info" | "success";
  /** Informational result line: a small dot of the tone colour before the message (never on errors). */
  dot?: boolean;
}) {
  if (!error && !message) return null;
  return (
    <InlineAlert aria-live="polite" className={className} tone={error ? "danger" : messageTone}>
      {dot && !error && <span aria-hidden="true" data-part="dot" className="mr-2 inline-block size-1.5 rounded-ds-full bg-info align-middle" />}
      <span>{error ?? message}</span>
      {error && onRetry && (
        <Button className="ml-3" onClick={onRetry} size="touch" variant="secondary">Try again</Button>
      )}
    </InlineAlert>
  );
}
