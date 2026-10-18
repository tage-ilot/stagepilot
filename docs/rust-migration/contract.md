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

## Remote, diagnostics and static profiles (phase 6b-A)

`scripts/contract/remote_profile.py` holds three more step lists, recorded into `remote_steps` (91),
`diagnostics_steps` (26) and `static_steps` (34) in `contract/transcripts.json`:

- `remote`: the demo app with a Remote identity store and `RemoteFeature` (no manager), plus the
  same app behind the trusted `RemoteIngress` (public origin `https://remote.contract.test`) on a
  second listener (`--ingress-url`). Steps with `via: "ingress"` go there; a stored session cookie
  and CSRF token are captured from the login responses. Covers the product controls, user
  administration, same-origin/CSRF/viewer policy, keyed retries, the login throttle.
- `diagnostics`: a scripted Remote manager enrolled against a mock control plane
  (`mock_control.py`, a request log like the other mocks). Covers the upload shape, the playback
  section, 413/422/429/502/503 and the manager error mapping.
- `static`: the fixture dashboard `scripts/contract/web/`: headers, redirect, 404 shape (no SPA
  fallback), 405, ranges and validators.

Compared headers: `content-type`, `cache-control`, `retry-after`, `accept-ranges`, `content-range`,
`content-length` (not for JSON bodies), and the presence of `etag` / `last-modified`; `set-cookie`
is compared with the token and date blanked. The Rust server runs the profiles with
`--contract-remote` and `--contract-diagnostics`.
