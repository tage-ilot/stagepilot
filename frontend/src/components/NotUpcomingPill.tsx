import { formatShortDate } from "../lib/servicePlans";
import { Badge } from "./ui/Badge";
import { Button } from "./ui/Button";
import { cx } from "./ui/cx";

/**
 * "Not the upcoming service: Sun, Nov 29" with a Back to upcoming button. The operator picked another service than the
 * upcoming one; this says so. It is a neutral warning, never an error, and it has no say in whether the system is Ready.
 */
export function NotUpcomingPill({
  date,
  onBack,
  canOperate,
  pending = false,
  className,
}: {
  /** The picked service's date (`YYYY-MM-DD`). */
  date: string;
  onBack: () => void;
  canOperate: boolean;
  pending?: boolean;
  className?: string;
}) {
  return (
    <span className={cx("inline-flex flex-wrap items-center gap-2", className)} data-testid="not-upcoming">
      <Badge data-testid="not-upcoming-pill" ignoreAccent tone="warning">
        Not the upcoming service: {formatShortDate(date)}
      </Badge>
      {canOperate && (
        <Button disabled={pending} onClick={onBack} variant="secondary">
          Back to upcoming
        </Button>
      )}
    </span>
  );
}
