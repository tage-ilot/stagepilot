import { Fragment, useState } from "react";

import { builtFromLine } from "./build";
import { layoutTemplates } from "./LayoutStates";

import { lightingStates } from "./LightingStates";
import { backendPages, lightingPages, playbackPages, presentationPages, servicesPages } from "./PageStates";
import { playbackStates } from "./PlaybackStates";
import { presentationStates } from "./PresentationStates";
import { presenterLinkStates } from "./PresenterLinkStates";
import { remotePages } from "./RemoteStates";
import { ConnectionGroup, type SampleState } from "./StateCard";
import { actionTemplates, noticeTemplates, optionalTemplates, settingsTemplates, summaryTemplates } from "./TemplateStates";

/** The widths the switch offers: the narrowest phone the app supports and a laptop. The framed page sees them as its own screen width, so every breakpoint answers. */
export const WIDTHS = [{ value: 360, label: "360 px" }, { value: 1280, label: "1280 px" }] as const;

const PAGES_NOTE = "Every configuration page below is the REAL page with made-up data. A held change or an error is reached by doing what a person does (typing, pressing Save), not painted on. Each page is built in the same order: header, alert lines, connection, settings, actions, optional sections, the page's own content, notices, Save bar, summary.";
const TEMPLATES_NOTE = "The templates by themselves, in a connection's colour, so a change to one is judged apart from any page.";

/** Every group of the gallery, in the order of the page. `lead` is a paragraph that opens a run of groups. */
const GROUPS: { id: string; title: string; intro: string; states: SampleState[]; lead?: string }[] = [
  { id: "presentation", title: "Connection section: Presentation", intro: "ProPresenter Live Link (Configuration > Presentation): the ProPresenter API and the optional Presenter Link.", states: presentationStates },
  { id: "playback", title: "Connection section: Playback", intro: "Playback Live Link (Configuration > Playback): the Playback API and the optional Library Link.", states: playbackStates },
  { id: "lighting", title: "Connection section: Lighting", intro: "sACN DMX Live Link (Configuration > Lighting): the Lightkey link and its helper on the Lighting computer.", states: lightingStates },
  { id: "page-presentation", title: "Page: Presentation", intro: "Settings, actions, summary and notices of the Presentation page: clean, held, error, busy, disabled, failed, loading.", states: presentationPages, lead: PAGES_NOTE },
  { id: "presenter-link", title: "Section: Presenter Link", intro: "The Presenter Link in the Presentation panel: playlist and section, ONE list of songs (Planning Center names until Playback has given them), and ONE button. Every state is reached by pressing the real buttons; the answers are made up.", states: presenterLinkStates },
  { id: "page-services", title: "Page: Services", intro: "Services (Planning Center): the three sign-in notices, the optional Manual API Connection, the settings group, the action row.", states: servicesPages },
  { id: "page-backend", title: "Page: StagePilot", intro: "StagePilot: the summary cards first, the optional Advanced section, Connection Access, the Save bar.", states: backendPages },
  { id: "page-playback", title: "Page: Playback / MIDI", intro: "Playback and MIDI: the Song order band (remembered open or closed), the MIDI input actions and the velocities.", states: playbackPages },
  { id: "page-lighting", title: "Page: Lighting", intro: "Lighting: OSC and MIDI views, the settings group on one line, Refresh outputs, the optional test cue and timeline.", states: lightingPages },
  { id: "page-remote", title: "Page: Remote access", intro: "Remote access (inside StagePilot): boxed groups, notices as lines and as named question groups, the remembered open section.", states: remotePages },
  { id: "template-settings", title: "Template: Settings", intro: "SettingsSection: every column set, held, error, hint, busy, boxed.", states: settingsTemplates, lead: TEMPLATES_NOTE },
  { id: "template-actions", title: "Template: Actions", intro: "ActionRow and its buttons: idle, busy, disabled, with the note.", states: actionTemplates },
  { id: "template-summary", title: "Template: Summary", intro: "StatusSummary: the list and the cards, with ok, warn and danger words.", states: summaryTemplates },
  { id: "template-notices", title: "Template: Notices", intro: "SectionNotice: the four tones as boxes, with a title and a button, as a question group, and as lines.", states: noticeTemplates },
  { id: "template-optional", title: "Template: Optional sections", intro: "OptionalSection and the band: closed, open, remembered, opened by a question.", states: optionalTemplates },
  { id: "template-layout", title: "Template: Panel order, and a new connection page", intro: "PanelLayout with every slot, with the summary first, and the worked example of DESIGN.md: a new connection built from the templates in 35 lines.", states: layoutTemplates },
];

/** One column of the side-by-side view: a connection, with the groups that show it (its Live Link block, then its page). */
const COLUMNS: { label: string; groups: string[] }[] = [
  { label: "Presentation", groups: ["presentation", "page-presentation", "presenter-link"] },
  { label: "Playback / MIDI", groups: ["playback", "page-playback"] },
  { label: "Lighting", groups: ["lighting", "page-lighting"] },
  { label: "Services", groups: ["page-services"] },
  { label: "StagePilot", groups: ["page-backend", "page-remote"] },
];

/** Every state of every connection, as the real blocks draw them (made-up data). `only` (a comma list of group ids, from `?only=`) draws just those groups: that is one column of the side-by-side view. */
export function Frame({ only }: { only?: string[] }) {
  const shown = GROUPS.filter((group) => !only || only.includes(group.id));
  return (
    <main className="mx-auto min-h-screen max-w-4xl space-y-10 bg-background p-4 font-type-body-md text-ink-body" data-gallery-frame>
      {!only && <p className="text-sm text-ink-muted">Press the Details button on a block to open or close it. Typing, Scan and Test connection do nothing here: the data is made up and no backend is asked.</p>}
      {shown.map((group) => (
        <Fragment key={group.id}>
          {!only && group.lead && <p className="border-t border-edge pt-6 text-sm text-ink-muted">{group.lead}</p>}
          <ConnectionGroup id={group.id} intro={group.intro} states={group.states} title={group.title} />
        </Fragment>
      ))}
    </main>
  );
}

type View = "stacked" | "side-by-side";

/** The page: a width switch over the frame, which is the same page in an iframe of that width (so the 640 px breakpoint really switches). Side by side: one frame per connection. */
export function Gallery() {
  const [width, setWidth] = useState<number | "fit">(1280);
  const [view, setView] = useState<View>("stacked");
  const pressed = (on: boolean) => `min-h-9 rounded-ds-lg border px-3 text-sm ${on ? "border-ink bg-surface-900 text-ink" : "border-edge text-ink-body can-hover:border-ink-muted"}`;
  const px = width === "fit" ? "100%" : width;
  return (
    <div className="flex h-screen min-h-0 flex-col bg-background text-ink-body" data-section-gallery>
      <header className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-edge px-4 py-3">
        <h1 className="text-lg font-bold text-ink">StagePilot section gallery</h1>
        <p className="text-sm text-ink-muted">Every state of every section template (Connection, Settings, Actions, Summary, Notices, Optional, Panel order) and of every configuration page. Review-only; made-up data.</p>
        <p className="w-full text-xs text-ink-muted" data-testid="gallery-built-from">{builtFromLine()}</p>
        <div aria-label="View" className="ml-auto flex gap-2" role="group">
          {([["stacked", "One column"], ["side-by-side", "Side by side"]] as const).map(([value, label]) => (
            <button aria-pressed={view === value} className={pressed(view === value)} key={value} onClick={() => setView(value)} type="button">{label}</button>
          ))}
        </div>
        <div aria-label="Width" className="flex gap-2" role="group">
          {[...WIDTHS.map((item) => ({ value: item.value as number | "fit", label: item.label })), { value: "fit" as const, label: "Fit" }].map((item) => (
            <button aria-pressed={width === item.value} className={pressed(width === item.value)} key={item.label} onClick={() => setWidth(item.value)} type="button">{item.label}</button>
          ))}
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-auto bg-surface-950 p-3">
        {view === "stacked" ? (
          <iframe className="mx-auto block h-full max-w-full border border-edge bg-background" data-testid="gallery-frame" src="?frame=1" style={{ width: px }} title={`StagePilot sections at ${width === "fit" ? "the window's width" : `${width} px`}`} />
        ) : (
          <div className="flex h-full min-w-min gap-3" data-testid="gallery-columns">
            {COLUMNS.map((column) => (
              <section aria-label={column.label} className="flex h-full min-h-0 flex-none flex-col gap-1" key={column.label} style={{ width: width === "fit" ? "24rem" : width }}>
                <h2 className="text-sm font-semibold text-ink">{column.label}</h2>
                <iframe className="block min-h-0 w-full flex-1 border border-edge bg-background" data-testid="gallery-column-frame" src={`?frame=1&only=${column.groups.join(",")}`} title={`${column.label} at ${width === "fit" ? "24rem" : `${width} px`}`} />
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
