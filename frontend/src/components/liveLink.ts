import type { LightkeyFixtureStatus, LightkeyStatus } from "../types";
import { statusTone } from "./ui/statusWord";
import type { Tone } from "./ui/types";

/** Live Link controls are 40 px on a fine pointer (as the approved mock) and the usual 44 px on touch. */
export const CONTROL_40 = "!h-10 !min-h-10 !py-0 coarse:!h-11 coarse:!min-h-[44px]";

/** Where a cue's OSC address comes from. Behind a small disclosure, always fully readable (it used to be cut off). */
export const CUE_ADDRESS_HELP = "Enter the port your lighting app listens for OSC on. In Lightkey, control-click a cue and choose External Control, then Copy OSC Address, to get a cue's address.";

/** The fixture-level line: what is captured, in words. */
export function lightkeyFixtureWord(fixtures: LightkeyFixtureStatus): string {
  if (fixtures.link === "live") return `Colour and levels: ${fixtures.fixtures} fixture${fixtures.fixtures === 1 ? "" : "s"}`;
  if (fixtures.link === "waiting") return "Colour and levels: connecting";
  return fixtures.detail ?? "Colour and levels: not receiving";
}

/** What the Live Link pill says, and its colour. The ONE statement of the connection state on the block; the colours come from the
 * same word table as the panel header's pill (statusTone), so a state never wears two colours. */
export interface LiveLinkPillState { text: string; tone: Tone; /** Why, for the tooltip and the accessible name (never a separate paragraph). */ detail: string | null }

export function liveLinkPillState(status: LightkeyStatus): LiveLinkPillState {
  const fixtures = status.fixtures;
  switch (status.link) {
    case "connected":
      if (fixtures?.link === "waiting") return { text: "Connecting\u2026", tone: statusTone.Connecting, detail: lightkeyFixtureWord(fixtures) };
      if (fixtures?.link === "lost") return { text: "Connected \u00B7 not receiving", tone: statusTone.Connecting, detail: lightkeyFixtureWord(fixtures) };
      if (fixtures?.link === "live") return { text: `Connected \u00B7 ${fixtures.fixtures} fixture${fixtures.fixtures === 1 ? "" : "s"}`, tone: statusTone.Connected, detail: null };
      return { text: "Connected", tone: statusTone.Connected, detail: null };
    case "off": return { text: "Off", tone: statusTone.Disconnected, detail: status.detail };
    case "lightkey_closed": return { text: "Lightkey is closed", tone: statusTone.Connecting, detail: status.detail };
    default: return { text: "Not connected", tone: statusTone.Error, detail: status.detail };
  }
}

/**
 * The backend's neutral OSC notes, in one short line: "OSC ready for HOST:PORT. Nothing sent yet; UDP does not confirm delivery." becomes
 * "OSC ready for HOST:PORT. UDP does not confirm delivery." Anything else is shown as it is.
 */
export function shortLightingMessage(message: string): string {
  return message.replace(/\. Nothing sent yet; UDP does not confirm delivery\.$/, ". UDP does not confirm delivery.");
}
