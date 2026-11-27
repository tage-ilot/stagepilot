import { describe, expect, it } from "vitest";

import { addSongsRows, followByRow, planNameRows } from "./components/addSongsSongs";
import { followLine, hasNoCues, shortLine, lockedReason, LOCKED_LIBRARY, LOCKED_LINK, NO_CUES_FIX, songChipWords, songPhrase } from "./presenterFollow";
import { libraryPlan, librarySong, librarySongOf } from "./test/libraryFixtures";
import { followSongs, followStatus, NO_CUES_SONG } from "./test/presenterFollowFixtures";

describe("the status line", () => {
  it("says Off whenever the switch is off, whatever the status holds", () => {
    expect(followLine(followStatus({ state: "following" }), false, null)).toMatchObject({ text: "Off", state: "off" });
    expect(followLine(null, false, "x")).toMatchObject({ text: "Off" });
  });

  it("says Following: the section and the slide the way ProPresenter's window counts (from 1)", () => {
    const line = followLine(followStatus({ state: "following", section: { index: 3, name: "Chorus" }, slide: 2 }), true, null);
    expect(line).toMatchObject({ text: "Following: Chorus, slide 3", tone: "success", mark: "\u2713" });
    // The blank first group is Playback's Count Off; a status with no slide yet names the section only.
    expect(followLine(followStatus({ state: "following", section: { index: 0, name: "" }, slide: null }), true, null).text).toBe("Following: Count Off");
  });

  it("says in words that a person's click holds the song, and that failures paused it (both with a mark)", () => {
    const held = followLine(followStatus({ state: "held", message: "Paused: someone changed the slide in ProPresenter", hold_reason: "Paused: someone changed the slide in ProPresenter" }), true, null);
    expect(held).toMatchObject({ text: "Paused: someone changed the slide in ProPresenter", mark: "!", tone: "warning" });
    const paused = followLine(followStatus({ state: "paused", message: "Paused: ProPresenter did not take 5 slide changes in a row (x)." }), true, null);
    expect(paused).toMatchObject({ mark: "!", tone: "danger" });
    expect(paused.text).toMatch(/^Paused: ProPresenter did not take 5/);
  });

  it("explains a song that cannot be followed, and waits quietly while nothing is known", () => {
    expect(followLine(followStatus({ state: "not_followed", message: "Arrangement was changed in ProPresenter" }), true, null).text).toBe("Not following: Arrangement was changed in ProPresenter");
    expect(followLine(null, true, null).text).toBe("Checking\u2026");
    expect(followLine(null, true, "The state could not be read.")).toMatchObject({ text: "The state could not be read.", state: "error" });
    expect(followLine(followStatus({ state: "off" }), true, null).text).toBe("Turning on\u2026");
    expect(followLine(followStatus({ state: "idle", message: "Playback is not on a song" }), true, null).text).toBe("Playback is not on a song");
  });
});

describe("the short line for a folded header", () => {
  it("keeps the first sentence and leaves out the reason in brackets", () => {
    expect(shortLine("Paused: ProPresenter did not take 5 slide changes in a row (ProPresenter did not answer). Following starts again with the next song, or press Try again.")).toBe("Paused: ProPresenter did not take 5 slide changes in a row");
    expect(shortLine("Following: Chorus, slide 3")).toBe("Following: Chorus, slide 3");
    expect(shortLine("Paused: someone changed the slide in ProPresenter")).toBe("Paused: someone changed the slide in ProPresenter");
  });
});

describe("a song's chip", () => {
  it("wears the backend's words with a mark that says the state without colour", () => {
    const [cues, bars, mismatch, changed, , , off] = followSongs({}, NO_CUES_SONG, { code: "cues_do_not_match", text: "Cues do not match the file (cues go to slide 30, file has 54 cues)" }, { code: "arrangement_changed", text: "Arrangement was changed in ProPresenter" }, { code: "not_in_playlist", text: "Not in the ProPresenter playlist" }, { code: "link_not_ready", text: "ProPresenter Link not ready" }, { code: "switch_off", text: "Switch is off" });
    expect(songChipWords(cues!)).toEqual({ word: "Following by Playback cues", mark: "\u2713", tone: "success" });
    expect(songChipWords(bars!)).toMatchObject({ word: "Following by bars (no Playback cues)", mark: "~", tone: "info" });
    expect(songChipWords(mismatch!)).toMatchObject({ mark: "!", tone: "warning" });
    expect(songChipWords(changed!)).toMatchObject({ word: "Arrangement was changed in ProPresenter", mark: "!", tone: "danger" });
    expect(songChipWords(off ?? followSongs()[0]!)).toBeDefined();
  });

  it("says No cues only for a song followed by bars for want of cues, and a mismatch is followed by bars", () => {
    const [noCues, plain, mismatch] = followSongs(NO_CUES_SONG, {}, { code: "cues_do_not_match", text: "Cues do not match the file (cues go to slide 30, file has 54 cues)", advice: null });
    expect(hasNoCues(noCues!)).toBe(true);
    expect(hasNoCues(plain!)).toBe(false);
    expect(hasNoCues(mismatch!)).toBe(false);
    expect(songPhrase(mismatch!)).toBe("Followed by bars");
    expect(songPhrase(plain!)).toBeNull();
    expect(NO_CUES_FIX).toBe("In Playback: Edit, the \u201C...\u201D on the song, MIDI OUT, Lyrics Bus, ProPresenter 1 Line Template, Update.");
  });
});

describe("what locks the switch", () => {
  it("is the Presenter Link first, then Playback's Library Link, else nothing", () => {
    expect(lockedReason(false, false)).toBe(LOCKED_LINK);
    expect(lockedReason(false, true)).toBe(LOCKED_LINK);
    expect(lockedReason(true, false)).toBe(LOCKED_LIBRARY);
    expect(lockedReason(true, true)).toBeNull();
  });
});

describe("followByRow", () => {
  const plan = libraryPlan(["Holy Forever", "Battle Belongs"].map((title, i) => librarySongOf(title, i + 1, { library: librarySong({ title, position: i + 1, setlist_song_id: 100 + i }) })));

  it("matches a Playback row by the plan song's key", () => {
    const rows = addSongsRows(plan);
    const found = followByRow(rows, followSongs());
    expect([...found.values()].map((song) => song.key)).toEqual(["k1", "k2"]);
  });

  it("matches by title when the status knows the plan's own keys (the switch is off), and keeps two songs of one title apart", () => {
    const status = followSongs().map((song) => ({ ...song, key: `pc-${song.key}` }));
    const rows = addSongsRows(plan);
    expect([...followByRow(rows, status).values()].map((song) => song.title)).toEqual(["Holy Forever", "Battle Belongs"]);
    const twins = planNameRows({ songs: ["Amen", "Amen"].map((title, order) => ({ id: `s${order}`, title, duration_seconds: 1, order, is_generic: false, source_song_id: null })) });
    const verdicts = [{ ...followSongs()[0]!, key: "s0", title: "Amen" }, { ...followSongs()[1]!, key: "s1", title: "Amen", code: "bars" as const, text: "Following by bars (no Playback cues)" }];
    expect([...followByRow(twins, verdicts).values()].map((song) => song.key)).toEqual(["s0", "s1"]);
  });

  it("finds nothing without a status", () => {
    expect(followByRow(addSongsRows(plan), undefined).size).toBe(0);
    expect(followByRow(addSongsRows(plan), []).size).toBe(0);
  });
});
