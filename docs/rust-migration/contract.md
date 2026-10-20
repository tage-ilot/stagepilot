# Contract freeze (phase 1)

The HTTP and WebSocket API of the Python backend is the contract the Rust server has to honour.

| File | What |
|---|---|
| `contract/openapi.json` | OpenAPI 3.1 schema for `/api/v1/*` (65 paths), generated from the FastAPI app. `info.version` is replaced by a placeholder. |
| `contract/transcripts.json` | 103 golden steps (HTTP exchanges, one WebSocket session, and `fake` / `wait` control steps for the Playback flows) recorded in process from `create_app(Settings(demo_mode=True))` with FastAPI's `TestClient`, the same harness the backend tests use. |
| `docs/rust-migration/ws-stream.md` | Description of the WebSocket stream. |
| `scripts/contract/scenarios.py` | The scenarios. Demo-safe only: no Planning Center, MIDI, OSC, ProPresenter or Playback traffic, no keychain, no remote access. |
| `scripts/contract/contract.py` | `record`, `replay BASE_URL`, `openapi`. |
| `scripts/contract/serve_python.py`, `run_baseline.sh` | Start the Python backend on a loopback port, replay, stop. |

Commands (repo root):

```
uv run --project backend --frozen python scripts/contract/contract.py openapi
uv run --project backend --frozen python scripts/contract/contract.py record
scripts/contract/run_baseline.sh              # Python baseline, must print 103/103
uv run --project backend --frozen python scripts/contract/contract.py replay http://127.0.0.1:PORT   # any target
```

`replay` exits 1 and prints every differing JSON path. Compared per step: status code, content type, full JSON body.

## Volatile fields (not compared, only their presence and type)

Matched by key name at any depth: `updated_at`, `started_at`, `last_activity_at`, `last_checked_at`, `last_cue_at`, `revision`,
`version`, `date`, `timestamp`, `position_seconds`, `elapsed_seconds`, `remaining_seconds`, `at`, and any key ending in `_at` (wall-clock
instants such as `last_attempt_at`, `last_successful_plan_reload_at`). A string `id` that is a UUID is replaced by `<uuid>`.
Response headers other than `content-type` (date, server, content-length) are ignored.

## Known limits

- The scenarios cover the demo-mode surface (state, actions, status endpoints, validation errors). Settings writes, Planning Center, MIDI,
  lights, ProPresenter and remote access are only covered by their status GETs and a few 422/409/503 cases. Extend `scenarios.py` as each
  port needs it; every extension is re-recorded from Python, never hand-written.
- Dashboard PIN and remote-auth flows are not replayed (they need persisted state and cookies); phase 2 adds them with the auth port.
- `recent_events` ordering inside one step depends on task scheduling in the demo plugin; the transcripts were recorded twice and are byte-identical, and the live
  replay passed 30/30 over real HTTP.

## Playback flows (phase 3b-B)

`scenarios.py` steps with `"fake"` drive `backend/tests/playback_fake.py` (a scripted Playback on loopback port 47901, started by the runner, so both backends talk to a real WebSocket server); steps with `"wait"` poll a GET until a field has a value, so a replay never races the connection. The scan summary in `scan.details` carries a wall-clock `duration_s`, which is normalised on both sides.

## Lightkey flows ("lightkey" profile)

`lightkey_steps` (54 steps) covers the Lightkey record/replay routes: status, settings (partial save, 422 forms, lax booleans), recording
(armed, refused, too short to keep, discarded), replay start and stop, take clearing, and `POST /lights/settings` keeping the `lightkey`
block. The app is the demo plan with simulated MIDI and timer output, Lighting on OSC to a loopback port nothing listens on, one seeded take
for the first demo song, and the Lightkey agent replaced by `scripts/contract/mock_lightkey.py` (a deterministic fake of the read-only
`lightkey-api`: `GET /time`, `GET /log?since=N`, plus runner-only `/__reset` and `/__change`). Like the pco profile it is recorded from the
Python app over real HTTP, because the link polls in the background. Python: `serve_python_lightkey.py`; Rust: `stagepilot-server
--contract-lightkey --lightkey-port N`. Settled states are awaited with `wait` steps; the few values that are a race inside Python (the
link name right after a settings save, how many seconds a too-short recording captured) are masked.
