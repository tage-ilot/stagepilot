# WebSocket stream (`/ws`)

Source: `backend/src/stagepilot/api/websocket.py`. The stream is receive-only full-state push.

- Path: `/ws`. Local use needs no subprotocol. The dashboard PIN middleware closes unauthenticated sockets with code `4401` and reason "Dashboard PIN required".
- Remote ingress (Cloudflare tunnel) only permits `/ws`, requires an `Origin` equal to the public origin, re-validates the session roughly every second, and
  closes with `4401` (session expired or revoked), `4503` (remote storage unavailable) or `4403` ("State stream is receive-only") if the client sends anything other than a disconnect.
- Server to client, text JSON, one message type:

  ```json
  {"type": "state.snapshot", "data": <ApplicationState>}
  ```

  `data` is the same object as `GET /api/v1/state` (see `contract/openapi.json`, schema `ApplicationState`, wrapped by `StateEnvelope`).
- Ordering: the first message is sent immediately after `accept` with the current snapshot. Later messages are sent whenever the state store publishes a snapshot with
  a `revision` strictly greater than the last one sent; equal or older revisions are skipped.
- Client to server: nothing is expected. The local (non-remote) handler ignores incoming messages; the remote handler treats any data message as a protocol violation (`4403`).
- Golden transcript: the step "ws: initial snapshot, then a snapshot after start_next" in `contract/transcripts.json` (first snapshot, then `POST /api/v1/actions/start_next`, then the next snapshot with the timer running).
