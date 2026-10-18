# Phase 3b-A: Playback plugin

Branch `feat/rust-phase3b` (phase 2 + phase 3a merged). Card A of four: only the plugin. Routes, contract transcripts, the test
port and the PR gates are cards B-D.

## What exists

`crates/stagepilot-playback/src/plugin.rs` is `PlaybackInputPlugin` (`plugins/playback_api/plugin.py`) as a `stagepilot-core`
`Plugin`, registered in the server `Runtime` in place of `PlaybackStub`. `mapping.rs` is `playback_api/mapping.py`.

| Python | Rust |
|---|---|
| connection lifecycle `start/stop/_start_selected/_stop_selected/reconfigure` | `PlaybackPlugin` (`Plugin` impl, `reconfigure`, `save_settings`) |
| `_observe`, bounded queue (64), `_consume`, generation guard | `Inner::observe`, `consume` |
| 50 ms `_tick` publishing `connection.changed` (only on change) | `Inner::tick`, `publish_connection` |
| arbiter / `ArbitratedDispatcher` | phase 3a `arbiter.rs`, wired through `CoreDispatcher` / `CoreState` |
| `PlaybackMapper` | `mapping.rs` (`Mapper::plan` is one synchronous decision) |
| `find` / `_scan` / `_adopt` / `cancel_scan` | `PlaybackPlugin::find`, `run_scan`, `adopt`, `cancel_scan` |
| `discover_song_order` | `PlaybackPlugin::discover_song_order` over phase 3a `discovery.rs` |
| `save_settings`, `save_arrangement`, plan/applied keys | same names; keys are byte-identical to Python's `json.dumps` |
| client `_locate` (last good, loopback, 45 s LAN scan, manual host never scans) | `client.rs` `run` |

## Differences from Python

* `midi_source = real` (the `MidiPlaybackPlugin` wrapper) belongs to phase 4. This plugin exposes `set_midi_connected`,
  `midi_guard` and `ownership_revision` for it to hook into. Playback auto-connect stays always on when Playback is selected.
* The Planning Center length writer is a separate service; the plugin only exposes `set_lengths_running`, `plan_changed`,
  `scan_plan_is_current`, `mark_lengths_applied`, `lengths_applied` for it.
* The mapper reads state as a snapshot, so Python's "re-check the generation after the await" collapses into one decision.
* `PlaybackStatus` gained `heartbeat_serial` (Python compares heartbeat object identity for "fresh").
* Discovery can be cancelled with `cancel_discovery`; `stop()` cancels it and waits for the selection to be restored.
* `FakePlayback` gained setlist navigation (`songs`) and `auto_beat` (heartbeats at ~10 Hz like the real app).

## Not verified here

Rig run on the real Playback (needs the CI artifact for macOS Intel and a rig day), live contract transcripts for the Playback
routes (card B).

## Phase 3b-B: routes, contract, tests, types

* `crates/stagepilot-server/src/playback_routes.rs`: every `/api/v1/playback-api/*` route (status, connection, events, arrangement, find, find/cancel, settings, discover-song-order, and the five `planning-center-lengths` routes). The old stub JSON files are gone. `PUT /api/v1/settings` now hands the new settings to the Playback plugin (`save_settings`), as Python does, so a general settings save reconfigures a running connection.
* Request bodies are validated by hand to produce pydantic's `detail` arrays (`extra_forbidden`, `bool_parsing`, `greater_than_equal`, `literal_error`, ...), because the contract compares whole bodies.
* `crates/stagepilot-playback/src/api.rs`: the response models (`PlaybackStatusResponse` and parts), `ts-rs` exported into `generated/ts/`; `tests/api_contract.rs` checks their field names against `contract/openapi.json`. The CI stale check now runs `cargo test -p stagepilot-core -p stagepilot-playback export_bindings`.
* `PlaybackStatus` gained `source` (`manual` / `loopback` / `lan`), set by the client when it locates Playback.
* `contract/transcripts.json`: 30 -> 103 steps, recorded from Python. `scripts/contract/contract.py` gained `fake` steps (drive `backend/tests/playback_fake.py` on loopback port 47901) and `wait` steps (poll a GET until the connection settles). The monitor entry key `at` is volatile. The Python recording is byte-identical across runs.

### Differences from Python

* The Planning Center length service is phase 5. Its routes answer what Python answers while Planning Center is not connected (409 / 503 / 422). `restore` with something to restore answers 503 ("not available in this build yet"); Python would write to Planning Center. `categories` and `category` always answer the 503 Python gives when it cannot read Planning Center, so a connected Planning Center is not served.
* `selected` on `/status` is read straight from the saved source; Python's plugin reads the same value.
* `elapsed` while scanning is rounded to one decimal like Python; the exact value is volatile.

### Not verified here

* The rig (real Playback on macOS): the 2026-10-17 permission has lapsed, so no rig check was done.
* The Planning Center-connected flows of the length routes (phase 5).
