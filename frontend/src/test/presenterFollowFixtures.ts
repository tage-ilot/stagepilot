import { vi } from "vitest";

import type { PresenterFollowController } from "../hooks/usePresenterFollow";
import type { FollowSong, FollowStatus } from "../presenterFollow";

const TITLES = ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"];

/** The four plan songs of the set, `k1` to `k4` as the library plan names them, each with its verdict (the last one repeats). */
export const followSongs = (...verdicts: Partial<FollowSong>[]): FollowSong[] => TITLES.map((title, i) => ({
  key: `k${i + 1}`, title, following: "cues" as const, code: "cues" as const, text: "Following by Playback cues", advice: null, lines_per_slide: 1, playback_lines: 1, template: "ProPresenter 1 Line Template", ...verdicts[Math.min(i, verdicts.length - 1)],
}));

export const NO_CUES_SONG: Partial<FollowSong> = { code: "bars", text: "Following by bars (no Playback cues)", following: "bars", advice: "No cues: in Playback choose a ProPresenter 1 Line Template for this song" };

export const followStatus = (over: Partial<FollowStatus> = {}): FollowStatus => ({
  enabled: true, offset_ms: 0, state: "idle", message: "Nothing to follow", song: null, section: null, slide: null, hold_reason: null, can_try_again: false, recent_triggers: [], songs: followSongs(), ...over,
});

export const followController = (over: Partial<PresenterFollowController> = {}): PresenterFollowController => ({
  status: followStatus(), error: null, enabled: true, offsetMs: 0, pending: null,
  setEnabled: vi.fn().mockResolvedValue(true), setOffset: vi.fn().mockResolvedValue(true), tryAgain: vi.fn(), ...over,
});
