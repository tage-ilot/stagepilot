import type { ReactNode } from "react";

import { connectionWord } from "./ui";

type Status = "connected" | "connecting" | "disconnected" | "error";

const statusTextStyles: Record<Status, string> = {
  connected: "text-success-text",
  connecting: "text-warning-text",
  disconnected: "text-ink-muted",
  error: "text-danger-text",
};

type Identity = "services" | "presentation" | "playback" | "lighting" | "backend";

// Identity tints the icon tile always, and the OPEN card's border, wash and left bar (index.css, data-tone). Never a rail or wash on a closed card.
// -soft is the role for tints and borders on dark surfaces (Playback base and soft are the same slate now).
const tileTint: Record<Identity, string> = {
  services: "border-integration-services-soft/30 bg-integration-services-soft/15",
  presentation: "border-integration-presentation-soft/30 bg-integration-presentation-soft/15",
  playback: "border-integration-playback-soft/30 bg-integration-playback-soft/15",
  lighting: "border-integration-lighting-soft/30 bg-integration-lighting-soft/15",
  backend: "border-integration-backend-soft/30 bg-integration-backend-soft/15",
};

export function StatusCard({
  tone,
  title,
  accessibleTitle = title,
  status,
  detail,
  icon,
  active,
  controls,
  onClick,
  interactive = true,
}: {
  /** Connection identity: tints the icon tile and the open card. */
  tone?: Identity;
  title: string;
  accessibleTitle?: string;
  status: Status;
  detail: string;
  icon: ReactNode;
  active: boolean;
  controls: string;
  onClick: () => void;
  interactive?: boolean;
}) {
  const Element = interactive ? "button" : "div";
  return (
    <Element
      aria-label={`${accessibleTitle} ${connectionWord[status]}. ${detail}`}
      aria-controls={interactive ? controls : undefined}
      aria-expanded={interactive ? active : undefined}
      data-status={status}
      data-tone={tone}
      className={`status-card group min-w-0 rounded-xl border p-3 text-left touch-manipulation transition-colors duration-150 motion-reduce:transition-none sm:p-4 ${
        active
          ? "border-edge-medium"
          : "border-edge-subtle"
      }`}
      onClick={interactive ? onClick : undefined}
      type={interactive ? "button" : undefined}
    >
      <span aria-hidden="true" className="status-card-surface" data-status-motion-part="surface" />
      <div className="status-card-layout grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-start gap-x-1 xl:gap-x-3">
        <div className="status-card-copy min-w-0">
          <p className="status-card-title whitespace-nowrap text-type-micro font-semibold uppercase tracking-[0.14em] text-ink-muted transition-colors can-hover:group-hover:text-ink-body" title={title}>{title}</p>
          <div className="status-card-status mt-2 flex min-w-0 items-center gap-1 xl:gap-2" data-status-motion-part="status">
            <span className={`status-card-status-word whitespace-nowrap font-semibold ${statusTextStyles[status]}`}>{connectionWord[status]}</span>
          </div>
        </div>
        <span
          aria-hidden="true"
          className={`status-card-icon grid h-12 w-12 shrink-0 place-items-center rounded-ds-xl border ${tone ? tileTint[tone] : "border-edge-subtle bg-edge-subtle"} text-ink-muted transition-colors can-hover:group-hover:text-ink`}
          data-status-icon-background
          data-status-motion-part="icon"
        >
          <span className="status-card-icon-content grid h-9 w-9 place-items-center overflow-hidden rounded-ds-lg">
            {icon}
          </span>
        </span>
        <p className="status-card-detail col-span-2 mt-2 truncate text-xs text-ink-muted transition-colors can-hover:group-hover:text-ink-body" title={detail}>{detail}</p>
      </div>
    </Element>
  );
}
