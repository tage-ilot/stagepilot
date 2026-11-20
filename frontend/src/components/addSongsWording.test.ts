import { describe, expect, it } from "vitest";

import { addSong, report, result } from "../test/propresenterLinkFixtures";
import {
  ACTION_STAMP, afterHeaderLabel, backupName, canTryAgain, isStuck, mappingLines, neighbours, orderWords, OUTCOME, playlistLabel, playlistSentence,
  resultCounts, SONG_STATUS, songSentence, STEP_STATE, STEP_WORDS,
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

  it("every action has a stamp with a mark and a word, and only blocked and needs_choice are stuck", () => {
    for (const action of ["reuse", "copy", "import_then_copy", "needs_choice", "blocked"] as const) {
      expect(ACTION_STAMP[action].mark).not.toBe("");
      expect(ACTION_STAMP[action].word).not.toBe("");
    }
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
  const playlist = { name: "Sunday Morning Service", before: "Worship", after: "Message" } as never;
  it("reads as the owner's sentence", () => {
    expect(playlistSentence({ playlist, to_add: 4 }, "Worship")).toBe("Adds 4 songs after Worship in Sunday Morning Service. Other items stay as they are.");
    expect(playlistSentence({ playlist, to_add: 1 }, null)).toBe("Adds 1 song at the end in Sunday Morning Service. Other items stay as they are.");
    expect(playlistSentence({ playlist, to_add: 0 }, null)).toBe("Adds nothing to Sunday Morning Service. Other items stay as they are.");
  });
  it("names the neighbours the songs land between", () => {
    expect(neighbours({ before: "A", after: "B" })).toBe("They go between \u201CA\u201D and \u201CB\u201D.");
    expect(neighbours({ before: "A", after: null })).toBe("They go after \u201CA\u201D, the last item.");
    expect(neighbours({ before: null, after: "B" })).toBe("They go before \u201CB\u201D, the first item.");
    expect(neighbours({ before: null, after: null })).toBeNull();
  });
  it("labels", () => {
    expect(afterHeaderLabel("Worship")).toBe("After \u201CWorship\u201D");
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
    for (const outcome of ["done", "done_with_differences", "needs_library_open", "stopped", "song_playing", "playlist_refused", "playlist_failed", "backup_failed", "not_reachable", "nothing_to_do"] as const) {
      expect(OUTCOME[outcome].heading).toBeTruthy();
      expect(OUTCOME[outcome].mark).toBeTruthy();
    }
  });
  it("offers Try again after a failure, never after success", () => {
    expect(canTryAgain("needs_library_open")).toBe(true);
    expect(canTryAgain("song_playing")).toBe(true);
    expect(canTryAgain("done")).toBe(false);
    expect(canTryAgain("done_with_differences")).toBe(false);
    expect(canTryAgain("nothing_to_do")).toBe(false);
  });
  it("shows only the file name of a backup, and counts what happened", () => {
    expect(backupName("/home/church/.config/StagePilot/propresenter-playlist-backups/Sunday-1.json")).toBe("Sunday-1.json");
    expect(backupName("C:\\Users\\church\\backups\\Sunday-1.json")).toBe("Sunday-1.json");
    expect(backupName(null)).toBeNull();
    expect(resultCounts(report({ songs: [result("1", "A", "added"), result("2", "B", "added"), result("3", "C", "skipped"), result("4", "D", "failed"), result("5", "E", "not_loaded")] }))).toBe("2 added, 1 skipped, 1 failed, 1 not loaded yet");
  });
});
