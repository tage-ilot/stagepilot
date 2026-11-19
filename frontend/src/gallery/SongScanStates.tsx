import { PlaybackApiPanel } from "../components/PlaybackApiPanel";
import { libraryController, libraryOf, playbackController, playbackDown, playbackOf } from "./fixtures";
import type { SampleState } from "./StateCard";

// The Playback config's Song order section with its ONE song-scan button: the best scan available, never hidden, greyed out with its reason.
const read = () => undefined;
const NOTE = "Not using the Library Link's order: Faithfulness is not in Playback's setlist.";
const ORDER = { song_order: [101, 102, 103, 104], song_count: 4, plan_song_count: 4, captured_at: "2026-11-22T07:30:00Z" };
const withLibrary = libraryController(libraryOf("ready"));

export const songScanStates: SampleState[] = [
  { id: "library", label: "Scan Playback Library (the Library Link works)", look: "The button says Scan Playback Library: it reads the Library Link at once (no confirmation), touches nothing in Playback and works while a song plays.", render: () => <PlaybackApiPanel library={withLibrary} playback={playbackController(playbackOf({ ...ORDER, order_source: "library", order_library_note: null }), { refreshFromLibrary: read })} /> },
  { id: "api", label: "Scan Playback API (only the Playback API)", look: "No Library Link: the button says Scan Playback API. It opens the confirmation (Scan Playback API?) and then steps through the songs.", render: () => <PlaybackApiPanel playback={playbackController(playbackOf({ song_order: [], stale: true }))} /> },
  { id: "library-failed", label: "Scan Playback API (the Library Link's order failed)", look: "The Library Link answers but its order failed: the plain line says why and the one button is the best scan left, Scan Playback API. There is no second button.", render: () => <PlaybackApiPanel library={withLibrary} playback={playbackController(playbackOf({ song_order: [], stale: true, order_source: "none", order_library_note: NOTE }), { refreshFromLibrary: read })} /> },
  { id: "reading", label: "Scan Playback Library: reading", look: "The button keeps its words and shows the loading sweep while it reads; it cannot be pressed twice.", render: () => <PlaybackApiPanel library={withLibrary} playback={playbackController(playbackOf({ ...ORDER, order_source: "library", order_library_note: null }), { refreshFromLibrary: read, pending: "library" })} /> },
  { id: "off-not-connected", label: "Greyed out: not connected", look: "Drawn, greyed out and not pressable. Hover it (or listen with a screen reader): Connect to the Playback API first.", render: () => <PlaybackApiPanel playback={playbackController(playbackDown())} /> },
  { id: "off-playing", label: "Greyed out: Playback is playing", look: "The API scan needs Playback stopped: greyed out, with Stop Playback to scan songs.", render: () => <PlaybackApiPanel playback={playbackController(playbackOf({ ...ORDER, playing: true }))} /> },
  { id: "off-running", label: "Greyed out: a scan is running", look: "The scan's progress in the section; the button is greyed out with A scan is running.", render: () => <PlaybackApiPanel playback={playbackController(playbackOf({ discovery: "running", discovery_song: 3, discovery_total: 8 }))} /> },
];
