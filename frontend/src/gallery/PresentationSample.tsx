import { useState } from "react";

import { ProPresenterLinkBlock } from "../components/ProPresenterLinkBlock";
import { NO_SCAN, type ScanState } from "../components/sections";
import type { LinkStatus } from "../propresenterLink";
import type { ProPresenterStatusResponse } from "../types";
import { linkController, presenterApi } from "./fixtures";

/** The Presentation panel's block, with the host and API boxes typed into like the real panel's (nothing is saved anywhere). */
export function Presentation({ api = presenterApi(), link, held = false, error, scan = NO_SCAN, connecting = false }: { api?: ProPresenterStatusResponse; link?: LinkStatus | null; held?: boolean; error?: string; scan?: ScanState; connecting?: boolean }) {
  const [host, setHost] = useState(held ? "192.0.2.77" : error ? "192.0.2.10 " : api.host);
  const [port, setPort] = useState(String(api.port));
  return (
    <ProPresenterLinkBlock
      connection={{ host, port, onHost: setHost, onPort: setPort, held: { host: held, port: false }, errors: error ? { host: error } : {}, busy: false, checksAgain: api.enabled && !held, connectDisabled: false, connecting, onConnect: () => undefined, scan: { state: scan, start: () => undefined, cancel: () => undefined, choose: (chosen, chosenPort) => { setHost(chosen); if (chosenPort !== undefined) setPort(String(chosenPort)); } } }}
      link={link === undefined ? undefined : linkController(link)}
      propresenter={api}
    />
  );
}
