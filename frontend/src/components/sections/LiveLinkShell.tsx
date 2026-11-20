import type { ReactNode } from "react";

import type { Tone } from "../ui/types";

/** The bordered Live Link block: title, the state pill beside it, then whatever the caller lays out. The Playback panel's block gives its own title. */
export function LiveLinkShell({ pill, className, testId, title = "sACN DMX Live Link", tone, children }: { pill?: ReactNode; className?: string; testId?: string; title?: string; tone?: Tone; children: ReactNode }) {
  return (
    <section aria-label={title} className={`live-link mt-6 rounded-ds-xl border border-edge bg-surface-panel-strong px-3 py-2.5 ${className ?? ""}`} data-testid={testId} data-tone={tone}>
      <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
        <h3 className="text-base font-semibold text-ink">{title}</h3>
        {pill}
      </div>
      {children}
    </section>
  );
}
