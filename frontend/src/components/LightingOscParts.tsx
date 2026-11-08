import { useEffect, useId, useState, type ReactNode } from "react";

import type { LightingEffect, OscArgument, OscArgumentType, OscLightingCue, OscMessage } from "../types";
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
import { StatusDot } from "./ui/StatusDot";
import type { Tone } from "./ui/types";
import { CONTROL_40, CUE_ADDRESS_HELP } from "./liveLink";
import { clampCueTime } from "./lightingTimelineMath";
import { clampRampStart, effectLabel, fadeError, fadeOf, formatFade, parseFadeInput, rowSummary, type RowValueIndex } from "./lightingSmooth";
import { EffectDetail } from "./LightingEffectDetail";

const argumentTypes: { value: OscArgumentType; label: string }[] = [
  { value: "int", label: "Whole number" },
  { value: "float", label: "Decimal" },
  { value: "string", label: "Text" },
  { value: "bool", label: "True or false" },
];

import { CueTypeBadge } from "./CueTypeParts";
import { classifyOscCue } from "./lightingCueTypes";

/** The small uppercase caption over a Live Link field. */
export function LiveLinkField({ caption, className, children }: { caption: string; className?: string; children: ReactNode }) {
  return (
    <label className={`flex min-w-0 flex-col gap-0.5 text-[0.6875rem] font-medium uppercase tracking-[0.07em] text-ink-muted ${className ?? ""}`}>
      {caption}
      {children}
    </label>
  );
}

/**
 * THE one place the connection state is written down on the Live Link block: a dot AND words (never colour alone), announced
 * politely when it changes. `detail` (why it is not connected) rides in the tooltip and the accessible name, not in extra paragraphs.
 */
export function LiveLinkPill({ text, tone, detail }: { text: string; tone: Tone; detail?: string | null }) {
  const full = detail ? `sACN DMX Live Link: ${text}. ${detail}` : `sACN DMX Live Link: ${text}`;
  return (
    <span aria-label={full} aria-live="polite" className="live-link-pill inline-flex items-center gap-[7px] whitespace-nowrap rounded-ds-full border border-edge bg-surface-950 px-3 py-[5px] text-[0.8125rem] leading-[1.45] text-ink" data-testid="lightkey-link" data-tone={tone} role="status" title={full}>
      <StatusDot ignoreAccent tone={tone} />
      <span>{text}</span>
    </span>
  );
}

/** The bordered Live Link block: title, the state pill beside it, then whatever the caller lays out. */
export function LiveLinkShell({ pill, className, testId, children }: { pill?: ReactNode; className?: string; testId?: string; children: ReactNode }) {
  return (
    <section aria-label="sACN DMX Live Link" className={`live-link mt-6 rounded-ds-xl border border-edge bg-surface-panel-strong px-3 py-2.5 ${className ?? ""}`} data-testid={testId}>
      <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
        <h3 className="text-base font-semibold text-ink">sACN DMX Live Link</h3>
        {pill}
      </div>
      {children}
    </section>
  );
}

const subButton = "!h-8 !min-h-8 !px-2.5 text-sm coarse:!h-11 coarse:!min-h-[44px]";

function DisclosureButton({ open, controls, onToggle, children }: { open: boolean; controls: string; onToggle: () => void; children: ReactNode }) {
  return (
    <Button aria-controls={controls} aria-expanded={open} className={subButton} onClick={onToggle} variant="secondary">
      {children}
      {/* Conventional state arrow: down when open, right when closed. Decorative; aria-expanded carries the state. */}
      <span aria-hidden="true" className="ml-1.5 text-ink-muted" data-part="arrow">{open ? "\u25BE" : "\u25B8"}</span>
    </Button>
  );
}

/**
 * The quiet strip under the fields: one line of state text, then Release lights, the Send a test cue disclosure and the
 * cue-address help. The test cue and the help open as nested boxes under the strip; both stay in the DOM so the words are always readable.
 */
export function LiveLinkTools({ stateText, release, testCue, footer }: {
  stateText?: ReactNode;
  release?: ReactNode;
  /** The test cue's fields; leave out to draw no disclosure. */
  testCue?: ReactNode;
  /** Notes under the nested boxes (last sent, what is kept while the method is held). */
  footer?: ReactNode;
}) {
  const [cueOpen, setCueOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const cueId = useId();
  const helpId = useId();
  return (
    <>
      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-[0.78125rem] text-ink-muted" data-testid="live-link-sub">
        {stateText}
        {/* The three actions travel together: when the strip is too narrow for them beside the state text they drop under it as one
            group, so "How to find a cue's address" never sits alone on a row (it did at about 820 px). */}
        <div className="flex min-w-0 flex-wrap items-center gap-2" data-testid="live-link-actions">
          {release}
          {testCue && <DisclosureButton controls={cueId} onToggle={() => setCueOpen(!cueOpen)} open={cueOpen}>Send a test cue</DisclosureButton>}
          <DisclosureButton controls={helpId} onToggle={() => setHelpOpen(!helpOpen)} open={helpOpen}>How to find a cue&apos;s address</DisclosureButton>
        </div>
      </div>
      <p className="mt-2 rounded-ds-lg border border-edge-subtle bg-surface-950 px-3 py-2 text-type-body-sm text-ink-body" data-testid="cue-address-help" hidden={!helpOpen} id={helpId}>{CUE_ADDRESS_HELP}</p>
      {testCue && (
        <div className="mt-2 rounded-ds-lg border border-edge bg-surface-900/60 p-3" data-testid="test-cue-box" hidden={!cueOpen} id={cueId}>
          {testCue}
        </div>
      )}
      {footer}
    </>
  );
}

/**
 * The sACN DMX Live Link block when the live link itself is not on screen (Lighting is not saved as OSC yet): the same shared host
 * and OSC UDP port boxes, so the person can type the machine before saving. Once OSC is saved, LightkeyStrip draws the full line.
 */
export function OscConnectionFields({
  host, port, held, attempted, onHost, onPort, testCue, footer,
}: {
  host: string;
  port: string;
  held: { host: boolean; port: boolean };
  attempted: boolean;
  onHost: (host: string) => void;
  onPort: (port: string) => void;
  testCue?: ReactNode;
  footer?: ReactNode;
}) {
  const hostError = attempted ? oscHostError(host) : null;
  const portError = attempted ? oscPortError(port) : null;
  return (
    <LiveLinkShell className="lighting-osc-connection">
      <div className="mt-2 flex flex-wrap items-end gap-x-2.5 gap-y-2" data-testid="live-link-line">
        <LiveLinkField caption="Host or IP (OSC output + Live Link)" className="live-link-host">
          <Input aria-label="Host or IP address" autoCapitalize="none" autoCorrect="off" className={`w-full font-mono ${CONTROL_40} ${held.host ? "!border-warning" : ""}`} error={Boolean(hostError)} onChange={(event) => onHost(event.target.value)} placeholder="192.0.2.10" spellCheck={false} value={host} />
        </LiveLinkField>
        <LiveLinkField caption="OSC UDP">
          <Input aria-label="OSC UDP port" className={`!w-[5.25rem] text-center tabular-nums ${CONTROL_40} ${held.port ? "!border-warning" : ""}`} error={Boolean(portError)} inputMode="numeric" onChange={(event) => onPort(event.target.value)} value={port} />
        </LiveLinkField>
      </div>
      {(hostError || portError) && <p className="mt-1 text-type-body-sm text-danger-text" role="alert">{[hostError, portError].filter(Boolean).join(" ")}</p>}
      <LiveLinkTools footer={footer} testCue={testCue} />
    </LiveLinkShell>
  );
}


/** Typed argument rows for the test cue and for each cue. Edits are reported as drafts. `inline` lays them out beside the other test-cue fields (Live Link block). */
export function OscArgumentRows({
  drafts, onChange, disabled = false, label, inline = false,
}: {
  drafts: OscArgumentDraft[];
  onChange: (drafts: OscArgumentDraft[]) => void;
  disabled?: boolean;
  label: string;
  inline?: boolean;
}) {
  const update = (index: number, change: Partial<OscArgumentDraft>) =>
    onChange(drafts.map((draft, position) => position === index ? { ...draft, ...change } : draft));
  const control = inline ? CONTROL_40 : undefined;
  // Inline: the visible caption is just "Type" / "Value"; the row number stays in the accessible name.
  const caption = (index: number, word: string) => inline ? <><span className="sr-only">{label} {index + 1} </span>{word}</> : `${label} ${index + 1} ${word}`;
  return (
    <div className={inline ? "flex flex-wrap items-end gap-2" : "grid gap-2"}>
      {drafts.map((draft, index) => (
        <div className={inline ? "flex items-end gap-2" : "grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)_auto] items-end gap-2"} key={index}>
          <Field className={inline ? "w-36" : undefined} label={caption(index, "type")}>
            <Select
              className={control}
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
          <Field className={inline ? "w-28" : undefined} label={caption(index, "value")}>
            {draft.type === "bool" ? (
              <Select className={control} disabled={disabled} onChange={(event) => update(index, { text: event.target.value })} value={draft.text}>
                <option value="true">True</option>
                <option value="false">False</option>
              </Select>
            ) : (
              <Input className={inline ? `${control} text-center tabular-nums` : undefined} disabled={disabled} onChange={(event) => update(index, { text: event.target.value })} value={draft.text} />
            )}
          </Field>
          <Button
            aria-label={`Remove ${label.toLowerCase()} ${index + 1}`}
            className={control}
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
        <Button className={control} disabled={disabled || drafts.length >= 16} onClick={() => onChange([...drafts, { type: "int", text: "0" }])} variant="tinted">
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
  cue, disabled, songDuration, selected, onCommit, onRemove, onRemoveFixture, onToggleSelect, highlight = false, allRows,
}: {
  cue: OscLightingCue;
  /** Every row of the song, so a ramp can say what value it starts from. */
  allRows?: OscLightingCue[] | RowValueIndex;
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
  const fade = fadeOf(cue);
  const ramp = fade > 0;
  const [fadeText, setFadeText] = useState(String(fade));
  useEffect(() => setFadeText(String(fadeOf(cue))), [cue.fade_seconds]); // eslint-disable-line react-hooks/exhaustive-deps
  const parsedFade = parseFadeInput(fadeText);
  const fadeProblem = fadeError(parsedFade, cue.at_seconds, songDuration);
  const timeError = parsedTime === null ? "Use mm:ss." : songDuration !== null && parsedTime > songDuration ? "Past the song length." : ramp && songDuration !== null && parsedTime + fade > songDuration + 1e-9 ? "This ramp would run past the end of the song." : null;
  const captured = isCaptured(cue);
  const move = (at: number) => onCommit({ ...cue, at_seconds: ramp ? clampRampStart(clampCueTime(at, songDuration), fade, songDuration) : clampCueTime(at, songDuration) });
  const commitFade = () => {
    if (parsedFade === null || fadeProblem) { setFadeText(String(fadeOf(cue))); return; }
    if (parsedFade !== fadeOf(cue)) {
      const next = { ...cue };
      if (parsedFade > 0) next.fade_seconds = parsedFade; else delete next.fade_seconds;
      onCommit(next);
    }
  };
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
        {ramp && <span className="rounded-ds-sm border border-cue-look-captured/60 px-1.5 font-mono tabular-nums text-ink-body" data-testid="look-fade" title={`A ramp: reaches its values over ${fade.toFixed(1)} s`}>Ramp {formatFade(fade)}</span>}
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
          <Field error={fadeProblem ?? undefined} label="Look duration (seconds, 0 = instant)">
            <Input
              className="w-28 font-mono tabular-nums"
              disabled={disabled}
              inputMode="decimal"
              onBlur={commitFade}
              onChange={(event) => setFadeText(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }}
              value={fadeText}
            />
          </Field>
          {ramp && (
            <ul aria-label="What this ramp does" className="grid gap-0.5 text-type-body-sm text-ink-body" data-testid="ramp-summary">
              {rowSummary(cue, allRows ?? [cue]).map((line, index) => (
                <li className="tabular-nums" key={index}>{line.fixture} {line.property}: {line.ramps ? `${line.from ?? "start"} → ${line.to}` : line.to}</li>
              ))}
            </ul>
          )}
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


/**
 * One loop of the song: a single line (closed) with what it is and when it plays, and Show / Hide for the details. It has no
 * checkbox and no fields: loops are never merged, never edited by the rows, and removed only after a question.
 */
export function EffectRow({ effect, disabled, highlight = false, onRemove }: {
  effect: LightingEffect;
  disabled: boolean;
  highlight?: boolean;
  onRemove: () => void;
}) {
  const [open, setOpen] = useState(false);
  const text = effectLabel(effect);
  const from = formatElapsed(effect.start_seconds);
  return (
    <div
      className={`lighting-effect-row grid gap-2 border-t border-edge-subtle pt-3 first:border-t-0 first:pt-0 ${highlight ? "rounded-ds-md bg-surface-panel" : ""}`}
      data-source={effect.source === "captured" ? "captured" : "manual"}
      data-testid="effect-row"
      id={`look-row-${effect.id}`}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Button aria-expanded={open} aria-label={`${open ? "Hide" : "Show"} loop ${text} at ${from}`} onClick={() => setOpen(!open)} variant="tinted">{open ? "Hide" : "Show"}</Button>
        <CueTypeBadge type="effect-loop" />
        <span className="font-semibold text-ink">{text}</span>
        <span className="font-mono tabular-nums text-ink-body">{from} to {formatElapsed(effect.end_seconds)}</span>
        <span className="rounded-ds-full border border-edge-medium px-2 text-type-body-sm text-ink-muted" data-testid="effect-badge">{effect.source === "captured" ? "Captured loop" : "Loop"}</span>
        <span className="ml-auto flex gap-1">
          <Button aria-label={`Remove loop ${text} at ${from}`} disabled={disabled} onClick={onRemove} tone="danger" variant="tinted">Remove...</Button>
        </span>
      </div>
      {open && <EffectDetail className="pl-2" effect={effect} />}
    </div>
  );
}
