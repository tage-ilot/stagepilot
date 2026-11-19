/** What the gallery was built from, stamped in by `vite.gallery.config.ts` (the `define`). Under the tests nothing stamps it, so the page says "unknown". */
export interface BuildInfo { commit: string; builtAt: string; dirty: boolean }

declare const __GALLERY_BUILD__: BuildInfo | undefined;

export const buildInfo: BuildInfo = typeof __GALLERY_BUILD__ === "undefined" ? { commit: "unknown", builtAt: "", dirty: false } : __GALLERY_BUILD__;

/** The line at the top of the gallery: which commit this page shows, so a reviewer never judges a stale build. */
export const builtFromLine = (info: BuildInfo = buildInfo) => `Last built from ${info.commit}${info.dirty ? " + uncommitted changes" : ""}${info.builtAt ? ` on ${info.builtAt}` : ""}`;
