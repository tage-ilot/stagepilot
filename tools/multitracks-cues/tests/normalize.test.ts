import { describe, expect, it } from "vitest";
import { normalizeSetlist, normalizeSetlistSummaries } from "../src/multitracks/normalize.js";

describe("MultiTracks response normalization", () => {
  it("lists and fetches setlists while preserving order", () => {
    expect(normalizeSetlistSummaries({ setlists: [{ id: 1, name: "Sunday", date: "2026-07-19" }] })[0]).toEqual({ id: "1", name: "Sunday", targetDate: "2026-07-19" });
    const result = normalizeSetlist({ id: "s1", name: "Sunday", items: [
      { title: "Header", type: "header" },
      { title: "Owned", isSong: true, libraryEntryId: "l1", arrangementType: "library" },
      { title: "Cloud", isSong: true, cloudArrangementId: "c1", arrangementType: "cloud" },
      { title: "Unknown", isSong: true },
    ] });
    expect(result.items.map((item) => item.targetType)).toEqual(["non-song", "library", "cloud", "ambiguous"]);
    expect(result.items.map((item) => item.position)).toEqual([1, 2, 3, 4]);
  });
});

