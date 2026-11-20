import { describe, expect, it } from "vitest";

import { addSong, guess, report, result } from "../test/propresenterLinkFixtures";
import {
  backupName, canTryAgain, CHANGE_FILE, END_OF_PLAYLIST, fileLabels, fileWords, guessAnnouncement, guessLine, guessNote, IMPORT_SONGS, IMPORTING, isStuck, mappingLines, neighbours,
  orderWords, ordinal, OUTCOME, playlistLabel, resultCounts, selectedWords, usingPhrase, SONG_STATUS, songSentence, STEP_STATE, STEP_WORDS, stuckReason, twoHeadersLine,
} from "./addSongsWording";

const sections = (...names: string[]) => ({ sections: names.map((name, i) => ({ name, start: i, end: i + 1 })) });

describe("what the preview says about a song", () => {
  it("copy: the new file and Playback's order, short", () => {
    const song = addSong({ ref_id: "1", title: "Holy Forever", action: "copy", file: "Holy Forever (2)", sections: 6 });
    expect(songSentence(song, sections("Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge", "Chorus"))).toBe(
      "New copy \u201CHoly Forever (2)\u201D with Playback's order (Verse 1, Chorus, Verse 2, Chorus, \u2026 (6 sections)).",
    );
    expect(songSentence(song, sections("Verse 1", "Chorus"))).toBe("New copy \u201CHoly Forever (2)\u201D with Playback's order (Verse 1, Chorus).");
    expect(songSentence(song, null)).toBe("New copy \u201CHoly Forever (2)\u201D with Playback's order (6 sections).");
    expect(songSentence(addSong({ ref_id: "1", title: "A", sections: 1 }), null)).toContain("(1 section)");
  });

  it("reuse: it is already there, and not added twice when it is in the playlist", () => {
    expect(songSentence(addSong({ ref_id: "1", title: "X", action: "reuse", file: "X (1)" }), null)).toBe("Already in ProPresenter as \u201CX (1)\u201D.");
    expect(songSentence(addSong({ ref_id: "1", title: "X", action: "reuse", file: "X (1)", already_in_playlist: true }), null)).toBe(
      "Already in ProPresenter as \u201CX (1)\u201D. It is already in this playlist, so it is not added again.",
    );
  });

  it("import then copy: loaded from search first", () => {
    const song = addSong({ ref_id: "1", title: "X", action: "import_then_copy", file: "X (1)", import_as: "X", sections: 2 });
    expect(songSentence(song, sections("Verse 1", "Chorus"))).toBe(
      "Will be loaded from ProPresenter search first (as \u201CX\u201D), then a new copy \u201CX (1)\u201D with Playback's order (Verse 1, Chorus).",
    );
    expect(songSentence({ ...song, import_as: null }, sections("Verse 1"))).toContain("Will be loaded from ProPresenter search first, then a new copy");
  });

  it("blocked: the reason, and a plain fallback; needs a choice: never a guess", () => {
    expect(songSentence(addSong({ ref_id: "1", title: "X", action: "blocked", reason: "The song has no group called Bridge." }), null)).toBe("Blocked: The song has no group called Bridge.");
    expect(songSentence(addSong({ ref_id: "1", title: "X", action: "blocked" }), null)).toBe("Blocked: StagePilot cannot make this song.");
    const choice = addSong({ ref_id: "1", title: "X", action: "needs_choice", reason: "Two files are this song.", choices: ["Songs / X.pro", "Songs / X (Live).pro"] });
    expect(songSentence(choice, null)).toBe("Needs a choice: Two files are this song. Files: \u201CSongs / X.pro\u201D, \u201CSongs / X (Live).pro\u201D.");
    expect(songSentence({ ...choice, reason: null, choices: [] }, null)).toContain("StagePilot does not guess");
  });

  it("only blocked and needs_choice are stuck (every action's chip is in addSongsStatus.test.ts)", () => {
    expect(["reuse", "copy", "import_then_copy", "needs_choice", "blocked"].map((action) => isStuck({ action: action as never }))).toEqual([false, false, false, true, true]);
  });
});

describe("mapping confidence lines", () => {
  const map = (how: string, line: string | null = null) => ({ section: "Verse 1", group: "Verse 1", how, line });
  it("says all matched exactly, or counts what did not", () => {
    expect(mappingLines({ mapping: [] })).toEqual([]);
    expect(mappingLines({ mapping: [map("full"), map("only")] })).toEqual(["All 2 sections matched exactly to the song's own groups."]);
    expect(mappingLines({ mapping: [map("full"), map("identical", "Two groups say the same thing; the first is used."), map("missing", "No group called Bridge.")] })).toEqual([
      "1 of 3 sections matched exactly, 1 to groups that say the same thing, 1 not matched.",
      "Two groups say the same thing; the first is used.",
      "No group called Bridge.",
    ]);
    expect(mappingLines({ mapping: [map("identical")] })[0]).toBe("0 of 1 section matched exactly, 1 to groups that say the same thing.");
  });
});

describe("the playlist side", () => {
  it("names the neighbours the songs land between", () => {
    expect(neighbours({ before: "A", after: "B" })).toBe("They go between \u201CA\u201D and \u201CB\u201D.");
    expect(neighbours({ before: "A", after: null })).toBe("They go after \u201CA\u201D, the last item.");
    expect(neighbours({ before: null, after: "B" })).toBe("They go before \u201CB\u201D, the first item.");
    expect(neighbours({ before: null, after: null })).toBeNull();
  });
  it("labels: a section is its own name, the last option is End of Playlist, and two headers of one name get the engine's line", () => {
    expect(END_OF_PLAYLIST).toBe("End of Playlist");
    expect(twoHeadersLine(" Worship ")).toBe("More than one header in this playlist is named \u201CWorship\u201D. Pick another place.");
    expect(playlistLabel({ name: "Youth", folder: "Special" })).toBe("Special / Youth");
    expect(playlistLabel({ name: "Sunday", folder: null })).toBe("Sunday");
  });
  it("orders: no sections known falls back to the count; blank names are Count Off", () => {
    expect(orderWords({ sections: 9 }, undefined)).toBe("9 sections");
    expect(orderWords({ sections: 2 }, sections("", "Verse 1"))).toBe("Count Off, Verse 1");
  });
});

describe("the job and its result", () => {
  it("has words for every step, step state, song status and outcome", () => {
    for (const step of ["check", "backup", "library", "import", "copy", "load", "playlist", "read_back"] as const) expect(STEP_WORDS[step]).toBeTruthy();
    for (const state of ["running", "done", "failed", "note"] as const) expect(STEP_STATE[state].word && STEP_STATE[state].mark).toBeTruthy();
    for (const status of ["added", "already_there", "skipped", "failed", "not_tried", "not_loaded"] as const) expect(SONG_STATUS[status].word && SONG_STATUS[status].mark).toBeTruthy();
    for (const outcome of ["done", "done_with_differences", "needs_library_open", "stopped", "song_playing", "live_on_screen", "playlist_refused", "playlist_failed", "backup_failed", "not_reachable", "nothing_to_do"] as const) {
      expect(OUTCOME[outcome].heading).toBeTruthy();
      expect(OUTCOME[outcome].mark).toBeTruthy();
    }
  });
  it("offers Try again after a failure, never after success", () => {
    expect(canTryAgain("needs_library_open")).toBe(true);
    expect(canTryAgain("song_playing")).toBe(true);
    expect(canTryAgain("live_on_screen")).toBe(true);
    expect(canTryAgain("done")).toBe(false);
    expect(canTryAgain("done_with_differences")).toBe(false);
    expect(canTryAgain("nothing_to_do")).toBe(false);
  });
  it("shows only the file name of a backup, and counts what happened", () => {
    expect(backupName("/home/church/.config/StagePilot/propresenter-playlist-backups/Sunday-1.json")).toBe("Sunday-1.json");
    expect(backupName("C:\\Users\\church\\backups\\Sunday-1.json")).toBe("Sunday-1.json");
    expect(backupName(null)).toBeNull();
    expect(resultCounts(report({ songs: [result("1", "A", "added"), result("2", "B", "added"), result("3", "C", "skipped"), result("4", "D", "failed"), result("5", "E", "not_loaded")] }))).toBe("2 added \u00B7 1 skipped \u00B7 1 failed \u00B7 1 not loaded");
  });
});

describe("the one button and the count", () => {
  it("is Import songs to ProPresenter, Importing… while it works, and the result says Done and Added", () => {
    expect(IMPORT_SONGS).toBe("Import songs to ProPresenter");
    expect(IMPORTING).toBe("Importing\u2026");
    expect(OUTCOME.done.heading).toBe("Done");
    expect(SONG_STATUS.added.word).toBe("Added");
    expect(selectedWords(3, 4)).toBe("3 of 4 selected");
  });
});

describe("sections that were guessed", () => {
  it("reads ordinals the natural way", () => {
    expect([1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101, 111].map(ordinal)).toEqual(["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd", "101st", "111th"]);
  });

  it("names the use, the group and which rule picked it", () => {
    expect(guessLine(guess({ section: "Interlude", position: 2, group_index: 2, rule: "in_order" }))).toBe("Interlude (2nd use) uses the 2nd \u201CInterlude\u201D group");
    expect(guessLine(guess())).toBe(
      "Interlude (3rd use) uses the 2nd \u201CInterlude\u201D group (Playback uses it once more than the song has groups, so the last group is used again)",
    );
    expect(guessLine(guess({ position: 5, group_index: 1, rule: "repeat_in_turn" }))).toBe(
      "Interlude (5th use) uses the 1st \u201CInterlude\u201D group (Playback uses it more often than the song has groups, so the groups are used in turn)",
    );
    expect(guessLine(guess({ section: "Count Off", position: 1, group_name: "", group_index: 3, rule: "in_order" }))).toBe("Count Off (1st use) uses the 3rd blank-named group");
  });

  it("is one short note per song under 'Check in ProPresenter:', and none when nothing was guessed", () => {
    expect(guessNote([])).toBeNull();
    expect(guessNote(undefined)).toBeNull();
    const note = guessNote([guess({ section: "Count Off", position: 1, group_name: "", group_index: 1, rule: "in_order" }), guess()]);
    expect(note?.heading).toBe("Check in ProPresenter:");
    expect(note?.lines).toHaveLength(2);
    expect(note?.lines[0]).toBe("Count Off (1st use) uses the 1st blank-named group");
  });

  it("is read out after the job, per song, only for songs that have guesses", () => {
    const withGuess = { ...result("1", "How Awesome", "added"), guesses: [guess()] };
    expect(guessAnnouncement(report({ songs: [result("2", "Plain", "added")] }))).toBe("");
    expect(guessAnnouncement(null)).toBe("");
    expect(guessAnnouncement(report({ songs: [withGuess, result("2", "Plain", "added")] }))).toMatch(/^Check in ProPresenter: How Awesome: Interlude \(3rd use\) uses the 2nd/);
  });

  it("counts guessed sections apart from the ones that did not match", () => {
    const song = addSong({
      ref_id: "1", title: "How Awesome",
      mapping: [
        { section: "Verse 1", group: "Verse 1", how: "full", line: null },
        { section: "Interlude", group: "Interlude", how: "guessed", line: guess().text },
        { section: "Interlude", group: "Interlude", how: "guessed", line: guess().text },
      ],
    });
    const lines = mappingLines(song);
    expect(lines[0]).toBe("1 of 3 sections matched exactly, 2 guessed (check them in ProPresenter).");
    expect(lines).not.toContain("1 of 3 sections matched exactly, 2 not matched.");
    expect(lines[1]).toContain("Interlude (3rd use) uses the 2nd");
  });
});

describe("the one button: what stops it after the preview, in words", () => {
  const plan = (over: Partial<Parameters<typeof stuckReason>[0]> = {}) => ({
    songs: [addSong({ ref_id: "1", title: "Holy Forever" }), addSong({ ref_id: "2", title: "Battle Belongs", action: "blocked", reason: "x" }), addSong({ ref_id: "3", title: "Gratitude", action: "needs_choice", reason: "y" })],
    stuck: 2, to_add: 1, can_apply: false, playlist: { blocked: null }, ...over,
  });

  it("is nothing when the preview is clean, so the songs go ahead", () => {
    expect(stuckReason(plan({ songs: [addSong({ ref_id: "1", title: "A" })], stuck: 0, can_apply: true }), true)).toBeNull();
  });

  it("says in a few words how many cannot be imported and what to do, with and without something left to add", () => {
    expect(stuckReason(plan(), false)).toBe("2 can't be imported yet");
    expect(stuckReason(plan(), true)).toBe("2 can't be imported yet: skip them, or fix them and try again");
    expect(stuckReason(plan({ to_add: 0, stuck: 1, songs: [addSong({ ref_id: "2", title: "Battle Belongs", action: "blocked", reason: "x" })] }), true)).toBe("1 can't be imported yet: fix them, or select other songs");
  });

  it("says a blocked place first, and nothing-to-add when every song is already there", () => {
    expect(stuckReason(plan({ playlist: { blocked: "That header is in the playlist twice." } }), true)).toBe("Playlist is blocked");
    expect(stuckReason(plan({ songs: [], stuck: 0, to_add: 0, can_apply: false }), true)).toBe("Nothing to import: all in playlist");
  });
});

describe("a title-only add", () => {
  it("has a sentence for a song that goes in as it is", () => {
    const song = addSong({ ref_id: "t1", title: "Plain", action: "as_is", file: "Plain", sections: 0, key: null });
    expect(songSentence(song, null)).toBe("Added as it is in ProPresenter as \u201CPlain\u201D: no copy, no new arrangement, no key change.");
    expect(songSentence({ ...song, already_in_playlist: true }, null)).toContain("It is already in this playlist, so it is not added again.");
    expect(isStuck(song)).toBe(false);
  });
});

describe("the files of a song with more than one", () => {
  it("a file is named without its library and without .pro", () => {
    expect(fileWords("Songs / Firm Foundation (Live).pro")).toBe("Firm Foundation (Live)");
    expect(fileWords("Firm Foundation.PRO")).toBe("Firm Foundation");
    expect(CHANGE_FILE).toBe("Change");
  });

  it("the buttons add the library only for a name two libraries share", () => {
    expect(fileLabels(["Songs / Firm Foundation (Live).pro", "Songs / Firm Foundation.pro"])).toEqual(["Firm Foundation (Live)", "Firm Foundation"]);
    expect(fileLabels(["Songs / Firm Foundation.pro", "Other / Firm Foundation.pro", "Songs / Other Song.pro"])).toEqual(["Songs / Firm Foundation", "Other / Firm Foundation", "Other Song"]);
  });

  it("the quiet phrase is six words at most for an ordinary name", () => {
    expect(usingPhrase("Songs / Firm Foundation (Live).pro")).toBe("Using \u201CFirm Foundation (Live)\u201D");
    expect(usingPhrase("Songs / Firm Foundation (Live).pro").split(" ").length).toBeLessThanOrEqual(6);
  });
});
