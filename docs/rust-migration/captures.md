# Real Playback captures (phase 1)

Raw captures contain the setlist name and real identifiers and are **not** in git. They live in
`~/AgentHub-data/projects/stagepilot-rust-migration/captures/` (mode 600). Only redacted fixtures are committed, under
`crates/stagepilot-playback/tests/fixtures/real/`.

| Fixture | Source | Content |
|---|---|---|
| `operator-25min.jsonl` | Passive receive-only capture of a real operator session on Playback 8.5.4 (recorded 2026-10-07, 4.3 h file; window 120 s - 1620 s = 25 min) | 1,934 frames: heartbeats, 31 `transportPlay`, 23 `setlistSelectSong`, 2 `waveformSeek`, 4 `mixerInfiniteLoop`, 3 `contentLoadSetlist`, 13 `mixerTrackMute`, 39 `mixerTrackSolo`, 53 `transportNavigateToSongMapElementIndex`, `contentUpdateSetlist`, volume frames thinned |
| `operator-pad-section.jsonl` | Same file, 80 s window around the only `waveformDoubleTap` in the capture | section jump |
| `operator-pad-off.jsonl` | Same file, 100 s window around a `transportPad {playing:false}` | pad off |
| `rig-commands.jsonl` | Recorded 2026-10-17 on JPL tracks by a listening connection while a script drove the commands (see the PR for exactly what was sent) | every command (`kind: sent`) and Playback's relayed copy (`kind: frame`) |
| `expected_events.json` | Output of the Python normalizer over the three operator files, fast_transport off and on | the Rust test must match it event for event |

Not in any real capture: `mixerLoop`, `mixerMuteMIDI`, `waveformLoop`, `transportPad {playing:true}`, `setlistSelectSongTransition` beyond the 6
early frames. Those are covered by `rig-commands.jsonl`, whose sent/relayed pairs prove the wire shape on a live Playback.

## Redaction (`tools/rust-playback/redact_capture.py`)

- `setlistName` becomes `"Redacted setlist"`.
- `setlistID` becomes `7000001, 7000002, ...`, `setlistSongID` becomes `101, 102, ...`, `setlistSongSectionID` becomes `9000, 9001, ...`
  (stable per distinct original value, so song/section relationships are preserved).
- `rentalData`, `modularClickSongData` and `rentalsData` lists are replaced by lists of empty objects of the same length (the normalizer only
  uses the counts). `liveData` becomes `{}`. Content ids and mapping ids inside them are gone.
- `mixerTrackVolume` frames are thinned to every 20th (they produce no events by default).
- Wall-clock times are rebased to start at 0; the ISO timestamps are dropped.
- Heartbeats, positions, transport and pad states, loop/mute flags, and track mute/solo ids (`TrackMute_Bus_18` style, not identifying) are kept verbatim.
- Checked: `grep -c "Sunday\|October"` on the fixtures returns 0 for every file.

To re-record: `python -m playback_api capture FILE --hours 0.5` (huntrw6/playback-api) on the Playback computer, then
`python3 tools/rust-playback/redact_capture.py RAW OUT [START_SEC END_SEC]`, then regenerate the expectations with
`cd backend && uv run python ../tools/rust-playback/gen_real_replay.py`.
