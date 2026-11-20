import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { applyAddSongs, getAddSongsJob, getProPresenterPlaylists, previewAddSongs } from "../api";
import type { AddSongsRow } from "../components/addSongsSongs";
import { isStuck, liveText, LINK_NOT_READY_WORDS, NOTHING_CHANGES, OUTCOME, PLAYING_WORDS } from "../components/addSongsWording";
import { describeError } from "../errorText";
import type { AddSongsJob, AddSongsPlan, AddSongsPreviewRequest, ProPresenterPlaylist } from "../propresenterLink";
import type { ProPresenterLinkController } from "./useProPresenterLink";

/** The last playlist and place, remembered in this browser only (never sent anywhere, never saved in StagePilot's settings). */
export const ADD_SONGS_CHOICE_KEY = "stagepilot.propresenter.add-songs-choice";
export const PLACE_END = "";
/** How often a running job is looked at. */
export const JOB_POLL_MS = 1000;
const POLL_FAILS_BEFORE_SAYING = 4;

export type AddSongsPhase = "choose" | "previewing" | "previewed" | "applying" | "finished";

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
  try { window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify(choice)); } catch { /* the choice lasts while the window is open */ }
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
 * The state of the "Add to ProPresenter" window: which songs, which playlist and place, the preview, the running job and its result.
 * Nothing is written to ProPresenter by anything here except `add`, and `add` only runs on a preview that is still true: changing a song,
 * the playlist or the place makes the preview stale (it must be looked at again). The last playlist and place are kept in this browser.
 */
export function useAddSongs({ rows, link, playing }: { rows: AddSongsRow[]; link: ProPresenterLinkController; playing: boolean }) {
  const named = useMemo(() => rows.filter((row) => !row.unnamed), [rows]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(named.map((row) => row.ref)));
  const [playlists, setPlaylists] = useState<ProPresenterPlaylist[] | null>(null);
  const [playlistsError, setPlaylistsError] = useState<string | null>(null);
  const [playlistId, setPlaylistId] = useState("");
  const [place, setPlace] = useState(PLACE_END);
  const [phase, setPhase] = useState<AddSongsPhase>("choose");
  const [plan, setPlan] = useState<AddSongsPlan | null>(null);
  const [previewSignature, setPreviewSignature] = useState<string | null>(null);
  const [gone, setGone] = useState(false);
  const [skipBlocked, setSkipBlocked] = useState(false);
  const [problem, setProblem] = useState<AddSongsProblem | null>(null);
  const [job, setJob] = useState<AddSongsJob | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [lostContact, setLostContact] = useState(false);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const ids = useMemo(() => named.filter((row) => selected.has(row.ref)).map((row) => Number(row.ref)), [named, selected]);
  const signature = `${ids.join(",")}|${playlistId}|${place}`;
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
  useEffect(() => { void loadPlaylists(); }, [loadPlaylists]);

  const toggle = useCallback((ref: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(ref)) next.delete(ref); else next.add(ref);
      return next;
    });
  }, []);
  const toggleAll = useCallback((on: boolean) => setSelected(on ? new Set(named.map((row) => row.ref)) : new Set()), [named]);

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

  // ---- what stops a button, said in words -------------------------------------------------------------------------------
  const linkReady = link.status?.state === "ready";
  const previewBlocked: string | null = !linkReady ? LINK_NOT_READY_WORDS
    : ids.length === 0 ? "Tick at least one song."
      : !playlistId ? "Choose a playlist."
        : !placeOk ? "Choose a place in the playlist."
          : null;
  const addBlocked: string | null = !linkReady ? LINK_NOT_READY_WORDS
    : playing ? PLAYING_WORDS
      : ids.length === 0 ? "Tick at least one song."
        : !plan ? "Press Preview first to see what will change."
          : stale ? "You changed something. Press Preview again."
            : plan.playlist.blocked ? `Blocked: ${plan.playlist.blocked}`
              : plan.stuck > 0 && !skipBlocked ? "Some songs are blocked. Tick \u201CSkip blocked songs\u201D to add the others."
                : !plan.can_apply ? "There is nothing to add."
                  : null;

  const preview = useCallback(async () => {
    if (previewBlocked) return;
    setPhase("previewing");
    setProblem(null);
    setGone(false);
    const where = place === PLACE_END ? { place: "end" as const } : { after_header: place };
    const request: AddSongsPreviewRequest = { setlist_song_ids: ids, playlist_id: playlistId, ...where };
    try {
      const made = await previewAddSongs(request);
      if (!alive.current) return;
      setPlan(made);
      setPreviewSignature(signature);
      setSkipBlocked(false);
      setPhase("previewed");
    } catch (cause) {
      if (!alive.current) return;
      setPlan(null);
      setProblem(problemOf(cause, "StagePilot could not make the preview."));
      setPhase("choose");
    }
  }, [ids, place, playlistId, previewBlocked, signature]);

  const add = useCallback(async () => {
    if (addBlocked || !plan) return;
    setProblem(null);
    setJob(null);
    setLostContact(false);
    setPhase("applying");
    try {
      const started = await applyAddSongs(plan.plan_id, skipBlocked ? plan.songs.filter(isStuck).map((song) => song.ref_id) : []);
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
  }, [addBlocked, plan, skipBlocked]);

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

  /** Try again: a fresh preview of the same choice (files made before are reused, so repeating is safe). */
  const tryAgain = useCallback(() => {
    setJob(null);
    setJobId(null);
    void preview();
  }, [preview]);

  // ---- the sentence a screen reader hears: the newest thing the job did, then its result ----------------------------------
  const report = job?.report ?? null;
  const announcement = useMemo(() => {
    if (phase === "previewing") return "Making the preview\u2026";
    if (phase === "previewed" && plan && !stale) return `Preview ready. ${plan.to_add === 1 ? "1 song" : `${plan.to_add} songs`} would be added. ${NOTHING_CHANGES}`;
    if (phase === "applying") {
      const last = job?.events[job.events.length - 1];
      if (!last) return "Adding songs\u2026";
      const title = last.song ? job?.songs.find((song) => song.ref_id === last.song)?.title ?? null : null;
      return liveText(title, last.text);
    }
    if (phase === "finished") return report ? `${OUTCOME[report.outcome].heading}. ${report.message}` : job?.error ?? "The job stopped.";
    return "";
  }, [phase, plan, stale, job, report]);

  return {
    rows: named, unnamed: rows.filter((row) => row.unnamed), selected, toggle, toggleAll, selectedCount: ids.length,
    playlists, playlistsError, reloadPlaylists: loadPlaylists, playlistId, choosePlaylist, place, choosePlace, playlist,
    phase, plan, stale, skipBlocked, setSkipBlocked, problem, previewBlocked, addBlocked,
    preview, add, tryAgain, job, report, lostContact, announcement, linkReady,
  };
}

export type AddSongsController = ReturnType<typeof useAddSongs>;
