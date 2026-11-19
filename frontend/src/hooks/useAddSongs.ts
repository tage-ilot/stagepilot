import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { applyAddSongs, getAddSongsJob, getProPresenterPlaylists, previewAddSongs } from "../api";
import { presenterLinkReady, type AddSongsRow } from "../components/addSongsSongs";
import { guessAnnouncement, isStuck, liveText, LINK_NOT_READY_WORDS, NOTHING_CHANGES, OUTCOME, PLAYING_WORDS, stuckReason } from "../components/addSongsWording";
import { describeError } from "../errorText";
import type { AddSongsJob, AddSongsPlan, AddSongsPreviewRequest, ProPresenterPlaylist } from "../propresenterLink";
import type { ProPresenterLinkController } from "./useProPresenterLink";
import { useTitleLookup } from "./useTitleLookup";

/** The last playlist and place, remembered in this browser only (never sent anywhere, never saved in StagePilot's settings). */
export const ADD_SONGS_CHOICE_KEY = "stagepilot.propresenter.add-songs-choice";
export const PLACE_END = "";
/** How often a running job is looked at. */
export const JOB_POLL_MS = 1000;
const POLL_FAILS_BEFORE_SAYING = 4;

/** `choose`: nothing asked yet. `previewed`: the one button made a preview and STOPPED (something needs a decision, or nothing can be added). */
export type AddSongsPhase = "choose" | "previewing" | "previewed" | "applying" | "finished";

/** Why a row cannot be ticked: Playback gave it no number, ProPresenter has no song with this title, or it has two. */
export type RowBlock = "unnamed" | "missing" | "several";

interface Remembered { playlist_id: string; place: string }

function remembered(): Remembered | null {
  try {
    const value = JSON.parse(window.localStorage.getItem(ADD_SONGS_CHOICE_KEY) ?? "null") as Partial<Remembered> | null;
    return value && typeof value.playlist_id === "string" && typeof value.place === "string" ? { playlist_id: value.playlist_id, place: value.place } : null;
  } catch {
    return null;
  }
}
function remember(choice: Remembered) {
  try { window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify(choice)); } catch { /* the choice lasts while the section is on screen */ }
}

export interface AddSongsProblem { message: string; code: string | null }

/**
 * The words for a refusal. The routes answer `{detail, code}` and write `detail` for an operator (even for a 502 such as "ProPresenter did
 * not answer"), so a coded answer is shown as sent; anything else (a transport error, a bare status) gets the plain sentences of `describeError`.
 */
export function problemOf(cause: unknown, fallback: string): AddSongsProblem {
  const code = typeof (cause as { code?: unknown } | null)?.code === "string" ? (cause as { code: string }).code : null;
  const status = (cause as { status?: unknown } | null)?.status;
  if (code && cause instanceof Error && typeof status === "number" && status !== 401 && status !== 403 && /\s/.test(cause.message) && cause.message.length <= 300 && !/[{}<>]/.test(cause.message)) {
    return { message: cause.message, code };
  }
  return { message: describeError(cause, fallback), code };
}

/**
 * The state of the Presenter Link section: which songs (the list's rows, whichever source they come from), which playlist and section, the
 * preview, the running job and its result. ONE press (`run`) makes the preview and, when nothing in it is blocked, applies it at once; if
 * anything is blocked or needs a decision it stops after the preview with the reason, and only `skipAndAdd` goes on without those songs.
 * Nothing is written to ProPresenter by anything here except `start`, and `start` only runs on the preview just made. Ticks belong to a song
 * (its title), not to a row, so the list swapping from Planning Center names to Playback's songs keeps them. The last playlist and place are
 * kept in this browser.
 */
export function useAddSongs({ rows, link, playing }: { rows: AddSongsRow[]; link: ProPresenterLinkController; playing: boolean }) {
  const linkReady = presenterLinkReady(link);
  const names = useMemo(() => rows.filter((row) => row.source === "planning-center").map((row) => row.title), [rows]);
  const lookup = useTitleLookup(names, linkReady);
  const blockOf = useCallback((row: AddSongsRow): RowBlock | null => {
    if (row.source === "playback") return row.unnamed ? "unnamed" : null;
    const known = lookup.stateOf(row.title);
    return known === "missing" || known === "several" ? known : null;
  }, [lookup]);
  const choices = useMemo(() => rows.filter((row) => !blockOf(row)), [rows, blockOf]);

  // The person's UNticks: a song nobody touched is ticked, so a new row (the list swapped, a song arrived) starts ticked.
  const [unticked, setUnticked] = useState<Set<string>>(() => new Set());
  const [playlists, setPlaylists] = useState<ProPresenterPlaylist[] | null>(null);
  const [playlistsError, setPlaylistsError] = useState<string | null>(null);
  const [playlistId, setPlaylistId] = useState("");
  const [place, setPlace] = useState(PLACE_END);
  const [phase, setPhase] = useState<AddSongsPhase>("choose");
  const [plan, setPlan] = useState<AddSongsPlan | null>(null);
  const [previewSignature, setPreviewSignature] = useState<string | null>(null);
  const [previewRows, setPreviewRows] = useState<Map<string, AddSongsRow>>(() => new Map());
  const [gone, setGone] = useState(false);
  const [skipBlocked, setSkipBlocked] = useState(false);
  const [problem, setProblem] = useState<AddSongsProblem | null>(null);
  const [job, setJob] = useState<AddSongsJob | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [lostContact, setLostContact] = useState(false);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const picked = useMemo(() => choices.filter((row) => !unticked.has(row.id)), [choices, unticked]);
  const selected = useMemo(() => new Set(picked.map((row) => row.id)), [picked]);
  const fromPlayback = rows.some((row) => row.source === "playback");
  const signature = `${fromPlayback ? "playback" : "names"}|${picked.map((row) => row.id).join(",")}|${playlistId}|${place}`;
  const stale = plan !== null && (gone || previewSignature !== signature);
  const playlist = playlists?.find((candidate) => candidate.id === playlistId) ?? null;
  // A header the playlist no longer has falls back to the end.
  const placeOk = place === PLACE_END || Boolean(playlist?.headers.includes(place));

  const loadPlaylists = useCallback(async () => {
    setPlaylistsError(null);
    try {
      const answer = await getProPresenterPlaylists();
      if (!alive.current) return;
      setPlaylists(answer.playlists);
      const last = remembered();
      const chosen = answer.playlists.find((candidate) => candidate.id === last?.playlist_id);
      if (chosen) {
        setPlaylistId((current) => current || chosen.id);
        setPlace((current) => (current === PLACE_END && last && chosen.headers.includes(last.place) ? last.place : current));
      }
    } catch (cause) {
      if (alive.current) setPlaylistsError(problemOf(cause, "StagePilot could not read the playlists from ProPresenter.").message);
    }
  }, []);
  // The playlists are asked for only while the Presenter Link is ready (the section is not drawn otherwise).
  useEffect(() => { if (linkReady) void loadPlaylists(); }, [linkReady, loadPlaylists]);

  const toggle = useCallback((id: string) => {
    setUnticked((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }, []);
  const toggleAll = useCallback((on: boolean) => setUnticked(on ? new Set() : new Set(choices.map((row) => row.id))), [choices]);

  const choosePlaylist = useCallback((id: string) => {
    setPlaylistId(id);
    const last = remembered();
    const next = playlists?.find((candidate) => candidate.id === id);
    const keep = last?.playlist_id === id && next?.headers.includes(last.place) ? last.place : PLACE_END;
    setPlace(keep);
    remember({ playlist_id: id, place: keep });
  }, [playlists]);
  const choosePlace = useCallback((value: string) => {
    setPlace(value);
    remember({ playlist_id: playlistId, place: value });
  }, [playlistId]);

  // ---- what stops the button, said in words -------------------------------------------------------------------------------
  const blockedBy: string | null = !linkReady ? LINK_NOT_READY_WORDS
    : picked.length === 0 ? "Tick at least one song."
      : !playlistId ? "Choose a playlist."
        : !placeOk ? "Choose a section of the playlist."
          : null;

  // ---- the one button -----------------------------------------------------------------------------------------------------
  /** The preview of what is ticked. `null` when it could not be made (then `problem` says why). */
  const makePreview = useCallback(async (): Promise<AddSongsPlan | null> => {
    setPhase("previewing");
    setProblem(null);
    setGone(false);
    setJob(null);
    setJobId(null);
    const where = place === PLACE_END ? { place: "end" as const } : { after_header: place };
    const request: AddSongsPreviewRequest = fromPlayback
      ? { setlist_song_ids: picked.map((row) => Number(row.ref)), playlist_id: playlistId, ...where }
      : { titles: picked.map((row) => row.title), playlist_id: playlistId, ...where };
    // What the preview calls a song: Playback's number, or `t1`, `t2`... in the order the titles were sent.
    const refs = new Map(picked.map((row, index) => [fromPlayback ? row.ref : `t${index + 1}`, row] as const));
    try {
      const made = await previewAddSongs(request);
      if (!alive.current) return null;
      setPlan(made);
      setPreviewSignature(signature);
      setPreviewRows(refs);
      setSkipBlocked(false);
      return made;
    } catch (cause) {
      if (!alive.current) return null;
      setPlan(null);
      setProblem(problemOf(cause, "StagePilot could not make the preview."));
      setPhase("choose");
      return null;
    }
  }, [fromPlayback, picked, place, playlistId, signature]);

  /** Applies a preview that was just made; `skip` leaves out the songs it called stuck. */
  const start = useCallback(async (made: AddSongsPlan, skip: boolean) => {
    setProblem(null);
    setJob(null);
    setLostContact(false);
    setSkipBlocked(skip);
    setPhase("applying");
    try {
      const started = await applyAddSongs(made.plan_id, skip ? made.songs.filter(isStuck).map((song) => song.ref_id) : []);
      if (!alive.current) return;
      setJobId(started.job_id);
    } catch (cause) {
      if (!alive.current) return;
      const found = problemOf(cause, "StagePilot could not start.");
      setProblem(found);
      // The preview is no longer the truth (ProPresenter, the playlist or Playback changed, or it is too old): look again.
      if (found.code === "plan_changed" || found.code === "unknown_plan" || found.code === "songs_blocked") setGone(true);
      setPhase("previewed");
    }
  }, []);

  /** The button: the preview, then (when nothing is blocked and no song is playing) the add, in one go. */
  const run = useCallback(async () => {
    if (blockedBy) return;
    const made = await makePreview();
    if (!made) return;
    if (playing || stuckReason(made, false) !== null) { setPhase("previewed"); return; }
    await start(made, false);
  }, [blockedBy, makePreview, playing, start]);

  /** **Skip those and add the rest**: only on a preview that stopped for blocked songs and is still true. */
  const canSkip = phase === "previewed" && plan !== null && !stale && !playing && linkReady && plan.stuck > 0 && !plan.playlist.blocked && plan.to_add > 0;
  const skipAndAdd = useCallback(() => { if (canSkip && plan) void start(plan, true); }, [canSkip, plan, start]);

  // A running job is looked at every second until it has an answer.
  useEffect(() => {
    if (phase !== "applying" || !jobId) return undefined;
    let cancelled = false;
    let timer: number | undefined;
    let misses = 0;
    const tick = async () => {
      try {
        const next = await getAddSongsJob(jobId);
        if (cancelled || !alive.current) return;
        misses = 0;
        setLostContact(false);
        setJob(next);
        if (next.state !== "running") { setPhase("finished"); return; }
      } catch {
        misses += 1;
        if (!cancelled && misses >= POLL_FAILS_BEFORE_SAYING) setLostContact(true);
      }
      if (!cancelled) timer = window.setTimeout(() => void tick(), JOB_POLL_MS);
    };
    void tick();
    return () => { cancelled = true; if (timer !== undefined) window.clearTimeout(timer); };
  }, [phase, jobId]);

  /** Try again: a fresh preview of the same choice, applied at once when it is clean (files made before are reused, so repeating is safe). */
  const tryAgain = useCallback(() => {
    setJob(null);
    setJobId(null);
    void run();
  }, [run]);

  // Why the one button stopped after the preview (in words), while that preview is still true.
  const stopped = phase === "previewed" && plan && !stale ? (playing ? PLAYING_WORDS : stuckReason(plan, true)) : null;

  // ---- the sentence a screen reader hears: the newest thing the job did, then its result ----------------------------------
  const report = job?.report ?? null;
  const announcement = useMemo(() => {
    if (phase === "previewing") return "Making the preview\u2026";
    if (phase === "previewed" && plan && !stale) return stopped ? `Preview ready. ${stopped} ${NOTHING_CHANGES}` : `Preview ready. ${plan.to_add === 1 ? "1 song" : `${plan.to_add} songs`} would be added. ${NOTHING_CHANGES}`;
    if (phase === "applying") {
      const last = job?.events[job.events.length - 1];
      if (!last) return "Adding songs\u2026";
      const title = last.song ? job?.songs.find((song) => song.ref_id === last.song)?.title ?? null : null;
      return liveText(title, last.text);
    }
    if (phase === "finished") return report ? [`${OUTCOME[report.outcome].heading}. ${report.message}`, guessAnnouncement(report)].filter(Boolean).join(" ") : job?.error ?? "The job stopped.";
    return "";
  }, [phase, plan, stale, stopped, job, report]);

  return {
    rows, choices, blockOf, selected, toggle, toggleAll, selectedCount: picked.length, fromPlayback, previewRows,
    playlists, playlistsError, reloadPlaylists: loadPlaylists, playlistId, choosePlaylist, place, choosePlace, playlist,
    phase, plan, stale, skipBlocked, problem, blockedBy, stopped, canSkip,
    run, skipAndAdd, tryAgain, job, report, lostContact, announcement, linkReady,
  };
}

export type AddSongsController = ReturnType<typeof useAddSongs>;
