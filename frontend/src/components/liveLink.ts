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

/** What one Live Link pill says, and its colour. The colours come from the same word table as the panel header's pill (statusTone),
 * so a state never wears two colours. */
export interface LiveLinkPillState { text: string; tone: Tone; /** Why, for the tooltip and the accessible name (never a separate paragraph). */ detail: string | null }

/** The block's two pills, always both drawn, always in this order: sACN (the link to Lightkey) and DMX (the fixture stream). */
export interface LiveLinkPills { sacn: LiveLinkPillState; dmx: LiveLinkPillState }

/** DMX has nothing to say while sACN is down (and for a Lightkey API that does not report fixtures): a dash in the neutral tone, never hidden. */
const DMX_NONE: LiveLinkPillState = { text: "DMX: \u2014", tone: statusTone["Not connected"], detail: null };

export function liveLinkPills(status: LightkeyStatus): LiveLinkPills {
  const fixtures = status.fixtures;
  const sacn = (word: string, tone: Tone, detail: string | null): LiveLinkPillState => ({ text: `sACN: ${word}`, tone, detail });
  const dmx = (word: string, tone: Tone, detail: string | null): LiveLinkPillState => ({ text: `DMX: ${word}`, tone, detail });
  switch (status.link) {
    case "connected":
      if (fixtures?.link === "waiting") return { sacn: sacn("Connected", statusTone.Connected, null), dmx: dmx("Connecting\u2026", statusTone.Connecting, lightkeyFixtureWord(fixtures)) };
      if (fixtures?.link === "lost") return { sacn: sacn("Connected", statusTone.Connected, null), dmx: dmx("Not receiving", statusTone.Connecting, lightkeyFixtureWord(fixtures)) };
      if (fixtures?.link === "live") return { sacn: sacn("Connected", statusTone.Connected, null), dmx: dmx(`${fixtures.fixtures} fixture${fixtures.fixtures === 1 ? "" : "s"} identified`, statusTone.Connected, null) };
      return { sacn: sacn("Connected", statusTone.Connected, null), dmx: DMX_NONE };
    case "off": return { sacn: sacn("Off", statusTone["Not connected"], status.detail), dmx: DMX_NONE };
    case "lightkey_closed": return { sacn: sacn("Lightkey is closed", statusTone.Connecting, status.detail), dmx: DMX_NONE };
    default: return { sacn: sacn("Not connected", statusTone.Error, status.detail), dmx: DMX_NONE };
  }
}

/**
 * The backend's neutral OSC notes, in one short line: "OSC ready for HOST:PORT. Nothing sent yet; UDP does not confirm delivery." becomes
 * "OSC ready for HOST:PORT. UDP does not confirm delivery." Anything else is shown as it is.
 */
export function shortLightingMessage(message: string): string {
  return message.replace(/\. Nothing sent yet; UDP does not confirm delivery\.$/, ". UDP does not confirm delivery.");
}
