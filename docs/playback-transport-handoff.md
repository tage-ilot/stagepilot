# Playback transport integration handoff

This slice is stdlib-only and does not alter existing MIDI code or dependencies.
Reference: huntrw6/Playback-Application-Programming-Interface, `PROTOCOL.md` and
`playback_api/{ws,normalizer,client,find}.py`. Only receive-side semantics are
ported; no song-ID threshold, passive order inference, command dispatcher,
section discovery, or general text-send method exists.

## Typed contracts

- `client.ConnectionOptions(enabled, host_override, port, auto_scan)` is independent
  of persistent settings. Adapt `PlaybackApiSettings.host` to `host_override`.
- `client.PlaybackClient` exposes async `start`, `stop`, `reconfigure`, and `find`,
  immutable `status`, and bounded `events` (default 100). Integration owns the
  plugin `health` adapter, action queue, persistence, and API routes.
- `PlaybackStatus`: connected, host, port, source (`manual|loopback|lan`),
  last_error, heartbeat. Connected means the current connection received a valid
  heartbeat, not just an HTTP upgrade. Errors are categorical/privacy-safe.
- `Heartbeat`: song_id (opaque int), position (finite nonnegative seconds), playing,
  pad, setlist_version (int or None). Missing/malformed version is None; downstream
  mapping must not treat it as authoritative.
- `PlaybackEvent`: typed event name/timestamp, song_id, position,
  previous_song_id, continues_playing, playing, pad, setlist_version,
  previous_version, section_id, active, reason, message_kind. No song_number.
  Timestamps from the client are monotonic observation times, not UNIX dates.
- `observer(status, tuple[PlaybackEvent, ...])` runs synchronously on the asyncio
  loop. It receives status updated BEFORE the batch, including every valid
  heartbeat even when the event tuple is empty. Each valid heartbeat creates a
  NEW Heartbeat object; non-heartbeat messages retain the previous object. This
  lets discovery count continued fresh unchanged heartbeats at setlist ends.
  Keep the callback bounded: enqueue integration work, do not block the loop.
  Observer exceptions do not kill the transport; integration owns diagnostics.
- Version-change observation is first in a heartbeat's event batch; integration
  should validate the supplied heartbeat before ALL events in that batch.
  Natural continuing transitions emit ended, changed, then started; changed is
  monitor-only. The first heartbeat after every connection is a `state.snapshot`;
  mapping uses it only to align the timer when Playback is playing StagePilot's
  current song (valid order), otherwise it is monitor-only.

## Transport and discovery

`ws.WebSocket` validates status 101, accept hash, Upgrade/Connection headers, and
exact `pr-protocol` selection. No auth header. Partial/fragmented text frames and
interleaved ping/pong work; masked server frames, invalid fragmentation/binary/text
encoding and messages over 1 MiB fail closed. Buffers survive receive timeouts.
Closing shuts down the socket to wake blocked receives. No application control
is sent by normal receive, reconnection, or address discovery.

`find.find_playback` is blocking and runs via `asyncio.to_thread` from the client:
manual address exclusively overrides loopback/LAN; otherwise loopback first;
if unsuccessful and auto_scan=True, enumerate attached IPv4 networks and probe
bounded /24s (or smaller attached subnets), 32 workers. OS interface discovery
supports ip/ifconfig and Windows PowerShell with bounded subprocess calls. It
never guesses networks from DNS, scans the internet, uses cached unrelated hosts,
or sends Playback commands. Every match requires valid heartbeat plus handshake.

Reconnect re-probes every second (`RECONNECT_PROBE_INTERVAL`, one-second probe
timeout, no backoff), interruptible at shutdown: the manual host, else the last good
address, else loopback. The full scan (`FULL_SCAN_INTERVAL`, 45 s, 3 s probe timeout)
runs only with auto-scan on and no saved address; a manual host never scans. Five seconds without a valid heartbeat disconnects even
if unrelated messages arrive. Shutdown waits for bounded discovery/socket work,
not cancellation that abandons worker threads. Reconfigure closes the old listener
before starting the new one. Mapping trust/revalidation is integration-owned.

## Hunter amendment: narrow discovery hook only

`await client._discovery_step(direction: Literal['previous','next'])` is a private
integration hook with disconnected/playing checks. The underlying WebSocket hook
has exactly two hard-coded masked payloads: transportPreviousSong and
transportNextSong. There is NO generic send method, select-song restoration,
play/pause/seek/section/pad/fade operation, or automatically triggered control.

Task t_003f1dc5 owns the ONLY caller: explicitly operator-confirmed
`discover_song_order()`, stopped/current-heartbeat checks, exclusive execution,
~200-step cap, waits, Previous-only restoration, suppression of ALL actions during
discovery, tagged monitor, complete-only persistence, and routes. Do not expose
the private steps directly as routes. Persist {song_order,captured_version,
captured_at}; validate version/membership per Hunter's root amendment. Reconnect
alone must not invalidate a saved order. Unknown IDs never imply positions.

## Executed validation

On clean main base 4872a5e4fc6b29080ecb1625381bd4ece025d503:

- uv sync --project backend --frozen --extra dev: success, unchanged lock/manifest.
- uv run --frozen pytest tests/test_playback_api_transport.py: 28 passed.
- uv run --frozen pytest: 472 passed, 1 skipped (Windows-only), 1 existing
  Starlette/httpx deprecation warning.
- uv run --frozen mypy: success, 130 source files.
- uv run --frozen ruff check .: all checks passed.
- uv run --frozen ruff format --check .: 130 files already formatted.

Fake endpoints only; no real Playback, LAN scan, command or hardware probing.
Full integration suites/Frontend remain downstream task scope.
