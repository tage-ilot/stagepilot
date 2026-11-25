import { accentClasses, useAccent } from "../ui/accent";
import { cx } from "../ui/cx";

export interface SectionButtonProps {
  /** The name on the button (`VerseVision`). */
  title: string;
  /** One short line under it: what the page is for. */
  description?: string;
  /** The page it opens, always in a tab or window of its own. */
  href: string;
  /** Opens the page where the browser's own new tab is not the way (the desktop app hands it to the system browser). Given, the click does only this. */
  onOpen?: (href: string) => void;
  className?: string;
  testId?: string;
}

/**
 * The BIG SECTION BUTTON: one wide button in the connection's colour that opens a page of its own (VerseVision, from the Presentation
 * panel) instead of changing anything on this one. It is a real link, so it can be opened in a new tab, copied or middle-clicked; it
 * never saves or changes a setting, so it sits with the page's own content and not with the Save bar.
 */
export function SectionButton({ title, description, href, onOpen, className, testId }: SectionButtonProps) {
  const accent = useAccent();
  const colour = accentClasses[accent ?? "presentation"];
  return (
    <a
      className={cx(
        "flex min-h-ds-touch-target w-full min-w-0 touch-manipulation items-center gap-4 rounded-ds-xl border px-5 py-5 text-left transition-colors duration-[140ms] motion-reduce:transition-none",
        colour.tinted,
        colour.tintedHover,
        className,
      )}
      data-section="section-button"
      data-testid={testId}
      href={href}
      onClick={onOpen ? (event) => { event.preventDefault(); onOpen(href); } : undefined}
      rel="noopener noreferrer"
      target="_blank"
    >
      <span className="min-w-0 flex-1" data-part="words">
        <span className="block break-words text-xl font-bold text-ink" data-part="title">{title}</span>
        {description && <>{" "}<span className="mt-1 block break-words text-sm text-ink-body" data-part="description">{description}</span></>}
        {" "}
        <span className="sr-only">(opens in a new window)</span>
      </span>
      <svg aria-hidden="true" className={cx("size-6 shrink-0", colour.text)} data-part="arrow" viewBox="0 0 24 24">
        <path d="M8 16L16 8M9.5 8H16v6.5" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" />
      </svg>
    </a>
  );
}
