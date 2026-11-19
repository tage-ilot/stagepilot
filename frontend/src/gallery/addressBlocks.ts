import type { Tone } from "../components/ui/types";
import type { RecentScope } from "../hooks/useRecentAddresses";

/** The three Live Link blocks' boxes: the same box, each with its own words and its own saved list (the scope). */
export interface AddressBlock { title: string; tone: Tone; label: string; caption: string; scope: RecentScope; placeholder: string }

export const BLOCKS: AddressBlock[] = [
  { title: "sACN DMX Live Link", tone: "lighting", label: "Lighting host or IP address", caption: "Host", scope: "lighting", placeholder: "192.0.2.10" },
  { title: "Playback Live Link", tone: "playback", label: "Playback host", caption: "Host", scope: "playback", placeholder: "For example, 192.0.2.10" },
  { title: "ProPresenter Live Link", tone: "presentation", label: "Host", caption: "Host", scope: "presentation", placeholder: "192.0.2.10" },
];

