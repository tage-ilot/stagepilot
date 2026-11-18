import { useState } from "react";

import { LightkeyStrip } from "../components/LightkeyStrip";
import type { LightkeyStatus } from "../types";

/** The Lighting strip, reading its status from the page (made-up) instead of the backend; its host and port boxes are typed into. */
export function Lighting({ status, attempted = false, held = false, host = "192.0.2.10" }: { status: LightkeyStatus; attempted?: boolean; held?: boolean; host?: string }) {
  const [typed, setTyped] = useState(host);
  const [port, setPort] = useState("21600");
  return (
    <LightkeyStrip
      connection={{ host: typed, onHost: setTyped, oscPort: port, onPort: setPort, savedOscHost: "192.0.2.10", attempted, held: { host: held, port: held } }}
      lightingEnabled
      readStatus={async () => status}
      testCue={<p className="text-sm text-ink-body">The test cue's address and values go here.</p>}
    />
  );
}
