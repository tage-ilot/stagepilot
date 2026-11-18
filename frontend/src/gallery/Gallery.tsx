import { useState } from "react";

import { lightingStates } from "./LightingStates";
import { playbackStates } from "./PlaybackStates";
import { presentationStates } from "./PresentationStates";
import { ConnectionGroup } from "./StateCard";

/** The widths the switch offers: the narrowest phone the app supports and a laptop. The framed page sees them as its own screen width, so every breakpoint answers. */
export const WIDTHS = [{ value: 360, label: "360 px" }, { value: 1280, label: "1280 px" }] as const;

/** Every state of every connection, as the real blocks draw them (made-up data). */
export function Frame() {
  return (
    <main className="mx-auto min-h-screen max-w-4xl space-y-10 bg-background p-4 font-type-body-md text-ink-body" data-gallery-frame>
      <p className="text-sm text-ink-muted">Press the Details button on a block to open or close it. Typing, Scan and Test connection do nothing here: the data is made up and no backend is asked.</p>
      <ConnectionGroup id="presentation" intro="ProPresenter Live Link (Configuration > Presentation): the ProPresenter API and the optional Presenter Link." states={presentationStates} title="Presentation" />
      <ConnectionGroup id="playback" intro="Playback Live Link (Configuration > Playback): the Playback API and the optional Library Link." states={playbackStates} title="Playback" />
      <ConnectionGroup id="lighting" intro="sACN DMX Live Link (Configuration > Lighting): the Lightkey link and its helper on the Lighting computer." states={lightingStates} title="Lighting" />
    </main>
  );
}

/** The page: a width switch over the frame, which is the same page in an iframe of that width (so the 640 px breakpoint really switches). */
export function Gallery() {
  const [width, setWidth] = useState<number | "fit">(1280);
  return (
    <div className="flex h-screen min-h-0 flex-col bg-background text-ink-body" data-section-gallery>
      <header className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-edge px-4 py-3">
        <h1 className="text-lg font-bold text-ink">StagePilot section gallery</h1>
        <p className="text-sm text-ink-muted">Every state of the Connection section (the three Live Link blocks). Review-only; made-up data.</p>
        <div aria-label="Width" className="ml-auto flex gap-2" role="group">
          {[...WIDTHS.map((item) => ({ value: item.value as number | "fit", label: item.label })), { value: "fit" as const, label: "Fit" }].map((item) => (
            <button
              aria-pressed={width === item.value}
              className={`min-h-9 rounded-ds-lg border px-3 text-sm ${width === item.value ? "border-ink bg-surface-900 text-ink" : "border-edge text-ink-body can-hover:border-ink-muted"}`}
              key={item.label}
              onClick={() => setWidth(item.value)}
              type="button"
            >
              {item.label}
            </button>
          ))}
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-auto bg-surface-950 p-3">
        <iframe className="mx-auto block h-full max-w-full border border-edge bg-background" data-testid="gallery-frame" src="?frame=1" style={{ width: width === "fit" ? "100%" : width }} title={`StagePilot sections at ${width === "fit" ? "the window's width" : `${width} px`}`} />
      </div>
    </div>
  );
}
