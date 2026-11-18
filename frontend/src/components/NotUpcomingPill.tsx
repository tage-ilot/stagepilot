import { Badge } from "./ui/Badge";
import { Button } from "./ui/Button";
import { cx } from "./ui/cx";

/**
 * A "Current Upcoming" button followed by "Current upcoming service not selected". The operator picked another service than the
 * upcoming one; this says so. It is a neutral warning, never an error, and it has no say in whether the system is Ready.
 */
export function NotUpcomingPill({
  date,
  onBack,
  canOperate,
  pending = false,
  className,
  size = "control",
}: {
  /** The picked service's date (`YYYY-MM-DD`). */
  date: string;
  onBack: () => void;
  canOperate: boolean;
  pending?: boolean;
  className?: string;
  /** The button's height: "touch" beside the Calendar button so the two match. */
  size?: "control" | "touch";
}) {
  return (
    <span className={cx("inline-flex flex-wrap items-center gap-2", className)} data-testid="not-upcoming" data-date={date}>
      {canOperate && (
        <Button disabled={pending} onClick={onBack} size={size} variant="secondary">
          Current Upcoming
        </Button>
      )}
      <Badge data-testid="not-upcoming-pill" ignoreAccent tone="warning">
        Current upcoming service not selected
      </Badge>
    </span>
  );
}
