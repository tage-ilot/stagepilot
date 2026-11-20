/** One row of the Event stream: the time, then the event in plain words; the raw `type · source` is the hover text, for support. */
export function EventRow({ time, text, title }: { time: string; text: string; title?: string }) {
  return (
    <div className="event-row grid grid-cols-[4.5rem_1fr] gap-3 border-b border-edge-subtle px-4 py-2.5 text-xs" title={title}>
      <time className="font-mono tabular-nums text-idle">{time}</time>
      <p className="truncate font-semibold text-ink-body">{text}</p>
    </div>
  );
}
