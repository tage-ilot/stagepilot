import type { UpdaterState } from "../hooks/useUpdater";
import { Button, Dialog, InlineAlert } from "./ui";

const formatBytes = (bytes: number) => {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
};

export function UpdateDialog({
  updater,
  onCancel,
  onConfirm,
  onRetry,
  onCloseError,
  returnFocus,
}: {
  updater: UpdaterState;
  onCancel: () => void;
  onConfirm: () => void;
  onRetry: () => void;
  onCloseError: () => void;
  returnFocus: React.RefObject<HTMLButtonElement | null>;
}) {
  const active = ["confirmation", "downloading", "installing", "restarting"].includes(updater.status)
    || (updater.status === "error" && updater.errorDialogOpen);
  const locked = ["downloading", "installing", "restarting"].includes(updater.status);

  if (!active) return null;
  const progress = updater.progress;
  const stage = updater.status === "restarting"
    ? "Restarting StagePilot"
    : progress?.stage === "installing"
      ? "Installing update"
      : progress?.stage === "verifying"
        ? "Verifying signature"
        : progress?.stage === "downloading"
          ? "Downloading update"
          : "Preparing update";
  // Escape and the Cancel button hand focus back to the opener explicitly (as before); Dialog's own return to the
  // previously focused element runs afterwards and lands on the same button.
  const dismiss = () => {
    if (updater.status === "confirmation") onCancel();
    else if (updater.status === "error") onCloseError();
    returnFocus.current?.focus();
  };
  const kicker = "text-xs font-black uppercase tracking-[0.18em]";

  return (
    <Dialog
      open
      layer="update"
      dismissible={!locked}
      onClose={dismiss}
      title={updater.status === "confirmation" ? "Update StagePilot?" : updater.status === "error" ? "StagePilot stayed open" : stage}
      className="max-w-lg"
      actions={updater.status === "confirmation" ? (
        <>
          <Button onClick={() => { onCancel(); returnFocus.current?.focus(); }} size="touch" variant="secondary">Cancel</Button>
          <Button onClick={onConfirm} size="touch" variant="primary">Update and Restart</Button>
        </>
      ) : updater.status === "error" ? (
        <>
          <Button onClick={onCloseError} size="touch" variant="secondary">Close</Button>
          <Button onClick={onRetry} size="touch" variant="primary">Retry</Button>
        </>
      ) : undefined}
    >
      {updater.status === "confirmation" ? (
        <div className="space-y-4">
          <p className={`${kicker} text-info-text`}>Update available</p>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm tabular-nums">
            <dt className="text-ink-muted">Current</dt><dd className="font-mono text-ink">{updater.currentVersion}</dd>
            <dt className="text-ink-muted">Available</dt><dd className="font-mono text-info-text">{updater.availableVersion}</dd>
          </dl>
          <div className="max-h-48 overflow-y-auto overscroll-contain whitespace-pre-wrap rounded-ds-lg bg-edge-subtle p-3 text-sm text-ink-body">
            {updater.releaseNotes ?? "No release notes were supplied for this update."}
          </div>
          <p className="text-sm text-ink-muted">StagePilot will install the signed update and restart automatically.</p>
        </div>
      ) : updater.status === "error" ? (
        <div className="space-y-4">
          <p className={`${kicker} text-danger-text`}>Update interrupted</p>
          <InlineAlert tone="danger">{updater.error ?? "The update could not be installed."}</InlineAlert>
        </div>
      ) : (
        <div className="space-y-3">
          <p className={`${kicker} text-info-text`}>Updating StagePilot</p>
          <div
            aria-label="Update progress"
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={progress?.percentage ?? undefined}
            className="h-3 overflow-hidden rounded-ds-full bg-surface-900"
            role="progressbar"
          >
            <div
              className={`h-full rounded-ds-full bg-info transition-[width] duration-200 motion-reduce:transition-none ${progress?.percentage == null ? "w-1/3 animate-pulse" : ""}`}
              style={progress?.percentage == null ? undefined : { width: `${progress.percentage}%` }}
            />
          </div>
          <p className="text-sm text-ink-muted tabular-nums">
            {progress
              ? `${formatBytes(progress.downloadedBytes)}${progress.totalBytes ? ` of ${formatBytes(progress.totalBytes)}` : " downloaded"}`
              : "Preparing the signed update package…"}
          </p>
          <p className="text-xs text-ink-muted">No further action is required. Do not quit StagePilot.</p>
        </div>
      )}
    </Dialog>
  );
}
