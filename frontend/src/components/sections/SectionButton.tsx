import type { ReactNode } from "react";

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
  /** The page's own logo, drawn in a tile at the left (decorative; the title names the page). */
  logo?: ReactNode;
  className?: string;
  testId?: string;
}

/**
 * The SECTION BUTTON: a card of its own that opens a page of its own (VerseVision, from the Presentation panel) instead of changing
 * anything on this one. It is deliberately NOT the feature card of the band beside it (the Presenter Link folds open here; this goes
 * somewhere else): a flat dark card with the neutral dark outline, the page's logo in a tile with a soft glow of that colour behind it, the title in plain ink, one line under it, and an "Open" pill with an
 * up-right arrow in the connection's colour at the right. It is one real link, so it can be opened in a new tab, copied or
 * middle-clicked; it never saves or changes a setting, so it sits with the page's own content and not with the Save bar.
 */
export function SectionButton({ title, description, href, onOpen, logo, className, testId }: SectionButtonProps) {
  const colour = accentClasses[useAccent() ?? "presentation"];
  return (
    <div className={cx("min-w-0", className)} data-section="section-button">
      <a
        className="group flex min-h-[4.25rem] w-full min-w-0 touch-manipulation items-center gap-4 rounded-ds-lg border border-edge bg-surface-900/60 py-3 pl-5 pr-4 text-left transition-colors motion-reduce:transition-none can-hover:border-ink-muted/60 can-hover:bg-surface-900"
        data-testid={testId}
        href={href}
        onClick={onOpen ? (event) => { event.preventDefault(); onOpen(href); } : undefined}
        rel="noopener noreferrer"
        target="_blank"
      >
        {logo && (
          <span className="relative flex size-12 shrink-0 items-center justify-center" data-part="logo-tile">
            <span aria-hidden="true" className={cx("absolute inset-0 rounded-ds-full opacity-70 blur-md", colour.dot)} data-part="glow" />
            <span className="relative flex size-12 items-center justify-center">{logo}</span>
          </span>
        )}
        <span className="flex min-w-0 flex-1 flex-col" data-part="words">
          <span className="font-type-body-md text-lg font-bold text-ink" data-part="title">{title}</span>
          {description && <>{" "}<span className="mt-0.5 min-w-0 text-balance text-sm text-ink-body" data-part="description">{description}</span></>}
          {" "}
          <span className="sr-only">(opens in a new window)</span>
        </span>
        <span aria-hidden="true" className={cx("flex shrink-0 items-center gap-1.5 rounded-ds-full border px-3 py-1.5 text-sm font-semibold", colour.tinted)} data-part="open">
          <span className="hidden sm:inline">Open</span>
          <svg className="size-3.5" viewBox="0 0 12 12">
            <path d="M3 9l6-6M4 3h5v5" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.75" />
          </svg>
        </span>
      </a>
    </div>
  );
}
