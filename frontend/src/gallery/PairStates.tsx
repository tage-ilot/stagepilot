// Made-up data for the pairing of a song by hand, in the Song order section: the real ledger (`PlaybackArrangement`) with a library plan.
// No backend is asked; Confirm and Undo do nothing here (the controller's functions are empty).
import { PlaybackArrangement } from "../components/PlaybackArrangement";
import type { LibraryPlan, LibraryPlanSong, LibrarySong, PairChoice } from "../types";
import { libraryController, libraryOf, playbackOf } from "./fixtures";
import { setlistOf } from "./presenterLinkData";
import { Scripted, type Step } from "./Scripted";
import type { SampleState } from "./StateCard";

// Playback's setlist: Holy Forever, Battle Belongs, Gratitude, Firm Foundation (the Presenter Link gallery's songs).
const SET = setlistOf(4);
const playbackSong = (index: number): LibrarySong => SET.songs[index]!.library!;
const choiceOf = (song: LibrarySong, suggested = false): PairChoice => ({
  setlist_song_id: song.setlist_song_id, content_id: song.content_id ?? 0, position: song.position, title: song.title, key: song.key, countdown_length_s: song.countdown_length_s, suggested,
});

const matched = (index: number, title = playbackSong(index).title): LibraryPlanSong => ({ ...SET.songs[index]!, title, plan_position: index + 1, key: `k${index + 1}`, pair_choices: [] });
const unpaired = (index: number, title: string, choices: PairChoice[], over: Partial<LibraryPlanSong> = {}): LibraryPlanSong => ({
  key: `k${index + 1}`, title, plan_position: index + 1, plan_seconds: 300, plan_key: null, status: "missing", confidence: null, library: null, candidate: null, pair_choices: choices, ...over,
});

const noop = () => Promise.resolve(true);
const ledger = (songs: LibraryPlanSong[], steps: Step[] = []) => {
  const plan: LibraryPlan = { ...SET, songs, playback_only: [] };
  const library = { ...libraryController(libraryOf("ready")), plan, pairSong: noop, unpairSong: noop };
  const status = playbackOf({ plan_songs: songs.map((song) => ({ id: song.key, title: song.title, duration_seconds: song.plan_seconds })), plan_song_count: songs.length });
  return (
    <Scripted steps={steps}>
      <PlaybackArrangement library={library} status={status} />
    </Scripted>
  );
};

const [BATTLE, GRATITUDE, FIRM] = [1, 2, 3].map(playbackSong) as [LibrarySong, LibrarySong, LibrarySong];

// Plan song 2 is called "Battle Belong": alike, not the same, so StagePilot only suggests Playback's "Battle Belongs".
const uncertain = () => [
  matched(0), unpaired(1, "Battle Belong", [choiceOf(BATTLE, true), choiceOf(GRATITUDE), choiceOf(FIRM)], { status: "uncertain", confidence: "uncertain", candidate: BATTLE }), matched(2), matched(3),
];
// Plan song 2 shares no words with any Playback song.
const nothingAlike = (choices: PairChoice[]) => [matched(0), unpaired(1, "Our Opening Song", choices), matched(2), matched(3)];
const OPEN: Step[] = [{ press: "Pick the Playback song" }];

export const pairStates: SampleState[] = [
  { id: "uncertain", label: "A song that is only alike", look: "The row says Check title with both titles in words, and under it the plain action Pick the Playback song. Nothing is paired: the alike song is only a suggestion.", render: () => ledger(uncertain()) },
  { id: "no-match", label: "A song with no match", look: "The row says Only in plan and, under it, the same plain action. The Playback song it could go with is still listed on the right by its own row once picked.", render: () => ledger(nothingAlike([choiceOf(BATTLE), choiceOf(FIRM)])) },
  { id: "chooser-open", label: "The chooser open", look: "A short list of radio buttons, one per Playback song, each with its title, key and length; the alike song first and marked Looks alike. Nothing is picked, so Confirm is off. Cancel closes it.", render: () => ledger(uncertain(), OPEN) },
  {
    id: "chooser-open-no-match", label: "The chooser open for a song with no match", look: "The same list, in Playback's order (no alike song to put first).", render: () => ledger([matched(0), unpaired(1, "Our Opening Song", [choiceOf(BATTLE), choiceOf(GRATITUDE), choiceOf(FIRM)]), matched(2), matched(3)], OPEN),
  },
  {
    id: "paired", label: "A song paired by you", look: "The stamp says Paired by you, a note names both titles, the right side shows the Playback song's own facts, and the plain action is now Undo.",
    render: () => ledger([matched(0), { ...matched(1, "Our Opening Song"), confidence: "by_hand", library: BATTLE }, matched(2), matched(3)]),
  },
  { id: "no-candidates", label: "No other Playback song to pair", look: "The setlist's other songs all have a plan song already, so the row says Playback has no other song to pair and offers no button.", render: () => ledger([matched(0), unpaired(1, "Our Opening Song", []), matched(2), matched(3)]) },
];
