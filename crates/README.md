# Rust workspace

The StagePilot server. The desktop app (`desktop/src-tauri`) links `stagepilot-server` and runs it
in-process; `stagepilot-headless` runs the same server without the desktop shell (`cargo run -p
stagepilot-headless`).

- `stagepilot-core`: models, events, event bus, state service, plugin supervisor, settings, auth, alerts (no I/O beyond the settings file, keyring and SQLite stores). TypeScript lands in `generated/ts/`.
- `stagepilot-server`: axum app and library (health, state, settings, dashboard auth, actions, `/ws`, static frontend, the embedded server the desktop shell starts).
- `stagepilot-playback`, `stagepilot-midi`, `stagepilot-osc`, `stagepilot-lights`, `stagepilot-propresenter`, `stagepilot-pco`: one integration each (Playback WebSocket client and fake, MIDI, OSC, Lightkey/lighting, ProPresenter, Planning Center).
- `stagepilot-remote`: the private Remote connector, its listener and the cloudflared tunnel supervision.
- `stagepilot-headless`: the server as a stand-alone binary.
- `midi-probe`: compile + port-listing smoke test for `midir`. Needs ALSA headers on Linux, see `docs/archive/rust-migration/linux-midi.md`.

Toolchain is pinned in `rust-toolchain.toml`; `Cargo.lock` is committed; lints are shared through `[workspace.lints]`.

Comments reading "port of `backend/…`" or "ported from `backend/tests/…`" point at the Python backend as of tag `v2.41.1`; read them with `git show v2.41.1:<path>`. The behaviour-preservation maps are in `docs/archive/rust-migration/`.

Fixtures under each crate's `tests/fixtures/` (OSC and Lightkey packets, the Playback replay, settings files, the MIDI golden) were generated once by the Python backend and are frozen goldens now. `tools/rust-playback/redact_capture.py` turns a raw Playback capture into a committable fixture. The contract replay (`scripts/contract/`, transcripts in `contract/`) checks the server's HTTP and WebSocket answers against the recorded 2.41.1 behaviour.

Regenerate TypeScript: `cargo test -p stagepilot-core export_bindings` then `git diff generated/ts` must be empty.
