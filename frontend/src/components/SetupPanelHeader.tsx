import type { ConnectionStatus } from "../types";
import { StatusBadge } from "./ui/Badge";
import { Button } from "./ui/Button";
import { connectionWord } from "./ui/statusWord";

type SetupPanelStatus = ConnectionStatus | "loading";

export function SetupPanelHeader({
  title,
  description,
  status,
  closeLabel,
  headingId,
  onClose,
}: {
  title: string;
  description: string;
  status: SetupPanelStatus;
  closeLabel: string;
  headingId?: string;
  onClose?: () => void;
}) {
  // Text and close share row one; below 480px the badge drops to row two, above it sits between them.
  return (
    <div
      className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-3 gap-y-3 min-[480px]:grid-cols-[minmax(0,1fr)_auto_auto]"
      data-setup-panel-header
    >
      <div className="col-start-1 row-start-1 min-w-0">
        <p className="text-xs font-bold uppercase tracking-[0.22em] text-ink-muted">
          Connection configuration
        </p>
        <h2 className="mt-1 text-lg font-semibold text-ink" id={headingId}>
          {title}
        </h2>
        <p className="mt-1 max-w-3xl text-sm text-ink-muted">{description}</p>
      </div>
      <StatusBadge
        className="col-span-2 row-start-2 justify-self-start min-[480px]:col-span-1 min-[480px]:col-start-2 min-[480px]:row-start-1 min-[480px]:self-center"
        data-status={status}
        data-status-badge
        status={connectionWord[status]}
      />
      {onClose && (
        <Button
          aria-label={closeLabel}
          className="col-start-2 row-start-1 min-w-ds-touch-target leading-none min-[480px]:col-start-3"
          onClick={onClose}
          size="touch"
          title="Close"
          variant="secondary"
        >
          ×
        </Button>
      )}
    </div>
  );
}
