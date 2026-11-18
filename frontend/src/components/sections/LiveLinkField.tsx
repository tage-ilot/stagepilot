import type { ReactNode } from "react";

/** The small uppercase caption over a Live Link field. */
export function LiveLinkField({ caption, className, marker, children }: { caption: string; className?: string; marker?: ReactNode; children: ReactNode }) {
  return (
    <label className={`flex min-w-0 flex-col gap-0.5 text-[0.6875rem] font-medium uppercase tracking-[0.07em] text-ink-muted ${className ?? ""}`}>
      {/* `marker` (a held change's words) sits beside the caption; without one the caption is drawn exactly as before. */}
      {marker ? <span className="flex flex-wrap items-center justify-between gap-x-3 gap-y-0.5"><span>{caption}</span>{marker}</span> : caption}
      {children}
    </label>
  );
}
