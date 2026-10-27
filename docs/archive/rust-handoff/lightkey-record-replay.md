# Lightkey record/replay: notes for the Rust migration

This feature (branch `feat/lightkey-record-replay`) was written against the Python backend on
`main`. It is small and almost all pure logic, so it ports cleanly. Port it into
`stagepilot-lights` (it hangs off the lights plugin) and keep the HTTP contract below
byte-for-byte so the frontend does not change.

## What it is

Lightkey sends no state back over OSC or MIDI. A separate read-only agent,
<https://github.com/huntrw6/lightkey-api>, runs on the Lightkey Mac and serves the active cue
set over HTTP. StagePilot polls it, records changes against a song while the countdown runs, and
replays them as OSC on later runs. The agent is not part of this repository and needs no change.

## Files (Python) and where they go in Rust

| Python | Role | Rust target |
|---|---|---|
| `core/lightkey.py` | models + pure logic, **no I/O, no clock** | `stagepilot-lights/src/lightkey.rs` (or a `stagepilot-lightkey` module) |
| `core/config.py` `LightkeySettings`, `LightsSettings.lightkey` | saved settings | `stagepilot-core` settings, inside `LightsSettings` |
| `plugins/lights/lightkey.py` | async shell: poll, record, replay, take store | `stagepilot-lights/src/lightkey_link.rs` |
| `plugins/lights/plugin.py` | owns a `LightkeyLink`; forwards start/stop/reconfigure; exposes `_send_raw_osc` | `LightsPlugin` |
| `core/lights.py` `LightsController` | 4 new methods | trait/impl |
| `api/routes.py`, `models/api.py` | 4 routes + request/response models | `stagepilot-server/src/routes_outputs.rs` |
| `tests/golden/lightkey_golden.json` + `tests/test_lightkey_core.py` | **shared golden vectors** | run the same JSON from a Rust test |
| `tests/test_lightkey_link.py` | integration tests with a fake agent | port as tokio tests with a mock HTTP server |

## Rules to preserve exactly (they are pinned by `lightkey_golden.json`)

Port `core/lightkey.py` function-for-function and load `tests/golden/lightkey_golden.json` in a
Rust test; it covers `state_at`, `managed_cues`, `replay_plan`, `osc_address`, `build_take_edges`,
`take_is_worth_keeping` and `OperatorHold`. Do not change a vector to make Rust pass; if a vector
looks wrong, fix Python and the vector together.

Constants: `MIN_TAKE_FRACTION = 0.5`, `REPLAY_LEAD_SECONDS = 0.04`, `OPERATOR_HOLD_SECONDS = 3.0`,
`ECHO_WINDOW_SECONDS = 2.5`, default API port `8770`.

Details that are easy to get wrong:

- Edge times are rounded to 1 ms, song time on StagePilot's clock. Lightkey's timestamp is on the
  other Mac's clock: measure the offset with 7 `GET /time` samples and keep the lowest-RTT one
  (`remote - (sent + received) / 2`), refresh every 120 s and on every (re)connect.
- `build_take_edges` drops no-op observations (adding a cue already on, removing one already off),
  folds anything at or before t=0 into `initial`, and clamps times into `[0, duration]`.
- `replay_plan` returns `(sync_now, later)`. `sync_now` is **every** managed cue forced to its
  captured state at the start position (`activate`/`deactivate`), sorted by name; `later` is one
  command per change after that position, removals before additions within one edge. Names
  containing `/` or control characters are dropped (they cannot be an OSC address).
- OSC address is `/live/*/cue/<name>/activate|deactivate` with **no arguments** (a float fade
  argument would override each cue's own fade). `toggle` is never used: it is not idempotent.
- Replay is blocked, with these exact strings in `replay_blocked`: `Lighting is off.`,
  `Replay needs Lighting set to OSC.`, `Paused while recording.`, `Replay is switched off.`
- Recording is discarded on seek, on timer reset, and when the song changes while it runs; a
  resume within 2 s of the last position carries on and back-fills changes made while paused. A
  stop saves only if `take_is_worth_keeping`.
- The agent restarting resets its `seq` to a lower number: detect `server_seq < last_seq`, reset
  and re-baseline rather than reporting a change.
- Take file `lightkey-takes.json` sits next to `settings.json`, written atomically
  (temp file + fsync + rename). A corrupt file is renamed `.bad` and ignored; never block startup.

## HTTP contract (unchanged JSON, paths under `/api/v1`)

- `GET /lights/lightkey` -> `LightkeyStatus`
- `PUT /lights/lightkey/settings` body `{api_url?: string, replay?: bool}` -> `{accepted, message, lightkey}`.
  422 on a bad address. Use the "no context" form of the validation error: Python's default error
  detail contained a non-serializable exception object, which this route avoids.
- `POST /lights/lightkey/recording` body `{recording: bool}` -> same response; `accepted` is false
  when the request was refused (not connected, MIDI method, Lighting off).
- `DELETE /lights/lightkey/takes/{song_key}` -> same response.
- `POST /lights/settings` must keep `lightkey` untouched (it rebuilds `LightsSettings`; the Python
  route had to be told to carry the field over, easy to miss in a port).
- `LightsSettings.lightkey = {api_url: "", replay: true}` is new and optional: older settings
  files without it must load with replay **on** (tested).

`ts-rs` export: `LightkeyStatus`, `LightkeyTakeSummary`, `LightkeyActionResponse`,
`LightkeySettings`. The frontend types are in `frontend/src/types.ts`.

## Agent HTTP shape (what the poller reads)

`GET {api}/log?since=N` -> `{"events":[{seq, online, added[], removed[], effective_ms, effective_src}],
"state":{online, active[]|null, seq, server_time}}`; `GET {api}/time` -> `{"t": <ms>}`.
Events without `added` are online/offline notices. `effective_ms` is the first changed DMX frame
(`effective_src: "dmx"`) or the read sample time.

## Measured on the rig (jpl-lights, 2026-10-20), for timing expectations

OSC command to first changed DMX frame, no fade argument: median 41 ms, 28 to 52 ms (n=12). One
agent read: about 0.8 s, which is why changes are stamped from DMX, not from the read time. The
agent's wired address must be allowed in its config; from the Wi-Fi address it answers 403.

## Port suggestions

- `reqwest` (already used for Playback/PCO) for polling; a `tokio::time::interval(250ms)` loop.
- Keep `OperatorHold` and `build_take_edges` as plain structs/fns with no `tokio` so they can be
  tested without a runtime, as in Python.
- The Python `LightkeyLink` is owned by the lights plugin and shares its OSC output; do the same
  rather than opening a second UDP socket.
