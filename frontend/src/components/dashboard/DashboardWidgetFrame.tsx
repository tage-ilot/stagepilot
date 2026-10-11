import type { ReactNode } from "react";

import type { DashboardItemId } from "./dashboardLayoutTypes";

export function DashboardWidgetFrame({
  children,
  editing,
  first,
  id,
  label,
  last,
  onMove,
}: {
  children: ReactNode;
  editing: boolean;
  first: boolean;
  id: DashboardItemId;
  label: string;
  last: boolean;
  onMove: (id: DashboardItemId, offset: -1 | 1) => void;
}) {
  return (
    <div className={`dashboard-widget-frame relative h-full min-h-0 ${editing ? "rounded-ds-xl ring-1 ring-primary/40" : ""}`}>
      {editing && (
        <div className="absolute right-2 top-2 z-10 flex items-center gap-1 rounded-ds-lg border border-edge bg-surface-950/90 p-1 shadow-elevation-default">
          <button
            aria-label={`Move ${label} earlier`}
            className="grid h-7 w-7 coarse:h-ds-touch-target coarse:w-ds-touch-target place-items-center touch-manipulation rounded-ds-lg text-sm text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-ink/10 can-hover:text-ink disabled:opacity-25"
            disabled={first}
            onClick={() => onMove(id, -1)}
            title={`Move ${label} earlier`}
            type="button"
          >
            ←
          </button>
          <button
            aria-label={`Drag ${label} to a new dashboard position`}
            className="dashboard-widget-handle flex h-7 coarse:h-ds-touch-target cursor-grab touch-none select-none items-center gap-1 rounded-ds-lg px-2 font-type-micro text-type-micro uppercase text-primary-focus transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-ink/10 active:cursor-grabbing"
            title={`Drag ${label} to a new dashboard position`}
            type="button"
          >
            <span aria-hidden="true" className="text-base leading-none">⠿</span>
            Move
          </button>
          <button
            aria-label={`Move ${label} later`}
            className="grid h-7 w-7 coarse:h-ds-touch-target coarse:w-ds-touch-target place-items-center touch-manipulation rounded-ds-lg text-sm text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-ink/10 can-hover:text-ink disabled:opacity-25"
            disabled={last}
            onClick={() => onMove(id, 1)}
            title={`Move ${label} later`}
            type="button"
          >
            →
          </button>
        </div>
      )}
      <div className="h-full min-h-0">{children}</div>
    </div>
  );
}
