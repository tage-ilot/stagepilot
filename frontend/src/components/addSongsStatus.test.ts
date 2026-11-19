import { describe, expect, it } from "vitest";

import { addPlan, addSong, guess, report, result } from "../test/propresenterLinkFixtures";
import type { AddSongsSong } from "../propresenterLink";
import { differenceLine, guessesPhrase, guessPhrase, isToAdd, placeHeading, resultHeading, resultWord, songDetail, songStatus, STATUS, stoppedAt, summaryLine, type StatusKey } from "./addSongsStatus";

const song = (over: Partial<AddSongsSong> = {}) => addSong({ ref_id: "1", title: "Holy Forever", ...over });
const MISSING = [{ section: "Bridge 2", group: null, how: "missing", line: "No group called Bridge 2." }, { section: "Bridge 2", group: null, how: "missing", line: null }];

/** Every status the preview can show, with the song the engine would send for it. */
const CASES: { name: string; song: AddSongsSong; key: StatusKey; chip: string; phrase: string | null }[] = [
  { name: "already in the playlist", song: song({ action: "reuse", file: "Holy Forever (1)", already_in_playlist: true }), key: "in_playlist", chip: "In playlist", phrase: null },
  { name: "a copy exists and will be added", song: song({ action: "reuse", file: "Holy Forever (1)" }), key: "ready", chip: "Ready", phrase: "Copy exists" },
  { name: "a title-only song goes in as it is", song: song({ action: "as_is", file: "Holy Forever" }), key: "ready", chip: "Ready", phrase: "Used as is" },
  { name: "a new copy, named by its suffix", song: song({ action: "copy", file: "Holy Forever (2)" }), key: "new_copy", chip: "New copy", phrase: "Playback order, \u201C(2)\u201D" },
  { name: "a new copy whose name does not start with the title", song: song({ action: "copy", file: "Other (1)" }), key: "new_copy", chip: "New copy", phrase: "Playback order, \u201COther (1)\u201D" },
  { name: "not in ProPresenter, importable", song: song({ action: "import_then_copy", file: "Holy Forever (1)", import_as: "Holy Forever" }), key: "will_import", chip: "Will import", phrase: null },
  { name: "not in ProPresenter, no Mac", song: song({ action: "blocked", file: null, reason: "ProPresenter does not have this song and it cannot be imported from here. Importing a song needs a Mac; this computer is not one." }), key: "cant_import", chip: "Can't import", phrase: "Needs a Mac" },
  { name: "not in ProPresenter, cannot import for another reason", song: song({ action: "blocked", file: null, reason: "ProPresenter does not have this song and it cannot be imported from here. Allow screen control first." }), key: "cant_import", chip: "Can't import", phrase: "Import not possible here" },
  { name: "several files match", song: song({ action: "needs_choice", file: null, reason: "More than one file in ProPresenter is this song and StagePilot does not guess which to use.", choices: ["Songs / Holy Forever (Live).pro", "Songs / Holy Forever.pro"] }), key: "pick_one", chip: "Pick one", phrase: null },
  { name: "sections that have no group", song: song({ action: "blocked", file: null, mapping: MISSING, reason: "These sections have no group with that name in the song: Bridge 2. Nothing is made for this song." }), key: "cant_map", chip: "Can't map", phrase: "Missing: Bridge 2" },
  { name: "sections that have no group (only the reason says which)", song: song({ action: "blocked", file: null, reason: "These sections have no group with that name in the song: Tag, Outro. Nothing is made for this song." }), key: "cant_map", chip: "Can't map", phrase: "Missing: Tag, Outro" },
  { name: "no MultiTracks number", song: song({ action: "blocked", file: null, content_id: null, reason: "Playback did not give this song's MultiTracks number, so StagePilot cannot find it in ProPresenter." }), key: "no_mtid", chip: "No mtID", phrase: null },
  { name: "a rental that ran out", song: song({ action: "blocked", file: null, reason: "The rental of this song has run out in Playback." }), key: "cant_import", chip: "Can't import", phrase: "Rental ended" },
  { name: "no arrangement", song: song({ action: "blocked", file: null, reason: "Playback gave no arrangement (no sections) for this song, so there is nothing to copy." }), key: "cant_import", chip: "Can't import", phrase: "No arrangement" },
  { name: "a file that cannot be read", song: song({ action: "blocked", file: null, reason: "ProPresenter's file for this song could not be read, so nothing can be copied from it." }), key: "cant_import", chip: "Can't import", phrase: "File unreadable" },
  { name: "a Planning Center name ProPresenter does not have", song: song({ action: "blocked", file: null, reason: "ProPresenter does not have \"Holy Forever\" in its Songs library. Playback's data is needed to import it." }), key: "cant_import", chip: "Can't import", phrase: "Needs Playback data" },
  { name: "a reason it does not know", song: song({ action: "blocked", file: null, reason: "Something new." }), key: "cant_import", chip: "Can't import", phrase: null },
  { name: "a guess", song: song({ action: "copy", file: "Holy Forever (1)", guesses: [guess()] }), key: "check", chip: "Check", phrase: "Interlude (3rd use): 2nd group" },
  { name: "two guesses", song: song({ action: "copy", file: "Holy Forever (1)", guesses: [guess(), guess({ section: "Tag", position: 1, group_name: "Tag 2", group_index: 1 })] }), key: "check", chip: "Check", phrase: "Interlude (3rd use): 2nd group +1" },
];

describe("every status of the preview is ONE chip and at most one short phrase", () => {
  it.each(CASES)("$name: $chip", ({ song: made, key, chip, phrase }) => {
    const status = songStatus(made);
    expect(status.key).toBe(key);
    expect(status.word).toBe(chip);
    expect(status.phrase).toBe(phrase);
    expect(status.mark).not.toBe("");
  });

  it("keeps every phrase short: seven words at most, never a sentence", () => {
    for (const { song: made } of CASES) {
      const phrase = songStatus(made).phrase;
      if (phrase === null) continue;
      expect(phrase.split(/\s+/).length, phrase).toBeLessThanOrEqual(7);
      expect(phrase, phrase).not.toMatch(/[.!?]$/);
      expect(phrase, phrase).not.toMatch(/[.!?]\s/);
    }
  });

  it("every chip word is short, and is one of the documented words", () => {
    const words = Object.values(STATUS).map((entry) => entry.word);
    expect(words).toEqual(["In playlist", "Ready", "New copy", "Will import", "Can't import", "Pick one", "Can't map", "No mtID", "Check", "Skipped"]);
    for (const word of words) expect(word.split(" ").length).toBeLessThanOrEqual(2);
  });

  it("uses the status tones: green for done, teal for news, red only for what stops a song, grey for skipped", () => {
    expect(Object.fromEntries(Object.entries(STATUS).map(([key, entry]) => [key, entry.tone]))).toEqual({
      in_playlist: "success", ready: "info", new_copy: "info", will_import: "info", cant_import: "danger", pick_one: "warning", cant_map: "danger", no_mtid: "danger", check: "info", skipped: "idle",
    });
  });

  it("Pick one lists the files, one each, and no sentence", () => {
    const status = songStatus(CASES.find((entry) => entry.key === "pick_one")!.song);
    expect(status.files).toEqual(["Songs / Holy Forever (Live).pro", "Songs / Holy Forever.pro"]);
    expect(status.phrase).toBeNull();
  });

  it("keeps the long sentences, and the guess lines, for the hover text and the screen reader", () => {
    const stopped = songStatus(CASES.find((entry) => entry.name === "not in ProPresenter, no Mac")!.song);
    expect(stopped.detail).toContain("Blocked: ProPresenter does not have this song and it cannot be imported from here. Importing a song needs a Mac");
    const guessed = songStatus(CASES.find((entry) => entry.name === "a guess")!.song);
    expect(guessed.detail).toContain("Interlude (3rd use) uses the 2nd");
    expect(songDetail(song({ action: "copy", file: "Holy Forever (1)" }))).toMatch(/^New copy/);
  });

  it("a song left out of the import is Skipped, whatever it was", () => {
    expect(songStatus(song({ action: "blocked", reason: "x" }), true)).toMatchObject({ key: "skipped", word: "Skipped", phrase: null });
  });

  it("a song already in the playlist is In playlist even with a guess (nothing is added)", () => {
    expect(songStatus(song({ action: "copy", already_in_playlist: true, guesses: [guess()] })).key).toBe("in_playlist");
  });
});

describe("the guess phrase", () => {
  it("is the section, its use, and the group, short; a group of another name is named", () => {
    expect(guessPhrase(guess())).toBe("Interlude (3rd use): 2nd group");
    expect(guessPhrase(guess({ group_name: "Tag" }))).toBe("Interlude (3rd use): 2nd \u201CTag\u201D group");
    expect(guessesPhrase([])).toBeNull();
  });
});

describe("the counts line", () => {
  it("is one line of counts in a fixed order, with zero counts left out", () => {
    expect(summaryLine(["new_copy", "in_playlist", "in_playlist", "cant_import", "pick_one"])).toBe("1 to add \u00B7 2 in playlist \u00B7 1 can't import \u00B7 1 pick one");
    expect(summaryLine(["ready", "check", "will_import", "cant_map", "no_mtid"])).toBe("3 to add \u00B7 1 can't map \u00B7 1 no mtID");
  });
  it("always says how many are added, even none", () => {
    expect(summaryLine(["in_playlist", "in_playlist"])).toBe("0 to add \u00B7 2 in playlist");
    expect(summaryLine([])).toBe("0 to add");
    expect(summaryLine(["skipped"])).toBe("0 to add");
  });
  it("counts as added only what the import would add", () => {
    expect((["ready", "new_copy", "will_import", "check"] as StatusKey[]).every(isToAdd)).toBe(true);
    expect((["in_playlist", "cant_import", "cant_map", "no_mtid", "pick_one", "skipped"] as StatusKey[]).some(isToAdd)).toBe(false);
  });
  it("heads the preview with the place instead of a sentence", () => {
    const plan = addPlan([]);
    expect(placeHeading(plan, "Worship", "End of Playlist")).toBe("Sunday Morning Service \u203A Worship");
    expect(placeHeading(plan, null, "End of Playlist")).toBe("Sunday Morning Service \u203A End of Playlist");
  });
});

describe("the result in a few words", () => {
  const done = (count: number) => Array.from({ length: count }, (_, i) => result(String(i), `Song ${i}`, "added"));
  it("says Done with the count, the differences, or where it stopped", () => {
    expect(resultHeading(report({ songs: done(3) })).text).toBe("Done: 3 added");
    expect(resultHeading(report({ songs: [result("1", "A", "already_there")] })).text).toBe("Done: nothing added");
    expect(resultHeading(report({ outcome: "done_with_differences", songs: done(3), differences: ["x"] })).text).toBe("3 added, 1 differs");
    expect(resultHeading(report({ outcome: "done_with_differences", songs: done(3), differences: ["x", "y"] })).text).toBe("3 added, 2 differ");
    const failed = report({ outcome: "stopped", songs: [result("1", "Holy Forever", "added"), result("2", "Battle Belongs", "failed")] });
    expect(resultHeading(failed).text).toBe("Stopped: Can't import \u201CBattle Belongs\u201D");
    expect(resultHeading(report({ outcome: "stopped", songs: [] })).text).toBe("Stopped");
  });
  it("says the whole-job stops in one short line", () => {
    expect(resultHeading(report({ outcome: "live_on_screen" })).text).toBe("Live on screen: clear it, then try again");
    expect(resultHeading(report({ outcome: "song_playing" })).text).toBe("Song playing: wait");
    for (const outcome of ["needs_library_open", "playlist_refused", "playlist_failed", "backup_failed", "not_reachable", "nothing_to_do"] as const) {
      const text = resultHeading(report({ outcome })).text;
      expect(text.split(/\s+/).length, text).toBeLessThanOrEqual(7);
      expect(text).not.toMatch(/\.$/);
    }
  });
  it("names a failed song's kind: Can't map for an unmapped one", () => {
    expect(resultWord({ status: "failed", code: "unmapped" })).toBe("Can't map");
    expect(resultWord({ status: "failed", code: null })).toBe("Can't import");
    expect(resultWord({ status: "failed", code: "wrong_song" })).toBe("Wrong song");
    expect(resultWord({ status: "added", code: null })).toBe("");
    expect(stoppedAt(report({ songs: [result("1", "A", "added")] }))).toBeNull();
  });
  it("shortens the read-back's lines to `Item 7: duration 80 \u2192 0`, and leaves what it does not know", () => {
    expect(differenceLine("Item 7 \"Gratitude\": its duration reads 0 now, it was 80.")).toBe("Item 7: duration 80 \u2192 0");
    expect(differenceLine("Item 3 \"Worship\": it is a song now, not a header.")).toBe("Item 3: header \u2192 song");
    expect(differenceLine("Item 2 \"Welcome\": it reads \"Welcome!\" now.")).toBe("Item 2: now \u201CWelcome!\u201D");
    expect(differenceLine("Item 5 \"Holy Forever (1)\": ProPresenter shows the arrangement \"Default\" instead of \"Playback\".")).toBe("Item 5: arrangement Playback \u2192 Default");
    expect(differenceLine("The playlist has 9 items now; 8 were written.")).toBe("Items: 8 written \u2192 9 now");
    expect(differenceLine("Something else happened.")).toBe("Something else happened");
  });
});
