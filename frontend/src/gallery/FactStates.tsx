// Made-up data for the song-fact chips of the section gallery: the chips by themselves, then in every card that has them (Lighting's songs,
// Playback's config songs, the matchup ledger). The Presenter Link's cards are in its own group. No backend is asked.
import { MatchupRow } from "../components/MatchupRow";
import { PlaybackLibrarySongs } from "../components/PlaybackLibrarySongs";
import { FactChips, type Fact } from "../components/sections";
import { cardLibrary } from "../components/SongCard";
import { SongsStrip, type SongChip } from "../components/SongsStrip";
import type { MatchRow, RowStamp } from "../components/songMatchup";
import { libraryController, libraryOf } from "./fixtures";
import { setlistOf } from "./presenterLinkData";
import type { SampleState } from "./StateCard";
import type { LibraryPlan, LibraryPlanSong } from "../types";

const none = () => undefined;
/** A box as wide as ONE song card at its narrowest (the grid's 240 px column), so a wrap is seen without a phone. */
const narrow = (node: React.ReactNode) => <div className="w-[240px] border border-dashed border-edge-medium p-2">{node}</div>;
const row = (label: string, node: React.ReactNode) => (
  <div className="grid items-start gap-1 sm:grid-cols-[14rem_1fr]"><span className="text-xs text-ink-muted">{label}</span>{node}</div>
);

const KEY: Fact = { text: "Ab", name: "Key Ab" };
const WAS: Fact = { text: "Ab (was G)", name: "Key Ab, was G" };
const LENGTH: Fact = { text: "4:37", name: "length 4:37" };
const BPM: Fact = { text: "65 BPM" };
const MTID: Fact = { text: "mtID-23126" };

const chipsByThemselves = (
  <div className="space-y-2 border border-edge-medium bg-surface-panel p-3">
    {row("Key and length", <FactChips facts={[KEY, LENGTH]} />)}
    {row("Key changed in Playback", <FactChips facts={[WAS, LENGTH]} />)}
    {row("Key, mtID", <FactChips facts={[{ text: "G", name: "Key G" }, MTID]} />)}
    {row("Status, tempo", <FactChips facts={[{ text: "not captured" }, BPM]} />)}
    {row("Four facts", <FactChips facts={[KEY, BPM, LENGTH, MTID]} />)}
    {row("A tone, only where the words said it", <FactChips facts={[{ text: "Not captured", tone: "warning" }, { text: "Failed", tone: "danger" }, { text: "Done", tone: "success" }, BPM]} />)}
    {row("One fact", <FactChips facts={[{ text: "No key" }]} />)}
    {row("A long fact: cut inside its chip, all words on hover", narrow(<FactChips facts={[{ text: "Ab (was G) with a very long extra note on this song" }, LENGTH]} />))}
    {row("In one card's width (wraps only when it must)", narrow(<FactChips facts={[KEY, BPM, LENGTH, MTID]} />))}
    {row("No facts", <span className="text-xs text-ink-muted">(nothing is drawn)</span>)}
  </div>
);

// ---- Lighting's songs ----------------------------------------------------------------------------------------------------
const NOW = Date.now();
const PLAN: LibraryPlan = setlistOf(4);
const cardOf = (index: number) => cardLibrary(PLAN.songs[index], NOW);
const lightingChips: SongChip[] = [
  { key: "s1", title: "Holy Forever", order: 1, looks: 12, handmade: 0, hasLoudness: true, label: "fully_captured", library: cardOf(0) },
  { key: "s2", title: "Battle Belongs", order: 2, looks: 4, handmade: 0, hasLoudness: false, label: "incomplete", library: cardOf(1) },
  { key: "s3", title: "Gratitude", order: 3, looks: 0, handmade: 3, hasLoudness: false, label: "manually_cued", library: cardOf(2) },
  { key: "s4", title: "Firm Foundation", order: 4, looks: 0, handmade: 0, hasLoudness: false, label: "not_captured", library: cardOf(3) },
  { key: "s5", title: "A song Playback does not have", order: 5, looks: 0, handmade: 0, hasLoudness: false },
  { key: "s6", title: "Loudness only, no tempo", order: 6, looks: 0, handmade: 0, hasLoudness: true, label: "not_captured" },
];

// ---- the ledger ------------------------------------------------------------------------------------------------------------
const songs = PLAN.songs.map((song) => song.library!);
const ledgerRow = (id: string, stamp: RowStamp, planKey: string | null, playbackIndex: number | null, notes: string[] = [], titleIndex = playbackIndex ?? 0): MatchRow => ({
  id, kind: "pair", stamp, confidence: "exact",
  plan: { number: 1, id, title: songs[titleIndex]!.title, seconds: 300, key: planKey, skipped: false },
  playback: playbackIndex === null ? null : { number: playbackIndex + 1, title: songs[playbackIndex]!.title, song: songs[playbackIndex]!, cover: false, seconds: 300, skipId: null, skipped: false },
  notes, sentence: `${songs[titleIndex]!.title}: ${stamp}.`,
});
const ledgerRows: MatchRow[] = [
  ledgerRow("r1", "matched", "G", 0),
  ledgerRow("r2", "check-key", "C", 1, ["The plan says C, Playback plays Ab."]),
  ledgerRow("r3", "matched", null, 2),
  { ...ledgerRow("r4", "only-plan", "E", null, [], 3), kind: "plan-only", playback: null },
];

export const factTemplates: SampleState[] = [
  { id: "chips", label: "Fact chips: by themselves", look: "One square chip per fact (the look the Presenter Link's key used to have), a small gap, no dot or bullet between; a tone only where the words said it; a long fact is cut inside its chip; the box on the dashed line is one card wide.", render: () => chipsByThemselves },
  { id: "lighting", label: "Lighting's songs", look: "Under each title: the capture words, a note chip when it has loudness, and the tempo, each its own chip. A song Playback does not have shows only its capture words.", render: () => <SongsStrip chips={lightingChips} liveKey="s2" noun="looks" onClear={none} onSelect={none} selectedKey="s1" /> },
  {
    id: "playback", label: "Playback's songs (config)", look: "Under each title: the key (with (was F) for a key changed today) and the length, one chip each. A song Playback does not have or cannot tell apart keeps its words, not chips.",
    render: () => {
      const songs4: LibraryPlanSong[] = [...PLAN.songs.slice(0, 3), { ...PLAN.songs[3]!, status: "missing", library: null }];
      return <PlaybackLibrarySongs library={{ ...libraryController(libraryOf("ready")), plan: { ...PLAN, songs: songs4 } }} plan={null} timeZone="America/Los_Angeles" />;
    },
  },
  { id: "ledger", label: "The song matchup (ledger)", look: "The key (the bare letter) and the tempo as chips on the Playback side, the key as a chip on the plan side (when the plan names one); a side with no facts keeps its place.", render: () => <ol aria-label="Song pairs" className="arr-list m-0 grid list-none gap-0 p-0">{ledgerRows.map((r) => <MatchupRow key={r.id} row={r} skips={false} />)}</ol> },
];
