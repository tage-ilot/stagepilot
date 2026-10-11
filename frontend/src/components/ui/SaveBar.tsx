import { useId, type ReactNode } from "react";

import { Button } from "./Button";
import { changeCountText } from "./changeCount";
import { cx } from "./cx";

export type SaveBarState =
  | { kind: "clean" }
  | { kind: "auto"; at: string }
  | { kind: "saving" }
  | { kind: "held"; count: number }
  | { kind: "error"; message: string; retryLabel?: "Try again" };

export interface SaveBarProps {
  state: SaveBarState;
  onSave?: () => void;
  onDiscard?: () => void;
  onRetry?: () => void;
  /** Fixed wording; DESIGN.md Saving. The button is never hidden. */
  saveLabel?: "Save configuration";
  /** Sticky at the bottom below the lg breakpoint (phones and tablets). Default true. */
  stickyOnPhone?: boolean;
  className?: string;
}

// Class strings are written out in full so Tailwind can see them.
const edges: Record<SaveBarState["kind"], string> = {
  clean: "border-edge",
  auto: "border-success/40",
  saving: "border-edge",
  held: "border-warning/60",
  error: "border-danger/60",
};

function Check() {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="h-4 w-4 shrink-0 text-success-text" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 8.5l3.2 3.2L13 4.8" />
    </svg>
  );
}

function status(state: SaveBarState): { text: ReactNode; note?: string; tone: string } {
  switch (state.kind) {
    case "clean":
      return { text: "All changes saved", note: "No changes to save", tone: "text-ink-body" };
    case "auto":
      return {
        text: (
          <>
            Saved automatically at <span className="tabular-nums">{state.at}</span>
          </>
        ),
        tone: "text-success-text",
      };
    case "saving":
      return { text: "Saving…", tone: "text-ink-body" };
    case "held":
      return {
        text: (
          <>
            <span className="tabular-nums">{changeCountText(state.count)}</span> waiting. Not saved yet.
          </>
        ),
        tone: "text-warning-text",
      };
    case "error":
      return { text: state.message, tone: "text-danger-text" };
  }
}

export function SaveBar({
  state,
  onSave,
  onDiscard,
  onRetry,
  saveLabel = "Save configuration",
  stickyOnPhone = true,
  className,
}: SaveBarProps) {
  const reasonId = useId();
  const { text, note, tone } = status(state);
  const kind = state.kind;
  const clean = kind === "clean";
  const saving = kind === "saving";

  return (
    <div
      data-save-bar
      data-state={kind}
      data-sticky={stickyOnPhone ? "true" : undefined}
      className={cx(
        "flex flex-col gap-3 rounded-ds-lg border bg-surface-900 p-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] min-[481px]:flex-row min-[481px]:items-center min-[481px]:justify-between",
        edges[kind],
        stickyOnPhone && "sticky bottom-0 z-10 lg:static",
        className,
      )}
    >
      <div aria-live="polite" className={cx("flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1 break-words font-type-body-md text-type-body-md", tone)}>
        {kind === "auto" && <Check />}
        <span data-save-status className="min-w-0 break-words">{text}</span>
        {note && <span id={reasonId} data-save-reason className="text-ink-muted">{note}</span>}
      </div>
      <div className="flex flex-col gap-2 min-[481px]:flex-row min-[481px]:shrink-0">
        {kind === "held" && (
          <Button variant="secondary" fullWidth className="min-[481px]:w-auto" onClick={onDiscard}>
            Discard
          </Button>
        )}
        {kind === "error" && (
          <Button variant="secondary" fullWidth className="min-[481px]:w-auto" onClick={onRetry}>
            {state.retryLabel ?? "Try again"}
          </Button>
        )}
        <Button
          variant="primary"
          fullWidth
          className={cx("min-[481px]:w-auto", clean && "opacity-40")}
          loading={saving}
          aria-disabled={clean ? true : undefined}
          aria-describedby={clean ? reasonId : undefined}
          onClick={onSave}
        >
          {saveLabel}
        </Button>
      </div>
    </div>
  );
}
