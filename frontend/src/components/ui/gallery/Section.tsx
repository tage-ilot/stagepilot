import type { ReactNode } from "react";

import { states, type GalleryState } from "./states";

export function Section({ name, title, children }: { name: string; title: string; children: ReactNode }) {
  return (
    <section data-gallery-section={name} aria-labelledby={`${name}-title`} className="space-y-4">
      <h2 id={`${name}-title`} className="text-xl font-bold text-ink">{title}</h2>
      {children}
    </section>
  );
}

/** Render actual primitive props per state; mark its interaction target data-gallery-target. */
export function StateRow({ label, render }: { label: string; render: (state: GalleryState) => ReactNode }) {
  return (
    <div data-gallery-row={label} className="space-y-2">
      <h3 className="font-semibold text-ink-body">{label}</h3>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {states.map((state) => (
          <div key={state} data-gallery-state={state} className="min-w-0 space-y-2">
            <p className="text-sm text-ink-muted">{state}</p>
            {render(state)}
          </div>
        ))}
      </div>
    </div>
  );
}
