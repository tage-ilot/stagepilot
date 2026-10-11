import { Button, type ButtonProps } from "../Button";
import type { LiveTone, Tone } from "../types";
import { Section, StateRow } from "./Section";
import type { GalleryState } from "./states";

const tones: Tone[] = ["services", "presentation", "playback", "lights", "success", "warning", "danger", "info", "idle"];
const liveTones: LiveTone[] = ["go", "back", "reload", "stop"];

/** Real Button per state. Hover/focus/active are driven by the capture tool, never faked with classes. */
function Cell({ state, label, ...props }: ButtonProps & { state: GalleryState; label: string }) {
  if (state === "error") {
    return <p data-gallery-unavailable className="text-sm text-ink-muted">Not applicable: buttons have no error state.</p>;
  }
  return (
    <Button
      data-gallery-target
      {...props}
      disabled={state === "disabled"}
      loading={state === "loading"}
      loadingLabel="Working…"
    >
      {label}
    </Button>
  );
}

const longLabel = "Reload the plan from Planning Center now";
const veryLongLabel = "Disconnect from Planning Center and remove every saved sign-in for this computer";

export function ButtonSection() {
  return (
    <Section name="button" title="Button">
      <StateRow label="primary" render={(s) => <Cell state={s} variant="primary" label="Save configuration" />} />
      <StateRow label="secondary" render={(s) => <Cell state={s} variant="secondary" label="Test connection" />} />
      {tones.map((tone) => (
        <StateRow key={tone} label={`tinted ${tone}`} render={(s) => <Cell state={s} variant="tinted" tone={tone} label={`Scan ${tone}`} />} />
      ))}
      <StateRow label="destructive" render={(s) => <Cell state={s} variant="destructive" label="Delete layout" />} />
      <StateRow label="destructive-soft" render={(s) => <Cell state={s} variant="destructive-soft" label="Discard changes" />} />
      {liveTones.map((liveTone) => (
        <StateRow key={liveTone} label={`live ${liveTone}`} render={(s) => <Cell state={s} variant="live" liveTone={liveTone} label={{ go: "Start next", back: "Previous", reload: "Reload plan", stop: "Stop timer" }[liveTone]} />} />
      ))}
      <StateRow label="touch size" render={(s) => <Cell state={s} variant="secondary" size="touch" label="Close" aria-label="Close" />} />
      <StateRow label="label length short" render={(s) => <Cell state={s} variant="primary" label="Save" />} />
      <StateRow label="label length long" render={(s) => <Cell state={s} variant="secondary" label={longLabel} />} />
      <StateRow label="label length very long" render={(s) => <Cell state={s} variant="tinted" tone="warning" fullWidth label={veryLongLabel} />} />
    </Section>
  );
}
