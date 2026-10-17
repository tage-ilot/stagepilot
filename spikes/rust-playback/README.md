# Rust Playback client spike (phase 0)

Throwaway evidence for the Rust migration go/no-go. Not part of the shipping app, the
installer or any release workflow. See the PR description for results.

- `crates/stagepilot-playback`: WebSocket client (`client.rs`), normalizer port
  (`normalizer.rs`), Rust fake Playback (`testkit.rs`), tests, benchmark examples.
- `crates/midi-probe`: compile + port-listing smoke test for `midir` (never sends MIDI).
- `tools/gen_replay.py`: runs the Python `Normalizer` over synthetic wire traffic and writes
  the replay fixture the Rust test compares against.
  `cd backend && uv run python ../spikes/rust-playback/tools/gen_replay.py`
- `tools/run_bench.py`, `tools/bench_py.py`: same scripted scenario against both clients.
