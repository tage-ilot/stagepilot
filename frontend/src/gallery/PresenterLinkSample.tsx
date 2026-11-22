import { useState } from "react";

import { PresenterLinkSection } from "../components/PresenterLinkSection";
import { PRESENTER_LINK_OPEN_KEY } from "../components/proPresenterLinkState";
import { Button } from "../components/ui/Button";
import { Panel } from "../components/ui/Panel";
import { PresenterApiContext, type PresenterApi } from "../hooks/presenterApi";
import type { Song } from "../types";
import { linkController, linkOf } from "./fixtures";
import { madeUpPresenterApi, NAMES_IN_PROPRESENTER, PLAN_NAMES, type Scenario, withoutPlayback, withPlayback } from "./presenterLinkData";
import { Fresh, Scripted, type Step } from "./Scripted";

/** The Presenter Link as it sits in the Presentation panel (the real panel frame, in the Presentation colour), asked of a made-up StagePilot. */
export function Sample({ scenario, steps = [], playback = false, songs, playing = false, link = linkController(linkOf("ready")), gap, open = true, plan = PLAN_NAMES }: { scenario?: Scenario; steps?: Step[]; playback?: boolean; /** How many of Playback's songs (four by default). */ songs?: number; playing?: boolean; link?: ReturnType<typeof linkController>; gap?: number; /** The remembered open flag the sample starts from: open (the body is what the state shows) or folded, as a new install is. */ open?: boolean; /** The loaded plan's Planning Center songs. */ plan?: { songs: Song[] } }) {
  const [api] = useState<PresenterApi>(() => madeUpPresenterApi(scenario));
  return (
    <Fresh flags={{ [PRESENTER_LINK_OPEN_KEY]: open }}>
      <PresenterApiContext.Provider value={api}>
        <Scripted gap={gap} patient steps={steps}>
          <Panel tone="presentation" variant="setup"><PresenterLinkSection library={playback ? withPlayback(songs) : withoutPlayback()} link={link} plan={plan} playing={playing} /></Panel>
        </Scripted>
      </PresenterApiContext.Provider>
    </Fresh>
  );
}

/** The swap, by hand: Playback gives the songs, then loses them again, with the section staying where it is. */
export function Swap({ steps }: { steps: Step[] }) {
  const [given, setGiven] = useState(false);
  const [api] = useState<PresenterApi>(() => madeUpPresenterApi({ titles: NAMES_IN_PROPRESENTER }));
  return (
    <Fresh flags={{ [PRESENTER_LINK_OPEN_KEY]: true }}>
      <PresenterApiContext.Provider value={api}>
        <Scripted patient steps={steps}>
          <div className="mb-2 flex flex-wrap items-center gap-3">
            <Button onClick={() => setGiven((now) => !now)} size="control" variant="secondary">{given ? "Playback loses the songs" : "Playback gives the songs"}</Button>
            <span className="text-xs text-ink-muted">{given ? "Playback's songs are listed." : "The Planning Center names are listed."}</span>
          </div>
          <Panel tone="presentation" variant="setup"><PresenterLinkSection library={given ? withPlayback() : withoutPlayback()} link={linkController(linkOf("ready"))} plan={PLAN_NAMES} /></Panel>
        </Scripted>
      </PresenterApiContext.Provider>
    </Fresh>
  );
}

