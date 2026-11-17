import { useEffect, useState } from "react";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ServicePlan } from "../types";
import { libraryNotices, songFacts } from "./playbackLibrary";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";
import { Switch } from "./ui/Switch";

/** What the plain "Check" says before the helper has answered. */
const OFF_WORDS = "Off. StagePilot does not ask the Playback computer for anything.";

/**
 * "Playback library (optional)", in the Playback API panel. A small helper on the Playback computer can tell StagePilot each song's
 * key, BPM, length and arrangement. StagePilot only READS it: nothing here changes Playback, the plan, the saved scan, a timer or a
 * light. The notices (a song missing from Playback, a different order, a rental that ends, another date) also go in the header
 * pill; the length difference is information only.
 */
export function PlaybackLibraryPanel({ library, plan, timeZone }: { library: PlaybackLibraryController; plan: ServicePlan | null; timeZone: string }) {
  const savedAddress = library.address.replace(/^http:\/\//, "");
  const [on, setOn] = useState(library.enabled);
  const [address, setAddress] = useState(savedAddress);
  useEffect(() => { setOn(library.enabled); setAddress(savedAddress); }, [library.enabled, savedAddress]);
  const changed = on !== library.enabled || address.trim() !== savedAddress;
  const busy = library.pending !== null;
  const notices = libraryNotices(library.plan, plan, timeZone);
  const songs = library.plan?.state === "ready" ? library.plan.songs : [];
  const words = library.status?.message ?? (library.enabled ? "Asking the Playback computer…" : OFF_WORDS);
  const problem = library.status ? library.status.state === "problem" : false;

  return (
    <section aria-labelledby="playback-library-heading" className="space-y-2 rounded-ds-xl bg-integration-playback-soft/5 p-4" data-testid="playback-library">
      <h3 className="font-bold text-ink" id="playback-library-heading">Playback library <span className="text-sm font-normal text-ink-muted">(optional)</span></h3>
      <p className="text-sm text-ink-body">
        A small helper on the Playback computer can tell StagePilot each song's key, BPM, length and arrangement. StagePilot only reads it:
        it never changes Playback, your plan or the saved song order.
      </p>
      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={(event) => { event.preventDefault(); void library.save(on, address); }}
      >
        <Switch checked={on} label="Use the Playback library" onChange={(event) => setOn(event.target.checked)} />
        <Field className="w-full flex-none sm:min-w-48 sm:flex-1" label="Playback library address">
          <Input onChange={(event) => setAddress(event.target.value)} placeholder="For example, 10.20.0.102 (port 8780 unless you add one)" value={address} />
        </Field>
        <Button disabled={busy || !changed} tone="playback" type="submit" variant="tinted">{library.pending === "save" ? "Saving…" : "Save"}</Button>
        <Button disabled={busy || !library.enabled || changed} onClick={library.check} tone="playback" type="button" variant="tinted">{library.pending === "check" ? "Checking…" : "Check"}</Button>
      </form>
      <p className={`text-sm ${problem ? "text-warning-text" : "text-ink-body"}`} data-testid="library-status" role="status">{words}</p>
      {library.error && <p className="text-sm text-danger-text" role="alert">{library.error}</p>}
      {songs.length > 0 && (
        <ul aria-label="Songs in Playback" className="space-y-0.5 text-sm text-ink-body" data-testid="library-songs">
          {songs.map((song) => (
            <li data-song={song.key} data-status={song.status} key={`${song.key}-${song.plan_position}`}>
              <span className="font-semibold text-ink">{song.title}</span>
              {" "}
              <span className="tabular-nums text-ink-muted">
                {song.library ? `· ${songFacts(song.library).join(" · ") || "no details"}`
                  : song.status === "missing" ? "· not in Playback's setlist"
                    : "· appears more than once, so StagePilot cannot tell which Playback song it is"}
              </span>
            </li>
          ))}
        </ul>
      )}
      {notices.length > 0 && (
        <ul aria-label="Playback library notices" className="space-y-0.5 text-sm" data-testid="library-notices">
          {notices.map((notice) => <li className={notice.actionable ? "text-warning-text" : "text-ink-muted"} data-kind={notice.kind} key={notice.id}>{notice.text}</li>)}
        </ul>
      )}
    </section>
  );
}
