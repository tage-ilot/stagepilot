import type { ReactNode } from "react";

import { Field } from "../Field";
import { Input } from "../Input";
import { Select } from "../Select";
import { Textarea } from "../Textarea";
import { Section, StateRow } from "./Section";
import type { GalleryState } from "./states";

const longLabel = "Name shown on the stage display for the second service of the morning";
const longOption = "Planning Center service plan for the combined Sunday morning gathering with the visiting choir";

/** Real primitives per state. Hover/focus/active are driven by the capture tool, never faked with classes. */
function InputCell({ state, label, held = false }: { state: GalleryState; label: string; held?: boolean }) {
  const error = state === "error";
  return (
    <Field label={label} hint="Shown on the output screen." error={error ? "Enter a name of 1 to 40 characters" : undefined} heldChange={held}>
      <Input data-gallery-target defaultValue="Sunday AM" placeholder="Service name" disabled={state === "disabled"} />
    </Field>
  );
}

function SelectCell({ state, label, option = "Main hall" }: { state: GalleryState; label: string; option?: string }) {
  return (
    <Field label={label} error={state === "error" ? "Choose an output" : undefined}>
      <Select
        data-gallery-target
        defaultValue={state === "error" ? "" : "a"}
        placeholder="Choose an output"
        disabled={state === "disabled"}
        loading={state === "loading"}
        loadingLabel="Loading…"
      >
        <option value="a">{option}</option>
        <option value="b">Side room</option>
      </Select>
    </Field>
  );
}

function TextareaCell({ state, label }: { state: GalleryState; label: string }) {
  return (
    <Field label={label} error={state === "error" ? "Notes must be under 500 characters" : undefined}>
      <Textarea data-gallery-target defaultValue="Doors open at nine." disabled={state === "disabled"} />
    </Field>
  );
}

const noLoading = (state: GalleryState, ui: ReactNode) =>
  state === "loading" ? <p data-gallery-unavailable className="text-sm text-ink-muted">Not applicable: text fields have no loading state.</p> : ui;

export function FieldSection() {
  return (
    <Section name="field" title="Field, Input, Select, Textarea">
      <StateRow label="input" render={(s) => noLoading(s, <InputCell state={s} label="Service name" />)} />
      <StateRow label="input held change" render={(s) => noLoading(s, <InputCell state={s} label="Service name" held />)} />
      <StateRow label="input long label" render={(s) => noLoading(s, <InputCell state={s} label={longLabel} />)} />
      <StateRow label="select" render={(s) => <SelectCell state={s} label="Lighting output" />} />
      <StateRow label="select long option" render={(s) => <SelectCell state={s} label="Plan" option={longOption} />} />
      <StateRow label="textarea" render={(s) => noLoading(s, <TextareaCell state={s} label="Notes" />)} />
    </Section>
  );
}
