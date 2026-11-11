import { useLayoutEffect, useRef, useState, type HTMLAttributes, type ReactNode, type Ref } from "react";

export interface HeaderChipNotice {
  id: number;
  message: string;
  tone: "info" | "error";
}

interface HeaderStatusChipProps {
  /** Classes for the pill while it shows the system status (its colours). */
  statusClass: string;
  statusLabel: string;
  notice: HeaderChipNotice | null;
  title?: string;
  expanded: boolean;
  wrapRef: Ref<HTMLDivElement>;
  wrapProps: HTMLAttributes<HTMLDivElement>;
  /** The readiness tooltip, which hangs under the pill. */
  popover: ReactNode;
}

const PADDING_X = 26; // pane padding (0.75rem each side) plus the 1px border on both sides
const PADDING_Y = 14; // pane padding (0.375rem each side) plus the border

interface Measured {
  statusW: number;
  statusH: number;
  /** The message on one line, untruncated. */
  noteW: number;
  noteH: number;
  /** The slot beside the logo. */
  avail: number;
  /** The whole header row: the most a message may ever grow to, so it never passes the left margin. */
  rowW: number;
  /** A compact window lets a message grow over the logo; a wide one keeps it in the slot. */
  compact: boolean;
}

// The narrowest the status pill may be cut to before it steps aside: the width of the "Ready" pill. A longer status (an error
// cause, a song to record again) is cut with "..." to the room it has instead of vanishing, because a problem must stay visible.
const MIN_STATUS_W = 66;

// The window width below which the page switches to its tighter padding (Tailwind `lg`).
const COMPACT_QUERY = "(max-width: 1023.98px)";

const NOTICE_CLASS = {
  // The pill is always the widgets' dark surface; the message's colour is carried by its text and border. Info is the blue of the
  // Previous button (live-back), errors are the error red.
  info: "border-live-back-soft text-live-back-text",
  error: "border-danger-edge text-danger-pill",
} as const;

/**
 * The header's one status element. It is the status pill; when a message arrives the same pill grows to the message's size,
 * takes the message's colours and swaps its words (and shrinks back when the message ends).
 *
 * A message is always ONE line (owner, 2026-10-16): when it is longer than the room it has, it ends in "...". On a compact
 * window it may grow leftward over the logo, because messages are short-lived, but never past the header's left edge, which
 * is the same margin the header keeps on its right. If the slot beside the logo is too small for even the status pill, the
 * status pill steps aside, but a message still shows, right-aligned, over the logo.
 */
export function HeaderStatusChip({ statusClass, statusLabel, notice, title, expanded, wrapRef, wrapProps, popover }: HeaderStatusChipProps) {
  const slotRef = useRef<HTMLDivElement>(null);
  const statusProbe = useRef<HTMLSpanElement>(null);
  const noteProbe = useRef<HTMLSpanElement>(null);
  const [m, setM] = useState<Measured | null>(null);
  // Keep showing the last message's words while the pill shrinks back, so the text does not blank out mid-animation.
  const [lastNotice, setLastNotice] = useState<HeaderChipNotice | null>(notice);
  if (notice && notice !== lastNotice) setLastNotice(notice);
  const shown = notice ?? lastNotice;

  useLayoutEffect(() => {
    const measure = () => {
      const slot = slotRef.current;
      const status = statusProbe.current;
      const note = noteProbe.current;
      if (!slot || !status || !note) return;
      const avail = slot.clientWidth;
      const rowW = slot.parentElement?.clientWidth ?? avail;
      const compact = typeof window.matchMedia === "function" && window.matchMedia(COMPACT_QUERY).matches;
      const next: Measured = {
        statusW: Math.ceil(status.getBoundingClientRect().width) + 1 + PADDING_X,
        statusH: Math.ceil(status.getBoundingClientRect().height) + PADDING_Y,
        noteW: Math.ceil(note.getBoundingClientRect().width) + 1 + PADDING_X, // +1: text widths have fractions, and a fraction too few would cut the last letter
        noteH: Math.ceil(note.getBoundingClientRect().height) + PADDING_Y,
        avail,
        rowW,
        compact,
      };
      setM((prev) => prev && Object.keys(next).every((k) => prev[k as keyof Measured] === next[k as keyof Measured]) ? prev : next);
    };
    measure();
    // The pixel font loads after first paint, which changes every width: measure again when it arrives.
    let alive = true;
    void document.fonts?.ready.then(() => { if (alive) measure(); });
    if (typeof ResizeObserver === "undefined" || !slotRef.current) return () => { alive = false; };
    const observer = new ResizeObserver(measure);
    observer.observe(slotRef.current);
    if (slotRef.current.parentElement) observer.observe(slotRef.current.parentElement);
    if (statusProbe.current) observer.observe(statusProbe.current);
    if (noteProbe.current) observer.observe(noteProbe.current);
    return () => { alive = false; observer.disconnect(); };
  }, [statusLabel, shown?.message]);

  // avail of 0 means the slot has not been laid out (a test renderer, or a hidden tab): assume it fits.
  const laidOut = m !== null && m.avail > 0;
  const fits = !laidOut || m.avail >= m.statusW;
  // The status pill shows (cut with "..." when its words are longer than the slot) unless even the shortest pill would not fit.
  const showsStatus = !laidOut || m.avail >= Math.min(m.statusW, MIN_STATUS_W);
  // A message always shows, even when the slot is too small for the status pill: it then sits over the logo.
  const messageInChip = Boolean(notice);
  // The most room a message has: the slot while the window is wide, the whole header row (logo included) when it is not.
  const room = m === null ? 0 : fits && !m.compact ? m.avail : Math.max(m.rowW, m.avail);
  const width = m === null ? undefined : messageInChip ? Math.min(m.noteW, room) : laidOut ? Math.min(m.statusW, m.avail) : m.statusW;
  const height = m === null ? undefined : messageInChip ? m.noteH : m.statusH;
  const noteTone = (shown ?? { tone: "info" as const }).tone;

  return (
    <div className="app-header-status relative -top-[3px] flex h-9 min-w-0 flex-1 items-center justify-end" ref={slotRef}>
      <div aria-atomic="true" aria-live="polite" className="sr-only" role="status">{notice?.message ?? ""}</div>
      <div className="header-chip-wrap relative z-40 shrink-0" data-fits={showsStatus ? "true" : "false"} data-notice={notice ? "true" : "false"} ref={wrapRef} {...wrapProps}>
        <button
          aria-describedby="system-readiness-popover"
          aria-expanded={expanded}
          className={`header-chip relative block cursor-help touch-manipulation overflow-hidden border bg-surface-950 font-pixel !text-type-badge uppercase tracking-wider text-shadow-label coarse:min-h-ds-touch-target ${messageInChip ? NOTICE_CLASS[noteTone] : statusClass}`}
          data-state={messageInChip ? "message" : "status"}
          data-testid="header-chip"
          style={width === undefined ? undefined : { width, height }}
          title={title}
          type="button"
        >
          <span className="header-chip-pane header-chip-status">{statusLabel}</span>
          <span aria-hidden="true" className="header-chip-pane header-chip-note" data-testid="header-message" data-tone={notice?.tone ?? "none"}><span className="header-chip-note-text" key={shown?.id}>{shown?.message ?? ""}</span></span>
        </button>
        {popover}
      </div>
      {/* Measuring copies: never visible, same type as the real ones. */}
      <span aria-hidden="true" className="header-chip-probe font-pixel !text-type-badge uppercase tracking-wider">
        <span className="header-chip-probe-status" data-text={statusLabel} ref={statusProbe} />
        <span className="header-chip-probe-note" data-text={shown?.message ?? ""} ref={noteProbe} />
      </span>
    </div>
  );
}
