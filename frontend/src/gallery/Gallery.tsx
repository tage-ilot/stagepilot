import { useState } from "react";

import { lightingStates } from "./LightingStates";
import { backendPages, lightingPages, playbackPages, presentationPages, servicesPages } from "./PageStates";
import { playbackStates } from "./PlaybackStates";
import { presentationStates } from "./PresentationStates";
import { remotePages } from "./RemoteStates";
import { ConnectionGroup } from "./StateCard";
import { actionTemplates, noticeTemplates, optionalTemplates, settingsTemplates, summaryTemplates } from "./TemplateStates";

/** The widths the switch offers: the narrowest phone the app supports and a laptop. The framed page sees them as its own screen width, so every breakpoint answers. */
export const WIDTHS = [{ value: 360, label: "360 px" }, { value: 1280, label: "1280 px" }] as const;

/** Every state of every connection, as the real blocks draw them (made-up data). */
export function Frame() {
  return (
    <main className="mx-auto min-h-screen max-w-4xl space-y-10 bg-background p-4 font-type-body-md text-ink-body" data-gallery-frame>
      <p className="text-sm text-ink-muted">Press the Details button on a block to open or close it. Typing, Scan and Test connection do nothing here: the data is made up and no backend is asked.</p>
      <ConnectionGroup id="presentation" intro="ProPresenter Live Link (Configuration > Presentation): the ProPresenter API and the optional Presenter Link." states={presentationStates} title="Connection section: Presentation" />
      <ConnectionGroup id="playback" intro="Playback Live Link (Configuration > Playback): the Playback API and the optional Library Link." states={playbackStates} title="Connection section: Playback" />
      <ConnectionGroup id="lighting" intro="sACN DMX Live Link (Configuration > Lighting): the Lightkey link and its helper on the Lighting computer." states={lightingStates} title="Connection section: Lighting" />
      <p className="border-t border-edge pt-6 text-sm text-ink-muted">Every configuration page below is the REAL page with made-up data. A held change or an error is reached by doing what a person does (typing, pressing Save), not painted on. Each page is built in the same order: header, alert lines, connection, settings, actions, optional sections, the page's own content, notices, Save bar, summary.</p>
      <ConnectionGroup id="page-presentation" intro="Settings, actions, summary and notices of the Presentation page: clean, held, error, busy, disabled, failed, loading." states={presentationPages} title="Page: Presentation" />
      <ConnectionGroup id="page-services" intro="Services (Planning Center): the three sign-in notices, the optional Manual API Connection, the settings group, the action row." states={servicesPages} title="Page: Services" />
      <ConnectionGroup id="page-backend" intro="StagePilot: the summary cards first, the optional Advanced section, Connection Access, the Save bar." states={backendPages} title="Page: StagePilot" />
      <ConnectionGroup id="page-playback" intro="Playback and MIDI: the Song order band (remembered open or closed), the MIDI input actions and the velocities." states={playbackPages} title="Page: Playback / MIDI" />
      <ConnectionGroup id="page-lighting" intro="Lighting: OSC and MIDI views, the settings group on one line, Refresh outputs, the optional test cue and timeline." states={lightingPages} title="Page: Lighting" />
      <ConnectionGroup id="page-remote" intro="Remote access (inside StagePilot): boxed groups, notices as lines and as named question groups, the remembered open section." states={remotePages} title="Page: Remote access" />
      <p className="border-t border-edge pt-6 text-sm text-ink-muted">The templates by themselves, in a connection's colour, so a change to one is judged apart from any page.</p>
      <ConnectionGroup id="template-settings" intro="SettingsSection: every column set, held, error, hint, busy, boxed." states={settingsTemplates} title="Template: Settings" />
      <ConnectionGroup id="template-actions" intro="ActionRow and its buttons: idle, busy, disabled, with the note." states={actionTemplates} title="Template: Actions" />
      <ConnectionGroup id="template-summary" intro="StatusSummary: the list and the cards, with ok, warn and danger words." states={summaryTemplates} title="Template: Summary" />
      <ConnectionGroup id="template-notices" intro="SectionNotice: the four tones as boxes, with a title and a button, as a question group, and as lines." states={noticeTemplates} title="Template: Notices" />
      <ConnectionGroup id="template-optional" intro="OptionalSection and the band: closed, open, remembered, opened by a question." states={optionalTemplates} title="Template: Optional sections" />
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
        <p className="text-sm text-ink-muted">Every state of every section template (Connection, Settings, Actions, Summary, Notices, Optional) and of every configuration page. Review-only; made-up data.</p>
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
