# Phase 2: core, settings, auth, server

Branch `feat/rust-phase2` (stacked on `feat/rust-phase1`). Nothing here is packaged by a release workflow.

## What exists

| Area | Where | Python source |
|---|---|---|
| Models, events, state, config schema (serde, ts-rs) | `crates/stagepilot-core/src/{state,events,config,settings}.rs` | `models/`, `core/events.py`, `core/config.py`, `core/settings.py` |
| Event bus (failure-isolated, per-handler timeout, bounded observer stream) | `core/src/bus.rs` | `core/event_bus.py` |
| State store (watch channel, newest snapshot only) | `core/src/store.rs` | `core/state.py` |
| State service (navigation, timer, labels, plan reload) | `core/src/state_service.rs` | `services/state_service.py` |
| Plugin manager and supervisor | `core/src/plugin.rs` | `core/plugin.py` |
| Settings load/save/migrate, backup, env precedence, keyring | `core/src/settings.rs`, `settings_errors.rs` | `core/settings.py`, `core/config.py` |
| Dashboard PIN (PBKDF2), sessions, remote store (Argon2id, throttle, sessions) | `core/src/auth.rs` | `services/dashboard_auth.py`, `services/remote_auth.py` |
| Critical alerts | `core/src/alerts.rs` | `services/critical_alerts.py` |
| axum server, demo plugin, stubs | `crates/stagepilot-server` | `main.py`, `api/routes.py`, `api/dashboard_auth*.py`, `api/access.py`, `api/websocket.py` |

Run it: `cargo run -p stagepilot-server -- --demo --port 8765` (in-memory demo settings, PIN not enforced) or without `--demo` (saved
settings file, keychain, PIN enforced). `STAGEPILOT_WEB_ROOT` (or `frontend/dist`) is served at `/`.

## Behaviour decisions

* Plugin supervision. Python has no automatic restart: a plugin that fails to start is marked `error`, `plugin.failed` is published
  and `restart_failed()` retries on demand. The Rust manager keeps those semantics and adds, as the task asked, a per-plugin
  start/stop/health task with timeouts (so a panic or hang is contained) and `supervise(name)`, which retries a failed plugin with
  exponential backoff (1 s doubling to 30 s). The server does not call `supervise` yet because the only plugins in this phase never fail.
* Dashboard PIN lockout. The Python dashboard PIN has no lockout at all; the only lockout in the app is the remote login throttle
  (5 attempts per email and 30 global per 300 s, 429 + `Retry-After: 300`). The Rust port matches that. No PIN lockout was invented.
* Settings migration. `schema_version` missing or 1 runs `migrate_v1`; the migrated file is written back as schema 2 and the original
  is copied once to `settings.json.pre-rust.bak` (never overwritten). `release_channel` is dropped on load and on save.
* Settings validation errors use pydantic's `detail` shape (`type`, `loc`, `msg`, `input`, `ctx`) so FastAPI-era clients keep working.
  serde stops at the first error, pydantic reports all of them; multi-error bodies therefore differ.

## Stubbed (Python answer while the plugin is disabled)

Planning Center status / service types, MIDI inputs / messages / input selection, ProPresenter, lights, Playback status / connection /
events / find, remote access, remote-auth users. Their bodies are copied from the recorded transcripts. They are not live: enabling
those integrations in settings does nothing yet (phases 3-6). Everything else under `/api/v1/*` that the contract lists but this phase
does not serve (Planning Center OAuth / settings / test / lengths, ProPresenter settings and tests, lights writes, Playback writes,
remote-access mutations, diagnostics) returns 404 from the Rust server.

## Contract

`contract/transcripts.json` has 56 steps (30 from phase 1, 26 new: PIN login, wrong/short PIN, PIN set, session invalidation on PIN
change, protection off/on, settings round trip, unknown/nested-unknown keys, invalid timezone/port, `release_channel`, schema-1 body).
`scripts/contract/run_baseline.sh` (Python) and `scripts/contract/run_rust.sh` (Rust) both print 56/56. Two normalisations were added
to the runner and apply to both targets: today's date compares as `<today>` (the transcripts no longer expire at midnight), and
`recent_events` is compared as a sorted list (plugins start concurrently so same-slot ordering is scheduling-dependent in both
backends). WebSocket messages after the first are compared by envelope only, because one action changes the state several times and
which intermediate revision is pushed first is a race in both implementations (Python's recorded run itself saw a half-applied state);
the HTTP step that follows covers the content. The replay client now keeps cookies, as the in-process TestClient does.
