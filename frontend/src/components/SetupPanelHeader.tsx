import type { ConnectionStatus } from "../types";
import { Badge, StatusBadge } from "./ui/Badge";
import { Button } from "./ui/Button";
import type { PanelIdentity } from "./ui/Panel";
import { accentClasses, useAccent } from "./ui/accent";
import { cx } from "./ui/cx";
import { connectionWord } from "./ui/statusWord";

type SetupPanelStatus = ConnectionStatus | "loading";

export function SetupPanelHeader({
  title,
  description,
  kicker,
  status,
  closeLabel,
  headingId,
  onClose,
  statusDetail,
  titleTone,
}: {
  title: string;
  /** The sentence under the title; omit it to show none. */
  description?: string;
  /** The small line above the title. Default: "Connection configuration". */
  kicker?: string;
  status: SetupPanelStatus;
  closeLabel: string;
  headingId?: string;
  onClose?: () => void;
  /** Extra badge after the status word, e.g. the saved alternate connection method. */
  statusDetail?: string;
  /** Keeps the title in this identity's colour when the controls wear another accent (Lighting on its MIDI method). */
  titleTone?: PanelIdentity;
}) {
  const accent = useAccent();
  // Text and close share row one; below 480px the badge drops to row two, above it sits between them.
  return (
    <div
      className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-3 gap-y-3 min-[480px]:grid-cols-[minmax(0,1fr)_auto_auto]"
      data-setup-panel-header
    >
      <div className="col-start-1 row-start-1 min-w-0">
        <p className="font-type-kicker text-type-kicker uppercase text-ink-muted">
          {kicker ?? "Connection configuration"}
        </p>
        <h2 className={cx("mt-1 text-xl font-bold", titleTone ? accentClasses[titleTone].title : accent ? accentClasses[accent === "midi" ? "playback" : accent].title : "text-ink")} id={headingId}>
          {title}
        </h2>
        {description ? <p className="mt-1 max-w-3xl text-sm text-ink-body">{description}</p> : null}
      </div>
      {/* The status pill always sits directly left of the close button, centred on its 44px height, on every connection page
          (owner, 2026-10-14). It used to centre on the title block, so it moved with the description. An extra badge, such as the
          saved alternate method, goes before the pill so the pill stays next to the close button. */}
      <div
        className="col-span-2 row-start-2 flex flex-wrap items-center gap-2 justify-self-start min-[480px]:col-span-1 min-[480px]:col-start-2 min-[480px]:row-start-1 min-[480px]:h-ds-touch-target min-[480px]:flex-nowrap min-[480px]:justify-self-end"
        data-status-slot
      >
        {statusDetail ? <Badge tone={accent === "midi" ? "midi" : "playback"}>{statusDetail}</Badge> : null}
        <StatusBadge data-status={status} data-status-badge status={connectionWord[status]} />
      </div>
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
