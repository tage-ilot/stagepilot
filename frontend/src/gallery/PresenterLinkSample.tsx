import { useState } from "react";

import { PresenterLinkSection } from "../components/PresenterLinkSection";
import { Button } from "../components/ui/Button";
import { Panel } from "../components/ui/Panel";
import { PresenterApiContext, type PresenterApi } from "../hooks/presenterApi";
import { linkController, linkOf } from "./fixtures";
import { madeUpPresenterApi, NAMES_IN_PROPRESENTER, PLAN_NAMES, type Scenario, withoutPlayback, withPlayback } from "./presenterLinkData";
import { Fresh, Scripted, type Step } from "./Scripted";

/** The Presenter Link as it sits in the Presentation panel (the real panel frame, in the Presentation colour), asked of a made-up StagePilot. */
export function Sample({ scenario, steps = [], playback = false, link = linkController(linkOf("ready")), gap }: { scenario?: Scenario; steps?: Step[]; playback?: boolean; link?: ReturnType<typeof linkController>; gap?: number }) {
  const [api] = useState<PresenterApi>(() => madeUpPresenterApi(scenario));
  return (
    <Fresh>
      <PresenterApiContext.Provider value={api}>
        <Scripted gap={gap} patient steps={steps}>
          <Panel tone="presentation" variant="setup"><PresenterLinkSection library={playback ? withPlayback() : withoutPlayback()} link={link} plan={PLAN_NAMES} /></Panel>
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
    <Fresh>
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

