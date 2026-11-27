// Made-up data for the follow-Playback states of the section gallery: no real computer, song or ProPresenter. The controller answers from
// here and keeps the switch in memory, so pressing it in the gallery changes the state the way the real one would.
import { useState } from "react";

import type { PresenterFollowController } from "../hooks/usePresenterFollow";
import type { FollowSong, FollowStatus } from "../presenterFollow";

const TITLES = ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"] as const;
const CUES_OFF: FollowSong["code"] = "switch_off";

type Verdict = Partial<FollowSong> & Pick<FollowSong, "code" | "text">;
/** The four songs of Playback's set (keys `k1` to `k4`, as the library plan names them), each with its verdict. */
const songs = (...verdicts: Verdict[]): FollowSong[] => TITLES.map((title, i) => ({
  key: `k${i + 1}`, title, following: null, advice: null, lines_per_slide: null, playback_lines: null, template: null, ...verdicts[Math.min(i, verdicts.length - 1)]!,
}));

const CUES: Verdict = { code: "cues", text: "Following by Playback cues", following: "cues" };
const NO_CUES: Verdict = { code: "bars", text: "Following by bars (no Playback cues)", following: "bars", advice: "No cues: in Playback choose a ProPresenter 1 Line Template for this song", lines_per_slide: 1, template: "ProPresenter 1 Line Template" };
/** A song file with two lines of text per slide: the recommended template is the 2 Line one. */
const NO_CUES_TWO: Verdict = { ...NO_CUES, advice: "No cues: in Playback choose the ProPresenter 2 Line Template for this song", lines_per_slide: 2, template: "ProPresenter 2 Line Template" };
/** A song file with three lines per slide: Playback has no such template, so the fix says to pick the one that matches the file. */
const NO_CUES_ODD: Verdict = { ...NO_CUES, advice: "No cues: in Playback choose a ProPresenter Line Template that matches the file for this song", lines_per_slide: 3, template: null };
const MISMATCH: Verdict = { code: "cues_do_not_match", text: "Cues do not match the file (cues go to slide 30, file has 54 cues)", following: "bars", advice: "In Playback choose the ProPresenter 2 Line Template for this song", lines_per_slide: 2, template: "ProPresenter 2 Line Template" };
const ALIGNED: Verdict = { code: "cues_aligned", text: "Following by Playback cues, aligned with extra slides (check the first slide of Verse 1)", following: "cues" };
/** Playback counts 2 lines per slide, the file 1: followed by lines, a Check with the tip to match the template. */
const BY_LINES: Verdict = { code: "cues_by_lines", text: "Aligned by lines (Playback 2 per slide, the file 1 per slide)", following: "cues", advice: "Matching the template is still best: in Playback choose a ProPresenter 1 Line Template for this song", lines_per_slide: 1, playback_lines: 2, template: "ProPresenter 1 Line Template" };
const CHANGED: Verdict = { code: "arrangement_changed", text: "Arrangement was changed in ProPresenter" };
const NOT_IN_LIST: Verdict = { code: "not_in_playlist", text: "Not in the ProPresenter playlist" };
const SWITCH_OFF: Verdict = { code: CUES_OFF, text: "Switch is off" };
const LINK_GONE: Verdict = { code: "link_not_ready", text: "ProPresenter Link not ready" };
const LIBRARY_GONE: Verdict = { code: "library_not_ready", text: "Playback's Library Link not ready" };

const status = (over: Partial<FollowStatus>): FollowStatus => ({
  enabled: true, offset_ms: 0, state: "idle", message: "Nothing to follow", song: null, section: null, slide: null, hold_reason: null, can_try_again: false, songs: songs(CUES), ...over,
});
const playing = { song: { key: "k1", title: "Holy Forever" }, section: { index: 3, name: "Chorus" } };

export const FOLLOW_STATUS = {
  off: status({ enabled: false, state: "off", message: "Switch is off", songs: songs(SWITCH_OFF) }),
  following: status({ ...playing, state: "following", message: "Following by Playback cues", slide: 2, songs: songs(CUES, CUES, CUES, NO_CUES) }),
  held: status({ ...playing, state: "held", message: "Paused: someone changed the slide in ProPresenter", hold_reason: "Paused: someone changed the slide in ProPresenter", slide: 2, songs: songs(CUES, CUES, CUES, NO_CUES) }),
  paused: status({
    state: "paused", can_try_again: true, song: playing.song, slide: 6,
    message: "Paused: ProPresenter did not take 5 slide changes in a row (ProPresenter did not answer). Following starts again with the next song, or press Try again.",
    songs: songs(CUES, CUES, CUES, NO_CUES),
  }),
  noCues: status({ state: "ready", message: "Nothing to follow", songs: songs(NO_CUES, NO_CUES_TWO, CUES, NO_CUES_ODD) }),
  aligned: status({ ...playing, state: "following", message: "Following by Playback cues", slide: 2, songs: songs(CUES, ALIGNED, BY_LINES, CUES) }),
  mismatch: status({ state: "idle", songs: songs(MISMATCH, CHANGED, NOT_IN_LIST, CUES) }),
  notFollowed: status({ state: "not_followed", message: "Arrangement was changed in ProPresenter", song: { key: "k2", title: "Battle Belongs" }, songs: songs(CUES, CHANGED, CUES, NO_CUES) }),
  linkGone: status({ enabled: false, state: "off", message: "Switch is off", songs: songs(LINK_GONE) }),
  libraryGone: status({ enabled: false, state: "off", message: "Switch is off", songs: songs(LIBRARY_GONE) }),
};

/** What the gallery's follower does: it starts as `start` says; the switch swaps between `on` and `off` and nothing else is asked. */
export function useMadeUpFollow({ on, off = FOLLOW_STATUS.off, enabled, offsetMs = 0, error = null }: { on: FollowStatus; off?: FollowStatus; enabled: boolean; offsetMs?: number; error?: string | null }): PresenterFollowController {
  const [isOn, setOn] = useState(enabled);
  const [offset, setOffsetMs] = useState(offsetMs);
  const now = isOn ? on : off;
  return {
    status: { ...now, enabled: isOn, offset_ms: offset }, error, enabled: isOn, offsetMs: offset, pending: null,
    setEnabled: async (next) => { setOn(next); return true; },
    setOffset: async (ms) => { setOffsetMs(ms); return true; },
    tryAgain: () => undefined,
  };
}
