import {
  hideDesktopWindow,
  isDesktopShell,
  minimizeDesktopWindow,
  toggleMaximizeDesktopWindow,
} from "../desktop";

const runWindowAction = (action: () => Promise<void>) => {
  void action().catch(() => undefined);
};

export function DesktopTitleBar() {
  if (!isDesktopShell()) return null;

  return (
    <div
      aria-label="Window controls"
      className="sticky top-0 z-[100] h-9 select-none coarse:h-ds-touch-target"
      data-tauri-drag-region
      onDoubleClick={() => runWindowAction(toggleMaximizeDesktopWindow)}
    >
      <div className="absolute inset-y-0 right-0 flex" onDoubleClick={(event) => event.stopPropagation()}>
        <button
          aria-label="Minimize window"
          className="grid h-9 w-12 place-items-center text-lg leading-none touch-manipulation text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-surface-panel-strong can-hover:text-ink coarse:h-ds-touch-target coarse:w-ds-touch-target"
          onClick={() => runWindowAction(minimizeDesktopWindow)}
          type="button"
        >
          <span className="-translate-y-1" aria-hidden="true">−</span>
        </button>
        <button
          aria-label="Maximize or restore window"
          className="grid h-9 w-12 place-items-center touch-manipulation text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-surface-panel-strong can-hover:text-ink coarse:h-ds-touch-target coarse:w-ds-touch-target"
          onClick={() => runWindowAction(toggleMaximizeDesktopWindow)}
          type="button"
        >
          <span aria-hidden="true" className="h-3 w-3 border border-current" />
        </button>
        <button
          aria-label="Hide StagePilot window"
          className="grid h-9 w-12 place-items-center text-xl leading-none touch-manipulation text-ink-body transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-danger-solid can-hover:text-on-danger coarse:h-ds-touch-target coarse:w-ds-touch-target"
          onClick={() => runWindowAction(hideDesktopWindow)}
          title="Hide StagePilot (it will keep running)"
          type="button"
        >
          <span aria-hidden="true">×</span>
        </button>
      </div>
    </div>
  );
}
