# Rust workspace (migration, phase 1)

Not shipped yet: nothing here is packaged by the release workflows. Plan and rationale: `docs/rust-migration/`.

- `stagepilot-core`: models and events (serde). Currently only the type-generation proof (`src/api.rs`); TypeScript lands in `generated/ts/`.
- `stagepilot-playback`: Playback WebSocket client, normalizer port, Rust fake Playback (`testkit`), tests, examples (`bench`, `stress`, `fake_playback`).
- `stagepilot-server`: empty placeholder for the axum server (phase 6).
- `midi-probe`: compile + port-listing smoke test for `midir`. Needs ALSA headers on Linux, see `docs/rust-migration/linux-midi.md`.

Toolchain is pinned in `rust-toolchain.toml`; `Cargo.lock` is committed; lints are shared through `[workspace.lints]`.

Tools (`tools/rust-playback/`): `gen_replay.py` (synthetic replay fixture), `gen_real_replay.py` (Python normalizer over the redacted real captures),
`gen_transport_cases.py` (Python accept/reject verdicts for crafted frames), `redact_capture.py`, `run_bench.py` / `bench_py.py`.
Contract freeze runner: `scripts/contract/`.

Regenerate TypeScript: `cargo test -p stagepilot-core export_bindings` then `git diff generated/ts` must be empty.
