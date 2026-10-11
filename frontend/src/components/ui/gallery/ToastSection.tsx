import { Button } from "../Button";
import { Toast, type ToastSide, type ToastTone } from "../Toast";
import { Section, StateRow } from "./Section";
import type { GalleryState } from "./states";

function Frame({ children }: { children: React.ReactNode }) {
  // A transformed box makes `fixed` descendants position inside it, so each toast is visible in page flow.
  return <div className="relative h-[28rem] overflow-hidden rounded-ds-lg border border-edge-subtle bg-surface-900 [transform:translateZ(0)]">{children}</div>;
}

const longTitle = "StagePilot's backend keeps crashing and could not restart after the last three attempts";

function Cell({ state, side, tone, title = "StagePilot ran into a problem", description = "This didn't stop the app, but you can send logs to the developer to help track it down." }: { state: GalleryState; side: ToastSide; tone?: ToastTone; title?: string; description?: string }) {
  if (state === "error") {
    return <p data-gallery-unavailable className="text-sm text-ink-muted">Not applicable: a toast is itself the error notice.</p>;
  }
  return (
    <Frame>
      <Toast
        side={side}
        className="!max-w-[calc(100%-3rem)]"
        tone={tone}
        kicker={tone === "warning" ? "Heads up" : "Something went wrong"}
        title={title}
        description={description}
        onDismiss={() => {}}
        actions={
          <Button data-gallery-target size="touch" variant="destructive-soft" disabled={state === "disabled"} loading={state === "loading"} loadingLabel="Sending…">
            Send logs to developer
          </Button>
        }
      />
    </Frame>
  );
}

/** Real Toast. The frame stands in for the viewport (max width follows the frame, as 100vw-3rem does in the app). Each sits inside a transformed frame so several show at once; the app mounts them fixed at the viewport corner. */
export function ToastSection() {
  return (
    <Section name="toast" title="Toast">
      <StateRow label="left, danger (global error)" render={(s) => <Cell state={s} side="left" />} />
      <StateRow label="right, danger (crash loop)" render={(s) => <Cell state={s} side="right" title="StagePilot's backend keeps crashing" description="Backend exited three times in a minute." />} />
      <StateRow label="left, warning" render={(s) => <Cell state={s} side="left" tone="warning" />} />
      <StateRow label="long title" render={(s) => <Cell state={s} side="right" title={longTitle} />} />
    </Section>
  );
}
