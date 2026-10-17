import { useState } from "react";

import type { OscArgument, OscArgumentType, OscLightingCue } from "../types";
import {
  argumentToDraft,
  formatElapsed,
  oscAddressError,
  oscHostError,
  oscPortError,
  parseElapsed,
  parseOscArguments,
  type OscArgumentDraft,
} from "./lightingOsc";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";
import { Select } from "./ui/Select";

const argumentTypes: { value: OscArgumentType; label: string }[] = [
  { value: "int", label: "Whole number" },
  { value: "float", label: "Decimal" },
  { value: "string", label: "Text" },
  { value: "bool", label: "True or false" },
];

/** Host and UDP port. The port has no default to guess at: each app listens on its own, so it is typed in. */
export function OscConnectionFields({
  host, port, held, attempted, onHost, onPort,
}: {
  host: string;
  port: string;
  held: { host: boolean; port: boolean };
  attempted: boolean;
  onHost: (host: string) => void;
  onPort: (port: string) => void;
}) {
  return (
    <div className="lighting-osc-connection mt-4">
      <div className="lighting-osc-fields grid gap-4">
        <Field error={attempted ? oscHostError(host) ?? undefined : undefined} heldChange={held.host} label="Host or IP address">
          <Input autoCapitalize="none" autoCorrect="off" onChange={(event) => onHost(event.target.value)} spellCheck={false} value={host} />
        </Field>
        <Field error={attempted ? oscPortError(port) ?? undefined : undefined} heldChange={held.port} label="UDP port">
          <Input className="tabular-nums" inputMode="numeric" onChange={(event) => onPort(event.target.value)} value={port} />
        </Field>
      </div>
      <p className="mt-2 text-type-body-sm text-ink-muted">
        Enter the port your lighting app listens for OSC on. In Lightkey, control-click a cue and choose External Control, then Copy OSC Address, to get a cue's address.
      </p>
    </div>
  );
}


/** Typed argument rows for the test cue and for each cue. Edits are reported as drafts. */
export function OscArgumentRows({
  drafts, onChange, disabled = false, label,
}: {
  drafts: OscArgumentDraft[];
  onChange: (drafts: OscArgumentDraft[]) => void;
  disabled?: boolean;
  label: string;
}) {
  const update = (index: number, change: Partial<OscArgumentDraft>) =>
    onChange(drafts.map((draft, position) => position === index ? { ...draft, ...change } : draft));
  return (
    <div className="grid gap-2">
      {drafts.map((draft, index) => (
        <div className="grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)_auto] items-end gap-2" key={index}>
          <Field label={`${label} ${index + 1} type`}>
            <Select
              disabled={disabled}
              onChange={(event) => {
                const type = event.target.value as OscArgumentType;
                update(index, { type, text: type === "bool" ? "true" : type === "string" ? "" : "0" });
              }}
              value={draft.type}
            >
              {argumentTypes.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}
            </Select>
          </Field>
          <Field label={`${label} ${index + 1} value`}>
            {draft.type === "bool" ? (
              <Select disabled={disabled} onChange={(event) => update(index, { text: event.target.value })} value={draft.text}>
                <option value="true">True</option>
                <option value="false">False</option>
              </Select>
            ) : (
              <Input disabled={disabled} onChange={(event) => update(index, { text: event.target.value })} value={draft.text} />
            )}
          </Field>
          <Button
            aria-label={`Remove ${label.toLowerCase()} ${index + 1}`}
            disabled={disabled}
            onClick={() => onChange(drafts.filter((_, position) => position !== index))}
            tone="danger"
            variant="tinted"
          >
            Remove
          </Button>
        </div>
      ))}
      <div>
        <Button disabled={disabled || drafts.length >= 16} onClick={() => onChange([...drafts, { type: "int", text: "0" }])} variant="tinted">
          Add argument
        </Button>
      </div>
    </div>
  );
}

/** One OSC cue. Edits stay local until every field is valid, then the cue is saved. */
export function OscCueRow({
  cue, disabled, canTest, testReasonId, testDisabledReason, songDuration, onCommit, onRemove, onTest,
}: {
  cue: OscLightingCue;
  disabled: boolean;
  canTest: boolean;
  testReasonId?: string;
  testDisabledReason?: boolean;
  songDuration: number | null;
  onCommit: (cue: OscLightingCue) => void;
  onRemove: () => void;
  onTest: (address: string, args: OscArgument[]) => void;
}) {
  const [time, setTime] = useState(formatElapsed(cue.at_seconds));
  const [address, setAddress] = useState(cue.address);
  const [label, setLabel] = useState(cue.label);
  const [drafts, setDrafts] = useState<OscArgumentDraft[]>(cue.args.map(argumentToDraft));
  const [touched, setTouched] = useState(cue.address === "");

  const parsedTime = parseElapsed(time);
  const timeError = parsedTime === null
    ? "Use mm:ss."
    : songDuration !== null && parsedTime > songDuration ? "Past the song length." : null;
  const addressError = oscAddressError(address);
  const parsedArgs = parseOscArguments(drafts);
  const error = timeError ?? addressError ?? parsedArgs.error;

  const commit = (next: { time?: string; address?: string; label?: string; drafts?: OscArgumentDraft[] }) => {
    setTouched(true);
    const nextTime = parseElapsed(next.time ?? time);
    const nextAddress = (next.address ?? address).trim();
    const nextArgs = parseOscArguments(next.drafts ?? drafts);
    if (nextTime === null || (songDuration !== null && nextTime > songDuration)) return;
    if (oscAddressError(nextAddress) || nextArgs.error) return;
    onCommit({ ...cue, at_seconds: nextTime, address: nextAddress, label: (next.label ?? label).trim(), args: nextArgs.args });
  };

  const testReason = testDisabledReason ? testReasonId : undefined;
  return (
    <div className="lighting-osc-cue grid gap-2 border-t border-edge-subtle pt-3 first:border-t-0 first:pt-0" data-testid="osc-cue-row">
      <div className="lighting-osc-cue-fields grid gap-2">
        <Field error={touched ? timeError ?? undefined : undefined} label="Cue elapsed time">
          <Input
            className="font-mono tabular-nums"
            disabled={disabled}
            onBlur={() => commit({})}
            onChange={(event) => setTime(event.target.value)}
            value={time}
          />
        </Field>
        <Field label="Lighting cue label">
          <Input disabled={disabled} maxLength={120} onBlur={() => commit({})} onChange={(event) => setLabel(event.target.value)} value={label} />
        </Field>
        <Field className="lighting-osc-address" error={touched ? addressError ?? undefined : undefined} label="OSC address">
          <Input
            autoCapitalize="none"
            autoCorrect="off"
            className="font-mono"
            disabled={disabled}
            onBlur={() => commit({})}
            onChange={(event) => setAddress(event.target.value)}
            spellCheck={false}
            value={address}
          />
        </Field>
      </div>
      <OscArgumentRows
        disabled={disabled}
        drafts={drafts}
        label="Argument"
        onChange={(next) => { setDrafts(next); commit({ drafts: next }); }}
      />
      {touched && parsedArgs.error && <p className="text-type-body-sm text-danger-text" role="alert">{parsedArgs.error}</p>}
      <div className="flex flex-wrap gap-2">
        <Button
          aria-describedby={testReason}
          disabled={!canTest || disabled || Boolean(error)}
          onClick={() => onTest(address.trim(), parsedArgs.args)}
          variant="tinted"
        >
          Test
        </Button>
        <Button aria-label={`Remove cue at ${formatElapsed(cue.at_seconds)}`} disabled={disabled} onClick={onRemove} tone="danger" variant="tinted">
          Remove
        </Button>
      </div>
    </div>
  );
}
