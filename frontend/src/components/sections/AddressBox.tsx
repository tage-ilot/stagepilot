import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";

import { rememberAddress, useRecentAddresses, type RecentScope } from "../../hooks/useRecentAddresses";
import { CONTROL_40 } from "../liveLink";
import { cx } from "../ui/cx";
import { Input } from "../ui/Input";
import { addressGroups, sameAddress, type AddressOption, type ScannedAddress } from "./addressOptions";

export const NO_ADDRESSES = "No recent or scanned addresses yet.";

export interface AddressBoxProps {
  value: string;
  onChange: (value: string) => void;
  /** Which block's addresses the Recent group remembers. */
  scope: RecentScope;
  /** The computers the last scan found (see `scannedAddresses`); empty while there is no recent scan. */
  scanned?: readonly ScannedAddress[];
  /** An address that is connected right now: it is remembered, so it is in Recent next time. */
  working?: string | null;
  /** Told when an entry is chosen, after `onChange` put its address in the box (a scanned computer's port, for ProPresenter). Choosing never connects. */
  onPick?: (option: { host: string; port?: number; group: "scanned" | "recent" }) => void;
  /** Enter in the box while the list is closed: the block's Connect. */
  onSubmit?: () => void;
  disabled?: boolean;
  error?: boolean;
  /** A held change: the warning border (the words "Not saved yet" belong to the field's caption). */
  held?: boolean;
  placeholder?: string;
  title?: string;
  /** The held-change marker's and the error text's ids. */
  "aria-describedby"?: string;
  "aria-label": string;
  className?: string;
  /** The text box itself, for a block that must put the cursor there (the scan's "Enter address instead"). */
  inputRef?: Ref<HTMLInputElement>;
  /** Draws the list open at first. For the gallery and tests; the person opens it with the arrow. */
  defaultOpen?: boolean;
  /** Replaces what `useRecentAddresses(scope)` reads. For the gallery and tests. */
  recent?: readonly string[];
}

function Check() {
  return (
    <svg aria-hidden="true" className="size-4 shrink-0 text-ink" data-part="check" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" viewBox="0 0 16 16">
      <path d="M3 8.5l3.2 3.2L13 4.8" />
    </svg>
  );
}

function Chevron() {
  return (
    <svg aria-hidden="true" className="size-4" data-part="arrow" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.75" viewBox="0 0 16 16">
      <path d="M4 6.5l4 4 4-4" />
    </svg>
  );
}

/**
 * THE address box of every Live Link block (Lighting, Playback, ProPresenter): an ordinary text box for the computer's address, with a small
 * arrow inside its right edge that opens the list of the addresses that worked (Recent) and the computers the last scan found (Scanned).
 *
 * The text is ALWAYS editable: typing, pasting, editing the address in it and replacing it all work whether or not the list is open. Choosing
 * an entry only puts its address in the box; it never connects (Connect does). The list is the standard combobox: the box has
 * `role="combobox"` with `aria-expanded`, `aria-controls` and `aria-activedescendant`; the list is a `listbox` of `option`s in two named groups.
 * It opens with the arrow, ArrowDown or Alt+ArrowDown; with it open the arrows move through the options, Enter chooses the active one, and typing
 * edits the box (and closes the list, so the next Enter is Connect). It closes on Escape, on choosing, on a click outside and when focus leaves; it never traps focus
 * (Tab goes on to the next control). The arrow is not a tab stop: the box is, and ArrowDown there is the keyboard's way in.
 */
export function AddressBox({
  value, onChange, scope, scanned = [], working, onPick, onSubmit, disabled = false, error = false, held = false, placeholder, title,
  "aria-describedby": describedBy, "aria-label": label, className, defaultOpen = false, recent, inputRef,
}: AddressBoxProps) {
  const listId = useId();
  const stored = useRecentAddresses(scope);
  const groups = useMemo(() => addressGroups(scanned, recent ?? stored), [scanned, recent, stored]);
  const options = useMemo<AddressOption[]>(() => [...groups.scanned, ...groups.recent], [groups]);
  const [opened, setOpened] = useState(defaultOpen);
  const [activeAt, setActiveAt] = useState(-1);
  const root = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const open = opened && !disabled;
  const active = open && activeAt < options.length ? activeAt : -1;
  const currentAt = options.findIndex((option) => sameAddress(option.host, value));
  const optionId = (index: number) => `${listId}-option-${index}`;

  useEffect(() => { rememberAddress(scope, working); }, [scope, working]);

  const close = () => { setOpened(false); setActiveAt(-1); };
  const show = (at: number) => { if (disabled) return; setOpened(true); setActiveAt(at); };
  const choose = (option: AddressOption) => {
    onChange(option.host);
    onPick?.({ host: option.host, port: option.port, group: option.group });
    close();
    field.current?.focus();
  };

  // A click or tap outside the box closes the list (focus leaving it does too, below).
  useEffect(() => {
    if (!open) return undefined;
    const outside = (event: Event) => { if (!root.current?.contains(event.target as Node)) close(); };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  useEffect(() => {
    if (active >= 0) document.getElementById(optionId(active))?.scrollIntoView?.({ block: "nearest" });
    // optionId is derived from listId.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  const move = (step: number) => {
    if (!options.length) return;
    setActiveAt(active < 0 ? (step > 0 ? 0 : options.length - 1) : (active + step + options.length) % options.length);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (!open) show(event.altKey || !options.length ? -1 : Math.max(currentAt, 0));
      else move(1);
    } else if (event.key === "ArrowUp" && open) {
      event.preventDefault();
      move(-1);
    } else if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === "Enter") {
      if (open) {
        event.preventDefault();
        const option = options[active];
        if (option) choose(option); else close();
      } else if (onSubmit) {
        event.preventDefault();
        onSubmit();
      }
    }
  };

  const row = (option: AddressOption, index: number): ReactNode => {
    const current = sameAddress(option.host, value);
    return (
      <div
        aria-selected={index === active}
        className={cx(
          "flex min-h-9 cursor-pointer items-center gap-2 rounded-ds-md px-2 py-1.5 font-mono text-type-body-sm text-ink-body coarse:min-h-11 [overflow-wrap:anywhere]",
          index === active ? "bg-edge-subtle text-ink ring-1 ring-inset ring-ink-muted" : "can-hover:bg-edge-subtle",
        )}
        data-current={current || undefined}
        id={optionId(index)}
        key={`${option.group}:${option.host}`}
        onClick={() => choose(option)}
        onMouseDown={(event) => event.preventDefault()}
        role="option"
      >
        <span className="min-w-0 flex-1">{option.text}</span>
        {current && <><Check /><span className="sr-only">(in the box now)</span></>}
      </div>
    );
  };
  const group = (name: string, id: string, items: AddressOption[], offset: number) => items.length > 0 && (
    <div aria-labelledby={`${listId}-${id}`} role="group">
      <div className="px-2 pb-0.5 pt-1.5 text-[0.6875rem] font-medium uppercase !tracking-[0.07em] text-ink-muted" id={`${listId}-${id}`}>{name}</div>
      {items.map((option, index) => row(option, offset + index))}
    </div>
  );

  return (
    <div className={cx("relative w-full min-w-0", className)} data-part="address-box" ref={root}>
      <Input
        aria-activedescendant={active >= 0 ? optionId(active) : undefined}
        aria-autocomplete="none"
        aria-controls={listId}
        aria-describedby={describedBy}
        aria-expanded={open}
        aria-label={label}
        autoCapitalize="none"
        autoComplete="off"
        autoCorrect="off"
        className={cx("w-full font-mono", CONTROL_40, "!pr-8 coarse:!pr-11", held && "!border-warning")}
        disabled={disabled}
        error={error}
        onBlur={(event) => { if (!root.current?.contains(event.relatedTarget as Node | null)) close(); }}
        onChange={(event) => { onChange(event.target.value); if (open) close(); }}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        ref={(node) => {
          field.current = node;
          if (typeof inputRef === "function") inputRef(node); else if (inputRef) (inputRef as { current: HTMLInputElement | null }).current = node;
        }}
        role="combobox"
        spellCheck={false}
        title={title}
        value={value}
      />
      <button
        aria-controls={listId}
        aria-expanded={open}
        aria-label="Show recent and scanned addresses"
        className="absolute inset-y-0 right-0 flex w-8 touch-manipulation items-center justify-center rounded-r-ds-lg text-ink-muted disabled:cursor-not-allowed disabled:opacity-40 can-hover:text-ink coarse:w-11"
        data-part="address-arrow"
        disabled={disabled}
        onClick={() => { if (open) { close(); return; } field.current?.focus(); show(Math.max(currentAt, -1)); }}
        onMouseDown={(event) => event.preventDefault()}
        tabIndex={-1}
        type="button"
      >
        <Chevron />
      </button>
      {open && (
        <div
          className="absolute left-0 right-0 top-full z-20 mt-1 max-h-60 text-left font-normal normal-case tracking-normal overflow-y-auto overscroll-contain rounded-ds-lg border border-edge-medium bg-surface-950 p-1 shadow-elevation-overlay"
          data-part="address-list"
          onMouseDown={(event) => event.preventDefault()}
        >
          {options.length > 0 ? (
            <div aria-label="Recent and scanned addresses" id={listId} role="listbox">
              {group("Scanned", "scanned", groups.scanned, 0)}
              {group("Recent", "recent", groups.recent, groups.scanned.length)}
            </div>
          ) : (
            <p className="px-2 py-2 text-type-body-sm text-ink-muted" id={listId}>{NO_ADDRESSES}</p>
          )}
        </div>
      )}
    </div>
  );
}
