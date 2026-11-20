import { StatusDot } from "../ui/StatusDot";
import type { Tone } from "../ui/types";

/**
 * THE one place the connection state is written down on the Live Link block: a dot AND words (never colour alone), announced
 * politely when it changes. `detail` (why it is not connected) rides in the tooltip and the accessible name, not in extra paragraphs.
 */
export function LiveLinkPill({ text, tone, detail, name = "sACN DMX Live Link", testId = "lightkey-link" }: { text: string; tone: Tone; detail?: string | null; name?: string; testId?: string }) {
  // `name` is empty when the visible words already say what the pill is about ("Playback API: Connected").
  const said = name ? `${name}: ${text}` : text;
  const full = detail ? `${said}. ${detail}` : said;
  return (
    <span aria-label={full} aria-live="polite" className="live-link-pill inline-flex items-center gap-[7px] whitespace-nowrap rounded-ds-full border border-edge bg-surface-950 px-3 py-[5px] text-[0.8125rem] leading-[1.45] text-ink" data-testid={testId} data-tone={tone} role="status" title={full}>
      <StatusDot ignoreAccent tone={tone} />
      <span>{text}</span>
    </span>
  );
}
