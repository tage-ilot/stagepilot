import { useEffect, useState } from "react";

import type { OscArgument, OscArgumentType, OscLightingCue, OscMessage } from "../types";
import { groupByFixture, isCaptured, lookLabel } from "./lightingLooks";
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
import { clampCueTime } from "./lightingTimelineMath";

const argumentTypes: { value: OscArgumentType; label: string }[] = [
  { value: "int", label: "Whole number" },
  { value: "float", label: "Decimal" },
  { value: "string", label: "Text" },
  { value: "bool", label: "True or false" },
];

/** Host and UDP port. The port has no default to guess at: each app listens on its own, so it is typed in. */
import { CueTypeBadge } from "./CueTypeParts";
import { classifyOscCue } from "./lightingCueTypes";

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
      <CueTypeBadge type={classifyOscCue(cue)} />
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

/** One message of a look. Edits stay local until the address and every value are valid, then the row is saved. */
function LookMessageEditor({
  message, disabled, onCommit, onRemove,
}: {
  message: OscMessage;
  disabled: boolean;
  onCommit: (message: OscMessage) => void;
  onRemove: () => void;
}) {
  const [address, setAddress] = useState(message.address);
  const [drafts, setDrafts] = useState<OscArgumentDraft[]>(message.args.map(argumentToDraft));
  const [touched, setTouched] = useState(message.address === "");
  const addressError = oscAddressError(address);
  const parsed = parseOscArguments(drafts);
  const commit = (nextAddress: string, nextDrafts: OscArgumentDraft[]) => {
    setTouched(true);
    const args = parseOscArguments(nextDrafts);
    if (oscAddressError(nextAddress.trim()) || args.error) return;
    if (nextAddress.trim() === message.address && JSON.stringify(args.args) === JSON.stringify(message.args)) return;
    onCommit({ address: nextAddress.trim(), args: args.args });
  };
  return (
    <div className="lighting-look-message grid gap-2 border-t border-edge-subtle pt-2 first:border-t-0 first:pt-0" data-testid="look-message">
      <Field error={touched ? addressError ?? undefined : undefined} label="OSC address">
        <Input
          autoCapitalize="none"
          autoCorrect="off"
          className="font-mono"
          disabled={disabled}
          onBlur={() => commit(address, drafts)}
          onChange={(event) => setAddress(event.target.value)}
          spellCheck={false}
          value={address}
        />
      </Field>
      <OscArgumentRows
        disabled={disabled}
        drafts={drafts}
        label="Value"
        onChange={(next) => { setDrafts(next); commit(address, next); }}
      />
      {touched && parsed.error && <p className="text-type-body-sm text-danger-text" role="alert">{parsed.error}</p>}
      <div>
        <Button aria-label={`Delete message ${message.address || "(new)"}`} disabled={disabled} onClick={onRemove} tone="danger" variant="tinted">Delete message</Button>
      </div>
    </div>
  );
}

/**
 * A look row: "Look - N fixtures" and a time. Closed it is one line (cheap enough for 400 of them); open it lists the
 * messages by fixture, each editable and deletable, with Add message and Remove fixture from this song.
 */
export function LookRow({
  cue, disabled, songDuration, selected, onCommit, onRemove, onRemoveFixture, onToggleSelect, highlight = false,
}: {
  cue: OscLightingCue;
  disabled: boolean;
  songDuration: number | null;
  selected: boolean;
  highlight?: boolean;
  onCommit: (cue: OscLightingCue) => void;
  onRemove: () => void;
  onRemoveFixture: (fixture: string) => void;
  onToggleSelect: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [time, setTime] = useState(formatElapsed(cue.at_seconds));
  useEffect(() => setTime(formatElapsed(cue.at_seconds)), [cue.at_seconds]);
  const messages = cue.messages ?? [];
  const parsedTime = parseElapsed(time);
  const timeError = parsedTime === null ? "Use mm:ss." : songDuration !== null && parsedTime > songDuration ? "Past the song length." : null;
  const captured = isCaptured(cue);
  const move = (at: number) => onCommit({ ...cue, at_seconds: clampCueTime(at, songDuration) });
  const setMessage = (index: number, message: OscMessage) =>
    onCommit({ ...cue, messages: messages.map((item, position) => (position === index ? message : item)) });
  const label = lookLabel(cue);
  const at = formatElapsed(cue.at_seconds);
  return (
    <div
      className={`lighting-look-row grid gap-2 border-t border-edge-subtle pt-3 first:border-t-0 first:pt-0 ${highlight ? "rounded-ds-md bg-surface-panel" : ""}`}
      data-source={captured ? "captured" : "manual"}
      data-testid="look-row"
      id={`look-row-${cue.id}`}
    >
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex min-h-[44px] items-center gap-2 text-type-body-sm text-ink-muted">
          <input aria-label={`Select look at ${at}`} checked={selected} className="h-5 w-5" disabled={disabled} onChange={onToggleSelect} type="checkbox" />
        </label>
        <Button aria-expanded={open} aria-label={`${open ? "Collapse" : "Expand"} look at ${at}`} onClick={() => setOpen(!open)} variant="tinted">{open ? "Hide" : "Edit"}</Button>
        <CueTypeBadge type={classifyOscCue(cue)} />
        <span className="font-semibold text-ink">{label}</span>
        <span className="font-mono tabular-nums text-ink-body">{at}</span>
        <span className={`rounded-ds-full border px-2 text-type-body-sm ${captured ? "border-integration-lighting-osc-soft/60 bg-integration-lighting-osc-soft/15 text-ink-body" : "border-edge-medium text-ink-muted"}`} data-testid="look-badge">
          {captured ? (cue.edited ? "Captured · edited" : "Captured") : "Manual"}
        </span>
        <span className="ml-auto flex gap-1">
          <Button aria-label={`Move look at ${at} 0.1 s earlier`} disabled={disabled || cue.at_seconds <= 0} onClick={() => move(cue.at_seconds - 0.1)} variant="tinted">−0.1 s</Button>
          <Button aria-label={`Move look at ${at} 0.1 s later`} disabled={disabled} onClick={() => move(cue.at_seconds + 0.1)} variant="tinted">+0.1 s</Button>
          <Button aria-label={`Remove look at ${at}`} disabled={disabled} onClick={onRemove} tone="danger" variant="tinted">Remove</Button>
        </span>
      </div>
      {open && (
        <div className="grid gap-3 pl-2" data-testid="look-detail">
          <Field error={timeError ?? undefined} label="Look elapsed time">
            <Input
              className="w-28 font-mono tabular-nums"
              disabled={disabled}
              onBlur={() => { if (parsedTime !== null && !timeError && parsedTime !== cue.at_seconds) move(parsedTime); else setTime(formatElapsed(cue.at_seconds)); }}
              onChange={(event) => setTime(event.target.value)}
              value={time}
            />
          </Field>
          {groupByFixture(messages).map((group) => (
            <section aria-label={`Fixture ${group.fixture}`} className="grid gap-2 rounded-ds-md border border-edge-subtle p-2" key={group.fixture}>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h4 className="font-semibold text-ink">{group.fixture}</h4>
                <Button aria-label={`Remove fixture ${group.fixture} from this song`} disabled={disabled} onClick={() => onRemoveFixture(group.fixture)} tone="danger" variant="tinted">Remove fixture from this song</Button>
              </div>
              {group.items.map(({ index, message }) => (
                <LookMessageEditor
                  disabled={disabled}
                  key={`${index}-${messages.length}`}
                  message={message}
                  onCommit={(next) => setMessage(index, next)}
                  onRemove={() => onCommit({ ...cue, messages: messages.filter((_, position) => position !== index) })}
                />
              ))}
            </section>
          ))}
          <div>
            <Button disabled={disabled || messages.length >= 256} onClick={() => onCommit({ ...cue, messages: [...messages, { address: "", args: [{ type: "float", value: 0 }] }] })} variant="tinted">Add message</Button>
          </div>
        </div>
      )}
    </div>
  );
}
