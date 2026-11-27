import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { FollowSong } from "../presenterFollow";
import type { LibraryPlan, LibrarySong, ServicePlan } from "../types";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";

/** One song of the Presenter Link list: one row, whichever source it came from. */
export interface AddSongsRow {
  /** What a tick belongs to: the song's title as ProPresenter would read it, then `#2` for a second song of that title. The same song has the same id in both sources, so a swap keeps the person's ticks. */
  id: string;
  /** `setlist_song_id` as text: what the preview names a song by (`ref_id`). Empty for a Planning Center name and for a Playback song with no number. */
  ref: string;
  title: string;
  /** Where the row comes from: Playback's setlist (id, key, arrangement known) or the Planning Center plan (the name only). */
  source: "playback" | "planning-center";
  /** The MultiTracks number; null when Playback gave none or the row is a Planning Center name. */
  contentId: number | null;
  /** The key Playback plays it in; null when none is known. */
  keyName: string | null;
  arrangement: string | null;
  /** The section names in playing order (for the row's Details). */
  sections: string[];
  song: LibrarySong | null;
  /** Playback gave the song no number, so StagePilot cannot name it to ProPresenter's side. */
  unnamed: boolean;
  /** The plan song's key as the follow status names it (Playback library plan key; the Planning Center id for a name); null when there is none. */
  planKey: string | null;
  /** The plan song's own name when a person paired it by hand with this Playback song (\"Paired by you\"); null for every song matched by name. */
  pairedAs: string | null;
}

/** A title as ProPresenter would read it: letters and digits only (no accents, no case), one space between words. Two rows with the same text are the same song. */
export function titleKey(title: string): string {
  return title.normalize("NFKD").replace(/\p{M}+/gu, "").toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

/** Ids for rows in order: the title's key, `#2` for the second row with that key. */
function withIds<T extends { title: string }>(rows: T[]): (T & { id: string })[] {
  const seen = new Map<string, number>();
  return rows.map((row) => {
    const key = titleKey(row.title);
    const n = (seen.get(key) ?? 0) + 1;
    seen.set(key, n);
    return { ...row, id: n === 1 ? key : `${key}#${n}` };
  });
}

/** The setlist songs the loaded plan is paired with (an uncertain or missing pairing has no Playback song to add). In Playback's setlist order. */
export function addSongsRows(plan: LibraryPlan | null | undefined): AddSongsRow[] {
  if (!plan?.enabled || plan.state !== "ready" || !plan.setlist) return [];
  const songs = plan.songs.flatMap((planSong) => (planSong.library ? [{ song: planSong.library, planKey: planSong.key, pairedAs: planSong.confidence === "by_hand" ? planSong.title : null }] : []));
  const ordered = songs.map((entry, index) => ({ ...entry, index })).sort((a, b) => a.song.position - b.song.position || a.index - b.index);
  return withIds(ordered.map(({ song, planKey, pairedAs }) => ({
    ref: song.setlist_song_id === null ? "" : String(song.setlist_song_id),
    title: song.title,
    source: "playback" as const,
    contentId: song.content_id ?? null,
    keyName: song.key,
    arrangement: song.arrangement,
    sections: song.sections.map((section) => section.name.trim() || "Count Off"),
    song,
    unnamed: song.setlist_song_id === null,
    planKey,
    pairedAs,
  })));
}

/** The upcoming plan's Planning Center song names, in plan order: no id, key or arrangement, and no Playback song behind them. */
export function planNameRows(plan: Pick<ServicePlan, "songs"> | null | undefined): AddSongsRow[] {
  return withIds((plan?.songs ?? []).filter((song) => song.title.trim()).map((song) => ({
    ref: "",
    title: song.title.trim(),
    source: "planning-center" as const,
    contentId: null,
    keyName: null,
    arrangement: null,
    sections: [] as string[],
    song: null,
    unnamed: false,
    planKey: song.source_song_id ?? song.id,
    pairedAs: null,
  })));
}

/** The Presenter Link is ready (this computer, or the helper): the one thing that decides whether the section is drawn. */
export function presenterLinkReady(link: Pick<ProPresenterLinkController, "status"> | undefined): boolean {
  return link?.status?.state === "ready" && link.status.mode !== null;
}

/**
 * Whether the songs come from Playback: the Presenter Link is ready, the Playback Library Link answers with data that is not an old
 * copy, and the loaded plan is paired with a Playback setlist that has songs. Anything else, and the list is the plan's names.
 */
export function addToProPresenterAvailable(link: Pick<ProPresenterLinkController, "status"> | undefined, library: Pick<PlaybackLibraryController, "status" | "plan"> | undefined): boolean {
  if (!link || !library) return false;
  const libraryReady = Boolean(library.status?.active) && !library.status?.stale;
  return presenterLinkReady(link) && libraryReady && addSongsRows(library.plan).length > 0;
}

/** The rows of the one list: Playback's setlist while it has given the songs, else the plan's Planning Center names. */
export function presenterRows(link: Pick<ProPresenterLinkController, "status"> | undefined, library: Pick<PlaybackLibraryController, "status" | "plan"> | undefined, plan: Pick<ServicePlan, "songs"> | null | undefined): AddSongsRow[] {
  return addToProPresenterAvailable(link, library) ? addSongsRows(library?.plan) : planNameRows(plan);
}

/**
 * What the follow status says about each row: by the plan song's key first (Playback's plan key, or the Planning Center id), then by the
 * title (the same id a row has), because a status made while the switch is off knows the plan's own songs by their own keys.
 */
export function followByRow(rows: readonly AddSongsRow[], songs: readonly FollowSong[] | undefined): Map<string, FollowSong> {
  const found = new Map<string, FollowSong>();
  if (!songs?.length) return found;
  const byKey = new Map(songs.map((song) => [song.key, song]));
  const byTitle = new Map(withIds(songs.map((song) => ({ title: song.title }))).map((entry, index) => [entry.id, songs[index]!]));
  for (const row of rows) {
    const song = (row.planKey !== null ? byKey.get(row.planKey) : undefined) ?? byTitle.get(row.id);
    if (song) found.set(row.id, song);
  }
  return found;
}
