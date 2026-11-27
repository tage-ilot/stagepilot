import { describe, expect, it } from "vitest";

import { addSongsRows, followByRow, planNameRows } from "./components/addSongsSongs";
import { followLine, followLock, hasFix, hasNoCues, libraryLinkState, lockWords, shortLine, songChipWords, songPhrase, templateFix } from "./presenterFollow";
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

  it("never says Following while Playback is stopped or paused: the song that was shown waits for the next Play", () => {
    const song = { key: "k1", title: "Holy Forever" };
    // A song loaded and not played yet keeps its own words; one that was shown (a slide is known) and stopped says it waits.
    expect(followLine(followStatus({ state: "ready", song, slide: null }), true, null).text).toBe("Ready: Holy Forever starts when Playback plays it");
    const stopped = followLine(followStatus({ state: "ready", song, slide: 6, message: "Ready: waiting for Playback to play the song" }), true, null);
    expect(stopped).toMatchObject({ text: "Ready: waiting for Playback to play the song", state: "ready", tone: "idle" });
    expect(stopped.text).not.toMatch(/Following/);
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
    // No cues: ONE notice chip in the SPEC's words, the info look of the other fallback chips and never an error (the backend's long words stay in the hover text).
    expect(songChipWords(bars!)).toEqual({ word: "No cues: following by bars", mark: "~", tone: "info" });
    // Followed by bars with nothing to fix (cues unusable, no advice) keeps the backend's own words.
    expect(songChipWords({ code: "bars", text: "Following by bars (no Playback cues)" })).toEqual({ word: "Following by bars (no Playback cues)", mark: "~", tone: "info" });
    expect(songChipWords(mismatch!)).toMatchObject({ mark: "!", tone: "warning" });
    expect(songChipWords(changed!)).toMatchObject({ word: "Arrangement was changed in ProPresenter", mark: "!", tone: "danger" });
    expect(songChipWords(off ?? followSongs()[0]!)).toBeDefined();
  });

  it("marks a song followed by cues with extra slides in the file as one to check, and carries the template that fits the file", () => {
    const [aligned, twoLine] = followSongs(
      { code: "cues_aligned", text: "Following by Playback cues, aligned with extra slides (check the first slide of Intro)" },
      { ...NO_CUES_SONG, lines_per_slide: 2, template: "ProPresenter 2 Line Template", advice: "No cues: in Playback choose the ProPresenter 2 Line Template for this song" },
    );
    expect(songChipWords(aligned!)).toEqual({ word: "Following by Playback cues, aligned with extra slides (check the first slide of Intro)", mark: "!", tone: "warning" });
    expect(hasNoCues(aligned!)).toBe(false);
    expect(songPhrase(aligned!)).toBeNull();
    expect(hasNoCues(twoLine!)).toBe(true);
    expect(twoLine).toMatchObject({ lines_per_slide: 2, template: "ProPresenter 2 Line Template" });
  });

  it("marks a song followed by lines (Playback and the file count lines per slide differently) as one to check, with the two numbers and the one-sentence tip", () => {
    const [byLines] = followSongs({ code: "cues_by_lines", text: "Aligned by lines (Playback 2 per slide, the file 1 per slide)", advice: "Matching the template is still best: in Playback choose a ProPresenter 1 Line Template for this song", lines_per_slide: 1, playback_lines: 2, template: "ProPresenter 1 Line Template" });
    expect(songChipWords(byLines!)).toEqual({ word: "Aligned by lines (Playback 2 per slide, the file 1 per slide)", mark: "!", tone: "warning" });
    expect(byLines).toMatchObject({ following: "cues", lines_per_slide: 1, playback_lines: 2 });
    expect(hasNoCues(byLines!)).toBe(false);
    expect(songPhrase(byLines!)).toBeNull();
    // The tip: matching the template is still best, named like the other fixes.
    expect(hasFix(byLines!)).toBe(true);
    expect(templateFix(byLines!)).toContain("Lyrics Bus, ProPresenter 1 Line Template, Update.");
  });

  it("says No cues only for a song followed by bars for want of cues, and a mismatch is followed by bars", () => {
    const [noCues, plain, mismatch] = followSongs(NO_CUES_SONG, {}, { code: "cues_do_not_match", text: "Cues do not match the file (cues go to slide 30, file has 54 cues)", advice: null });
    expect(hasNoCues(noCues!)).toBe(true);
    expect(hasNoCues(plain!)).toBe(false);
    expect(hasNoCues(mismatch!)).toBe(false);
    expect(songPhrase(mismatch!)).toBe("Followed by bars");
    expect(songPhrase(plain!)).toBeNull();
  });

  it("names the template that fits the file in the one sentence of the fix (a mismatch has the same fix)", () => {
    const [noCues, twoLine, mismatch, odd] = followSongs(
      { ...NO_CUES_SONG, template: "ProPresenter 1 Line Template" },
      { ...NO_CUES_SONG, lines_per_slide: 2, template: "ProPresenter 2 Line Template" },
      { code: "cues_do_not_match", text: "Cues do not match the file (cues go to slide 30, file has 54 cues)", advice: "In Playback choose the ProPresenter 2 Line Template for this song", template: "ProPresenter 2 Line Template" },
      { ...NO_CUES_SONG, lines_per_slide: 3, template: null },
    );
    const cues = followSongs()[0];
    expect(templateFix(noCues!)).toBe("In Playback: Edit, the \u201C...\u201D on the song, MIDI OUT, Lyrics Bus, ProPresenter 1 Line Template, Update.");
    expect(templateFix(twoLine!)).toContain("Lyrics Bus, ProPresenter 2 Line Template, Update.");
    expect(templateFix(odd!)).toContain("Lyrics Bus, a ProPresenter Line Template that matches the file, Update.");
    expect([noCues, twoLine, mismatch, odd, cues].map((song) => hasFix(song!))).toEqual([true, true, true, true, false]);
  });
});

describe("what locks the switch", () => {
  it("is the Library Link's state: not connected, an old copy, or current", () => {
    expect(libraryLinkState(null)).toBe("missing");
    expect(libraryLinkState({ active: false, stale: false })).toBe("missing");
    expect(libraryLinkState({ active: true, stale: true })).toBe("stale");
    expect(libraryLinkState({ active: true, stale: false })).toBe("ready");
  });

  it("tells the pending keychain prompt (the helper's needs_key) from a Library Link that was never set up", () => {
    expect(libraryLinkState({ active: false, stale: false, helper: { state: "needs_key" } })).toBe("needs_key");
    expect(libraryLinkState({ active: false, stale: false, helper: { state: "not_installed" } })).toBe("missing");
    expect(libraryLinkState({ active: false, stale: false, helper: { state: "unreachable" } })).toBe("missing");
    const keychain = followLock(true, "needs_key")!;
    expect(lockWords(keychain, false)).toBe("Needs the Playback Library Link");
    expect(keychain.where).toContain("Allow the keychain prompt");
    expect(keychain.where).not.toBe(followLock(true, "missing")!.where);
  });

  it("needs the Presenter Link AND the Library Link with current data, and names ONE reason with where to set it up", () => {
    expect(followLock(true, "ready")).toBeNull();
    const link = followLock(false, "ready")!;
    expect(lockWords(link, false)).toBe("Needs the Presenter Link");
    expect(link.where).toContain("Configuration \u2192 Presentation");
    const library = followLock(true, "missing")!;
    expect(lockWords(library, false)).toBe("Needs the Playback Library Link");
    expect(library.where).toContain("Configuration \u2192 Playback");
    expect(lockWords(followLock(true, "stale")!, false)).toBe("Needs current data from the Playback Library Link");
    expect(lockWords(followLock(false, "missing")!, false)).toBe("Needs the Presenter Link and the Playback Library Link");
  });

  it("says Waiting for ... once the switch was saved as on", () => {
    expect(lockWords(followLock(true, "missing")!, true)).toBe("Waiting for the Playback Library Link");
    expect(lockWords(followLock(false, "ready")!, true)).toBe("Waiting for the Presenter Link");
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
