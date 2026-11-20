import { useState } from "react";

import { ProPresenterLinkBlock } from "../components/ProPresenterLinkBlock";
import type { LinkStatus } from "../propresenterLink";
import type { ProPresenterStatusResponse } from "../types";
import { linkController, presenterApi } from "./fixtures";

/** The Presentation panel's block, with the host and port boxes typed into like the real panel's (nothing is saved anywhere). */
export function Presentation({ api = presenterApi(), link, held = false, error }: { api?: ProPresenterStatusResponse; link?: LinkStatus | null; held?: boolean; error?: string }) {
  const [host, setHost] = useState(held ? "192.0.2.77" : error ? "192.0.2.10 " : api.host);
  const [port, setPort] = useState(String(api.port));
  return (
    <ProPresenterLinkBlock
      connection={{ host, port, onHost: setHost, onPort: setPort, held: { host: held, port: false }, errors: error ? { host: error } : {}, busy: false, testDisabled: !api.enabled, testing: false, onTest: () => undefined }}
      link={link === undefined ? undefined : linkController(link)}
      propresenter={api}
    />
  );
}
