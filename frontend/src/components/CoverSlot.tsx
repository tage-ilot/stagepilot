import { useState } from "react";

/** The 28 px square every song card keeps for the album cover: the picture, or this tile when there is none. Same size and rounding either way, so titles line up in every list. */
const SLOT = "h-7 w-7 shrink-0 rounded-ds-md bg-surface-900";

/** A quiet music note (stroke icon, like the app's other small inline icons), drawn when a song has no picture. */
function CoverPlaceholder() {
  return (
    <span aria-hidden="true" className={`grid place-items-center text-ink-muted ${SLOT}`} data-part="cover-placeholder">
      <svg aria-hidden="true" className="h-4 w-4" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.5" viewBox="0 0 16 16">
        <path d="M6 12V3.5l7-1.5V10.5" />
        <circle cx="4.5" cy="12" r="1.5" />
        <circle cx="11.5" cy="10.5" r="1.5" />
      </svg>
    </span>
  );
}

/**
 * A song card always has a cover slot: the picture, or the placeholder. The placeholder replaces the picture when the song has none
 * (`src` null: no artwork, not in Playback) and when the picture fails to load (404, 503, a blocked address): a state, never a hidden
 * box, so the slot is never an empty gap.
 */
export function CoverSlot({ src }: { src: string | null }) {
  // The address that failed, so a changed picture (a new `?v=` etag) gets its own try.
  const [failed, setFailed] = useState<string | null>(null);
  if (!src || failed === src) return <CoverPlaceholder />;
  return <img alt="" className={`${SLOT} object-cover`} data-part="cover" decoding="async" height={28} loading="lazy" onError={() => setFailed(src)} src={src} width={28} />;
}
