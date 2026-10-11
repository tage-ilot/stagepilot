import type { DashboardSpacerId } from "./dashboardLayoutTypes";

export function DashboardSpacer({
  editing,
  id,
  onRemove,
}: {
  editing: boolean;
  id: DashboardSpacerId;
  onRemove: (id: DashboardSpacerId) => void;
}) {
  if (!editing) {
    return <div aria-hidden="true" className="pointer-events-none h-full opacity-0" />;
  }
  return (
    <div className="flex h-full min-h-0 items-center justify-center rounded-ds-xl border border-dashed border-info/40 bg-info/5">
      <div className="text-center">
        <p className="text-xs font-bold uppercase tracking-wider text-info-text">Intentional spacer</p>
        <button
          aria-label="Remove dashboard spacer"
          className="mt-2 touch-manipulation rounded-ds-lg border border-danger/30 bg-danger/10 px-2.5 py-1.5 text-xs font-semibold text-danger-text transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-danger/20 coarse:min-h-ds-touch-target"
          onClick={() => onRemove(id)}
          type="button"
        >
          Remove
        </button>
      </div>
    </div>
  );
}
