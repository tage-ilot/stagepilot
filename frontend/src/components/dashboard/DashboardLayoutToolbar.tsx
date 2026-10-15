export function DashboardLayoutToolbar({
  editing,
  eventsHidden,
  onCompact,
  onDone,
  onEdit,
  onReset,
  onToggleEventsHidden,
}: {
  editing: boolean;
  eventsHidden: boolean;
  onCompact: () => void;
  onDone: () => void;
  onEdit: () => void;
  onReset: () => void;
  onToggleEventsHidden: () => void;
}) {
  if (!editing) {
    return (
      <div className="mt-3 flex justify-end">
        <button
          className="touch-manipulation rounded-ds-lg border border-edge bg-surface-950/60 px-3 py-2 text-xs font-bold uppercase tracking-wider text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:border-edge-medium can-hover:bg-surface-950/90 can-hover:text-ink coarse:min-h-ds-touch-target"
          onClick={onEdit}
          type="button"
        >
          Edit layout
        </button>
      </div>
    );
  }

  return (
    <div
      aria-label="Dashboard layout tools"
      className="mt-3 flex flex-wrap items-center justify-end gap-2 rounded-ds-xl border border-primary/25 bg-surface-950/80 p-3 shadow-elevation-default"
      role="toolbar"
    >
      <span className="mr-auto text-xs font-bold uppercase tracking-[0.18em] text-primary-text">
        Layout editing
      </span>
      <button className="touch-manipulation rounded-ds-lg border border-edge-medium bg-surface-950 px-3 py-2 text-xs font-semibold text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-surface-900 coarse:min-h-ds-touch-target" onClick={onToggleEventsHidden} type="button">
        {eventsHidden ? "Show event stream" : "Hide event stream"}
      </button>
      <button className="touch-manipulation rounded-ds-lg border border-edge-medium bg-surface-950 px-3 py-2 text-xs font-semibold text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-surface-900 coarse:min-h-ds-touch-target" onClick={onCompact} type="button">
        Compact layout
      </button>
      <button className="touch-manipulation rounded-ds-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs font-semibold text-danger-text transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-danger/20 coarse:min-h-ds-touch-target" onClick={onReset} type="button">
        Reset layout
      </button>
      <button className="touch-manipulation rounded-ds-lg border border-transparent bg-primary px-3 py-2 text-xs font-bold text-on-primary text-shadow-label transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-primary-hover active:bg-primary-active coarse:min-h-ds-touch-target" onClick={onDone} type="button">
        Done
      </button>
    </div>
  );
}
