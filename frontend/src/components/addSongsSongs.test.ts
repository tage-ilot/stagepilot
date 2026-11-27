import { describe, expect, it } from "vitest";

import { libraryController, libraryPlan, librarySong, librarySongOf, libraryStatus } from "../test/libraryFixtures";
import { helperStatus, linkController } from "../test/propresenterLinkFixtures";
import type { Song } from "../types";
import { addSongsRows, addToProPresenterAvailable, planNameRows, presenterLinkReady, presenterRows, titleKey } from "./addSongsSongs";

const pc = (...titles: string[]) => ({ songs: titles.map((title, order): Song => ({ id: `s${order}`, title, duration_seconds: 1, order, is_generic: false, source_song_id: null })) });

describe("which songs the Presenter Link lists", () => {
  it("knows a song by its title, whatever the spelling around it", () => {
    expect(titleKey("  Holy   Forever ")).toBe("holy forever");
    expect(titleKey("Holy Forever (Live)")).toBe(titleKey("holy forever  LIVE"));
    expect(titleKey("Emptied (feat. Tyana Reynolds)")).toBe("emptied feat tyana reynolds");
    expect(titleKey("Él Es Digno")).toBe("el es digno");
  });

  it("gives a second song with the same title its own id, so two rows are never one tick", () => {
    expect(planNameRows(pc("Amen", "Other", "amen")).map((row) => row.id)).toEqual(["amen", "other", "amen#2"]);
  });

  it("the Planning Center rows carry only a name", () => {
    const [row] = planNameRows(pc(" Holy Forever "));
    expect(row).toMatchObject({ title: "Holy Forever", source: "planning-center", ref: "", contentId: null, keyName: null, arrangement: null, sections: [], song: null, unnamed: false });
    expect(planNameRows(null)).toEqual([]);
    expect(planNameRows(pc("  "))).toEqual([]);
  });

  it("the Playback rows carry the id, key, arrangement and section order, in setlist order", () => {
    const rows = addSongsRows(libraryPlan([
      librarySongOf("Second", 1, { library: librarySong({ title: "Second", position: 2, setlist_song_id: 22, content_id: 7002, key: "D", arrangement: "Long", sections: [{ name: "", start: 0, end: 1 }, { name: "Chorus", start: 1, end: 2 }] }) }),
      librarySongOf("First", 2, { library: librarySong({ title: "First", position: 1, setlist_song_id: null, content_id: null }) }),
    ]));
    expect(rows.map((row) => row.title)).toEqual(["First", "Second"]);
    expect(rows[0]).toMatchObject({ source: "playback", ref: "", unnamed: true, contentId: null });
    expect(rows[1]).toMatchObject({ ref: "22", contentId: 7002, keyName: "D", arrangement: "Long", unnamed: false, sections: ["Count Off", "Chorus"] });
  });

  it("a song a person paired by hand is a Playback row that remembers the plan song's own name", () => {
    const rows = addSongsRows(libraryPlan([
      librarySongOf("Our Name For It", 1, { confidence: "by_hand", library: librarySong({ title: "Battle Belongs", position: 1, setlist_song_id: 31, content_id: 23126 }) }),
      librarySongOf("Gratitude", 2, { library: librarySong({ title: "Gratitude", position: 2, setlist_song_id: 32, content_id: 31877 }) }),
    ]));
    expect(rows.map((row) => [row.title, row.pairedAs, row.contentId])).toEqual([["Battle Belongs", "Our Name For It", 23126], ["Gratitude", null, 31877]]);
    expect(planNameRows(pc("A"))[0]!.pairedAs).toBeNull();
  });

  it("uses Playback's songs only while the Link, the Library Link and a paired setlist are all there; else the names", () => {
    const ready = linkController(helperStatus("ready"));
    const lib = libraryController("ready");
    expect(addToProPresenterAvailable(ready, lib)).toBe(true);
    expect(presenterRows(ready, lib, pc("A")).map((row) => row.source)).toEqual(["playback"]);
    for (const [link, library] of [
      [linkController(helperStatus("not_installed")), lib],
      [ready, libraryController("not_installed")],
      [ready, libraryController("ready", { status: libraryStatus("ready", { stale: true }) })],
      [ready, libraryController("ready", { plan: libraryPlan([]) })],
      [ready, undefined],
    ] as const) {
      expect(addToProPresenterAvailable(link, library)).toBe(false);
      expect(presenterRows(link, library, pc("A")).map((row) => row.source)).toEqual(["planning-center"]);
    }
  });

  it("the Link is ready only when it says ready and names a way (this computer or the helper)", () => {
    expect(presenterLinkReady(linkController(helperStatus("ready")))).toBe(true);
    expect(presenterLinkReady(linkController(helperStatus("ready", { mode: null })))).toBe(false);
    expect(presenterLinkReady(linkController(helperStatus("outdated")))).toBe(false);
    expect(presenterLinkReady(linkController(null))).toBe(false);
    expect(presenterLinkReady(undefined)).toBe(false);
  });
});
