import type { PlaybackStatusResponse } from "../types";

const knownLength = (seconds: number | null | undefined): seconds is number =>
  typeof seconds === "number" && Number.isFinite(seconds) && seconds >= 0;

function duration(seconds: number | null | undefined): string {
  if (!knownLength(seconds)) return "Unknown";
  const rounded = Math.floor(seconds + 0.5);
  return `${Math.floor(rounded / 60)}:${String(rounded % 60).padStart(2, "0")}`;
}

export function PlaybackSongLengths({ status }: { status: PlaybackStatusResponse }) {
  const lengths = status.song_lengths ?? [];
  if (!lengths.length || status.stale || !status.song_order.length) return null;
  const songs = status.plan_songs ?? [];
  const count = Math.max(songs.length, lengths.length);

  return (
    <div className="min-w-0 space-y-2 text-sm" role="group" aria-label="Song lengths">
      <ol className="min-w-0 divide-y divide-edge" aria-label="Song length comparisons">
        {Array.from({ length: count }, (_, index) => {
          const song = songs[index];
          const length = lengths[index];
          const difference = knownLength(song?.duration_seconds) && knownLength(length)
            ? Math.abs(song.duration_seconds - Math.floor(length + 0.5)) : null;
          return (
            <li key={index} className="grid min-w-0 grid-cols-1 gap-1 py-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:gap-x-4">
              <div className="min-w-0 break-words text-ink-body">
                <span className="font-semibold">{index + 1}. {song ? song.title : "No plan song"}</span>
                {song && <> {" "}<span className="ml-2 tabular-nums text-ink-muted">Plan: {duration(song.duration_seconds)}</span></>}
              </div>
              {index < lengths.length && <p className="min-w-0 break-words tabular-nums text-ink-body">Playback: {duration(length)}</p>}
              {difference !== null && difference > 0 && <p className="min-w-0 break-words text-xs tabular-nums text-warning-text sm:col-span-2">differs by {Math.round(difference)} s</p>}
            </li>
          );
        })}
      </ol>
      <p className="break-words text-xs text-ink-muted">Playback lengths are read from where each song ends (the start of its last measure).</p>
    </div>
  );
}
