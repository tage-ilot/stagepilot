import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError, applyAddSongs, endProPresenterHelperInstall, getAddSongsJob, getProPresenterLinkStatus, getProPresenterPlaylists, lookupAddSongsTitles, previewAddSongs, startProPresenterHelperInstall } from "./api";

const answer = (status: number, body: unknown) => vi.spyOn(globalThis, "fetch").mockImplementation(async () => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } }));
afterEach(() => vi.restoreAllMocks());

describe("the ProPresenter Link routes", () => {
  it("asks for the status, with check=1 only when asked", async () => {
    const spy = answer(200, { state: "ready" });
    await getProPresenterLinkStatus();
    await getProPresenterLinkStatus(true);
    expect(spy.mock.calls.map((call) => String(call[0]).replace(/^https?:\/\/[^/]+/, ""))).toEqual(["/api/v1/propresenter-link/status", "/api/v1/propresenter-link/status?check=1"]);
  });

  it("opens and ends the setup session on the one helper-install route", async () => {
    const spy = answer(200, { ended: true });
    await startProPresenterHelperInstall();
    await endProPresenterHelperInstall();
    await startProPresenterHelperInstall("192.168.1.20");
    const calls = spy.mock.calls.map(([url, init]) => [String(url).replace(/^https?:\/\/[^/]+/, ""), init?.method, init?.body]);
    expect(calls).toEqual([
      ["/api/v1/propresenter-link/helper-install", "POST", "{}"],
      ["/api/v1/propresenter-link/helper-install", "DELETE", undefined],
      ["/api/v1/propresenter-link/helper-install", "POST", "{\"address\":\"192.168.1.20\"}"],
    ]);
  });

  it("previews, applies and reads a job with the bodies the routes want, and only those", async () => {
    const spy = answer(200, {});
    await getProPresenterPlaylists();
    await previewAddSongs({ setlist_song_ids: [1, 2], playlist_id: "PL", after_header: "Worship" });
    await applyAddSongs("0123456789abcdef0123456789abcdef", ["2"]);
    await getAddSongsJob("fedcba9876543210fedcba9876543210");
    const calls = spy.mock.calls.map(([url, init]) => [String(url).replace(/^https?:\/\/[^/]+/, ""), init?.method ?? "GET", init?.body]);
    expect(calls).toEqual([
      ["/api/v1/propresenter-link/playlists", "GET", undefined],
      ["/api/v1/propresenter-link/add-songs/preview", "POST", "{\"setlist_song_ids\":[1,2],\"playlist_id\":\"PL\",\"after_header\":\"Worship\"}"],
      ["/api/v1/propresenter-link/add-songs/apply", "POST", "{\"plan_id\":\"0123456789abcdef0123456789abcdef\",\"skip\":[\"2\"]}"],
      ["/api/v1/propresenter-link/add-songs/jobs/fedcba9876543210fedcba9876543210", "GET", undefined],
    ]);
  });

  it("looks titles up and previews by title with the bodies the routes want", async () => {
    const spy = answer(200, { titles: [] });
    await lookupAddSongsTitles(["Holy Forever", "Gratitude"]);
    await previewAddSongs({ titles: ["Holy Forever"], playlist_id: "PL", place: "end" });
    const calls = spy.mock.calls.map(([url, init]) => [String(url).replace(/^https?:\/\/[^/]+/, ""), init?.method ?? "GET", init?.body]);
    expect(calls).toEqual([
      ["/api/v1/propresenter-link/add-songs/titles", "POST", "{\"titles\":[\"Holy Forever\",\"Gratitude\"]}"],
      ["/api/v1/propresenter-link/add-songs/preview", "POST", "{\"titles\":[\"Holy Forever\"],\"playlist_id\":\"PL\",\"place\":\"end\"}"],
    ]);
  });

  it("an error keeps the server's sentence and its short code", async () => {
    answer(409, { detail: "A song is playing. Wait until it has ended, then try again.", code: "song_playing" });
    const error = await applyAddSongs("0123456789abcdef0123456789abcdef", []).catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).message).toBe("A song is playing. Wait until it has ended, then try again.");
    expect((error as ApiError).status).toBe(409);
    expect((error as ApiError).code).toBe("song_playing");
  });

  it("an error with no code, or no JSON at all, has none and a plain status sentence", async () => {
    answer(500, { detail: "boom" });
    expect(((await getAddSongsJob("x").catch((cause: unknown) => cause)) as ApiError).code).toBeUndefined();
    vi.spyOn(globalThis, "fetch").mockImplementation(async () => new Response("<html>", { status: 502 }));
    const error = (await getAddSongsJob("x").catch((cause: unknown) => cause)) as ApiError;
    expect(error.message).toBe("StagePilot API returned 502.");
    expect(error.code).toBeUndefined();
  });
});
