# Playback API input

Playback API is the default input for new installations and for settings migrated
from schema 1/unversioned files to schema 2. Native and network MIDI remain the
unchanged alternative (`integration_modes.midi_source = "real"`). Their saved
port, transport, channel, note, mappings, debounce and private IPC values survive
migration. Select `simulated` explicitly to use neither live input.

## Connection and safety

Enable Playback's remote connections. StagePilot negotiates `pr-protocol` on
`ws://HOST:8080/` and reports connected only after a valid heartbeat. With no
manual host it checks `127.0.0.1` first, then attached IPv4 /24s (or smaller
attached networks) when `auto_scan` is enabled. A manual host, for example
`192.0.2.10`, exclusively overrides both local and LAN discovery. Port defaults
to 8080. Address discovery validates the handshake and heartbeat and sends no
Playback application commands. The copied transport uses only the Python
standard library; no dependency or MIDI transport implementation was changed.

When Playback is not reachable StagePilot reconnects by itself (Remote Connections
turned off and on, a network drop, or a Playback restart). It re-probes the manual
host, else the last known good address, else loopback, once a second with a
one-second timeout: one cheap probe, no backoff. The full network scan is separate
and rare: only when auto-scan is on and no address is saved, and at most once every
45 seconds (probe timeout 3 seconds). A manual host is exclusive: it is probed alone,
never falls back to scanning, and never to MIDI. Reconnecting does not change the
saved address or the saved song order. Five seconds without a valid heartbeat
disconnects even when other messages arrive. Failure
status includes `Playback unavailable; remote connections may be off`. Playback
may disable remote connections when it restarts; re-enable them in Playback.
Find closes the old listener and starts a fresh address search; it does not
change the saved manual override. Disable/source switching and shutdown close
the old transport, wait for its bounded thread work, and discard old queued
observations before starting the next input. MIDI callbacks retain their existing
connection-generation guards. In Playback API mode MIDI is never started:
there is no automatic MIDI fallback and the saved MIDI settings are left untouched.
Explicit `real` mode runs MIDI only; `simulated` runs neither live input.

### One Playback connection and one action owner

The connection follows the selected Connection method and never switches by
itself. In Playback API mode it is connected only while the WebSocket is up and a
valid heartbeat is less than five seconds old; when Playback drops, the status
reads "Disconnected. Looking for Playback…" (never a MIDI message) until the API
reconnects. In MIDI mode MIDI owns and API events are not started. There is no
automatic MIDI fallback and no "Connected through MIDI (backup)" state; the status
projection still checks heartbeat expiry every 50 ms even without incoming events.

In API mode every MIDI action (including simulated MIDI) is ignored with
`ignored: MIDI is not the selected connection method`. Song-order validity never
changes ownership: healthy API with stale/unknown order remains connected and sole
owner, blocks starts/restarts, and allows API pause/stop outside discovery. Resume and seek need a valid order
mapped to StagePilot's current song; otherwise they stay monitor-only.
Discovery suppresses both inputs until exit. Queued work rechecks current ownership
and ownership generations.

A two-second cross-source dedupe window suppresses the same position start or
restart, or timer-stop action across ownership changes. A legacy unpositioned
`start_next` arriving after another source's start is also deduplicated in that
window. Different explicit positions and same-source repeats retain their
existing behavior. Rejected actions do not reserve a successful-action key.

## Discover Song Order: the sole control exception

Opaque song IDs do not encode position. StagePilot never infers absolute song
numbers from passive transitions, sorts IDs, or matches them to plan titles.
The operator must run Discover Song Order while Playback is stopped. Confirm
that the complete Playback setlist order matches the StagePilot plan order.
Number N is the discovered list index plus one (the existing MIDI position path).

The confirmed operation sends only Previous/Next, `transportReturnToStart`, and
`waveformSeek {"sequenceTime":86400.0}` over the current connection. It remembers the original
selection, walks Previous to the nonwrapping start, walks Next to the end, then
restores the original selection using Previous only. No select-song command or
generic send/control API exists. The operation is capped at 200 total navigation
steps (including the walk to the start and restoration), with five seconds per
step. A very long setlist/original selection may exceed that cap and fail safely.
At an end, two fresh unchanged heartbeats and continued recent receipt
are required; a silent endpoint is a timeout, never an empty/successful result.
The usual two-second song changes make runtime depend on setlist
length; probing both ends may each take five seconds.

On the forward walk, each stopped song is returned to zero (confirmed by a fresh
heartbeat), sought past its end, and measured from two steady fresh heartbeats.
It is then returned to zero before navigation continues. Failed/no-effect length
reads become null and do not discard a successful order. Nothing is played: no
Play, Fade, or Select command is sent, so scanning does not fire Playback MIDI cues.
Playback lengths are read from where each song ends (the start of its last measure).
Countdowns and lighting still use Planning Center durations.

All Playback-triggered StagePilot actions are suppressed from discovery entry
until exit, including starts and stop/pause actions. Monitor events carry
`discovery: true`; pre-operation and discovery queue envelopes cannot dispatch
later. Playing/disconnected/already-running requests are rejected. Playback
starting to play, version changes, wrapping navigation, loss of connection,
timeouts, shutdown, or failure to restore abort without saving a partial result.
On failure, check the selection in Playback; restoration cannot be promised if
the transport failed. Shutdown cancels and joins the operation before transport
cleanup. Discovery cannot be started by startup, reconnect, Find, or settings.

Only a complete restored result atomically saves
`{song_order, captured_version, captured_at, song_lengths, lengths_measured_at}`.
Lengths align by index, with null for an unreadable song; an empty list means
unknown. Order replacement or stale setlist validation clears lengths.
Progress counts all navigation/measurement commands; song X of Y uses the trusted
previous order count when available (the first walk's total is unknown until its end).
Start/restart actions require the
current heartbeat version to match and every observed heartbeat ID to belong to
the saved order. Changed/missing version or an unknown ID makes the order sticky
stale; matching later observations do not silently rearm it. Rediscover to repair.
Reconnect alone does not invalidate: its first heartbeat is snapshot-only and
revalidates the saved order. The protocol does not prove unique setlist identity
or detect every same-version reorder with the same IDs. Rediscover after known
setlist/plan reorder changes; StagePilot does not claim that stronger guarantee.

## Event mapping

| Observation | StagePilot action |
| --- | --- |
| `song.started`, known non-active position, valid order | `dispatch_song_position(N)` |
| `song.started`, already active position, valid order | `restart_current` |
| `song.paused` outside discovery, valid order and current song | `stop_timer_at(position)`: stops and keeps the remaining time |
| `song.paused`, otherwise (stale order, other song, no position) | `stop_timer`, even while stale |
| `song.stopped` outside discovery | `stop_timer`, even while stale |
| `song.resumed`, valid order, Playback song is StagePilot's current song | `resume_timer_at(position)`: countdown continues from the Playback position |
| `seek`, same conditions | `seek_timer(position)`: remaining time re-applied from the new position; keeps running if running, otherwise only the frozen value changes |
| First heartbeat after a reconnect, playing, same conditions | `resume_timer_at(position)` (alignment); ignored when Playback is not playing |
| `song.resumed`/`seek`/reconnect, stale order or a different song | Monitor only |
| selected/changed/ended, section, jump, fade, pad and unknown | Monitor only |

Stop and pause are the same state: there is one stopped timer status, not a separate
paused one. A stop (Playback pause or stop, or the manual Stop timer control) keeps the
remaining seconds and song position instead of blanking them; the dashboard keeps
its existing stopped wording and shows the frozen remaining time. Reset position
still clears it. Remaining time is the Planning Center duration minus the Playback
position, rounded and clamped at 0. A song start at position under two seconds
(seek to start, then play) is still the existing song-started restart. Heartbeats are
one second apart, so about one second of timing error is expected.

Consumers: the presentation countdown holds on stop and, on resume or seek, sets the
remaining seconds and starts it again (seek while stopped only updates the held value).
Lighting sends nothing while stopped; after a resume or seek it sends only cues that
are still upcoming from the new position, never a burst of earlier ones. MIDI has no
distinct pause message, so a MIDI stop is a stop and there is no MIDI resume.

Continuing song changes emit changed then started; changed never double-fires an
action. Duplicate unchanged heartbeats do not create another start. All actions
use the existing StateService pipeline, outside receive-worker threads.

## Backend API for the existing Playback configuration window

All routes use the existing dashboard authentication/remote retry boundary under
`/api/v1/playback-api`. There are no play, pause, select, seek or individual
Previous/Next routes. Keep controls inside the existing MIDI/Playback settings
widget, with MIDI in the initially collapsed "Alternate connection: MIDI settings" disclosure (frontend task).

- `GET /status`: typed network/configuration/discovery snapshot below.
- `GET /connection`: only the unified `{active_source, sources, connected, reason}`.
- `GET /events`: `{events: [{event: PlaybackEvent, discovery: bool}], capacity: 100}`.
  Oldest-first bounded recent receive observations, including monitor-only ones.
  Poll while the configuration window is open; replace the list, do not append
  snapshots. Event `timestamp` is monotonic observation time, not a UNIX date.
- `PUT /settings`: `{enabled: bool, host: string|null, port: int, auto_scan: bool}`.
  Omitted fields use enabled=true, host=null, port=8080, auto_scan=true. Supply all
  four fields from the current status when changing one. Saves and applies live;
  does not select the source or modify/arm song order. Extra order/trust fields are
  rejected (422), invalid endpoint syntax is 422, persistence failure is 503,
  discovery conflict is 409. An empty manual field should be sent as null.
- `POST /find`: starts a fresh asynchronous receive/address search with the saved
  policy; returns the initial status immediately, normally disconnected while
  probing. Poll `/status` for the final network outcome. Selected+enabled required
  (409 otherwise); while discovery runs, returns 409. Sends no application control.
- `POST /discover-song-order`: JSON `{confirm: true}` is mandatory. Missing body,
  missing/false/nonboolean confirmation returns 400. Live-state conflicts return
  409 with plain-language detail. This request remains open until discovery ends;
  poll `/status` concurrently to show progress and prevent duplicate clicks. A
  completed or safely failed run returns 200 with `discovery: done|failed`; inspect
  `last_error` rather than treating every 200 as discovery success. HTTP disconnect
  or application shutdown may cancel it; the status records failed/no result saved.

Status fields:

```text
selected: bool                 source is playback_api, independently of enable
 enabled: bool                 persisted/effective on/off setting
 connected: bool               either verified live input is connected
 active_source: playback_api|midi|none
 sources: {playback_api: {connected, reason}, midi: {connected, reason}}
 reason: string                plain-language unified connection/action eligibility
 host: string|null             current/last attempted endpoint, not manual setting
 port: int
 source: manual|loopback|lan|null
 last_error: string|null        discovery/action error takes precedence over network error
 playing: bool                 latest heartbeat; false without one
 setlist_cloud_version: int|null
 discovery: idle|running|failed|done
 progress: int                 navigation commands issued, not estimated percentage
 song_order: int[]              complete saved/discovered opaque IDs
 captured_version: int|null
 captured_at: ISO8601|null      UTC capture time
 stale: bool                   start/restart disarmed; initially true without order
 settings: {enabled, host, port, auto_scan}
```

Read full settings at `GET /api/v1/settings`; change
`integration_modes.midi_source` through `PUT /api/v1/settings` to select
`playback_api`, `real`, or `simulated`. Source-only/MIDI/Playback updates apply
without restart and report `restart_required: false`. Changing the MIDI input,
its availability or the MIDI source while the timer runs is refused with 409
(`Stop the timer before changing the MIDI input.`); an environment pin
(`STAGEPILOT_MIDI_SOURCE`, `STAGEPILOT_MIDI_ENABLED`, `STAGEPILOT_DEMO_SIMULATE_MIDI`)
is reported by variable name. `POST /midi/inputs/refresh` re-scans live; a saved
input that appears later connects on its own, and a removed one reports
Disconnected and reconnects when it returns (existing supervisor behaviour). MIDI selection and
monitor routes remain available while API is selected (MIDI itself stays stopped); cue
simulation still requires explicitly selecting MIDI. No stopped/old controller
can bypass ownership checks. Both settings
routes refuse order/capture edits; only completed discovery produces those fields.
Settings writes/Find are serialized against discovery and cannot persist a
configuration change after returning a discovery conflict. Other aspect-specific
settings routes and unrelated generic settings writes do not re-install/rearm the
Playback mapping.

The legacy application `midi_status` readiness slot represents the unified
Playback connection; only the arbiter projects connection changes there. The
dashboard state and unified status therefore cannot report one source's failure
as a disconnected Playback while the other is connected. `/playback-api/status`
exposes the active owner and both sources; use `sources.playback_api.connected`
for API-only controls such as discovery, not top-level `connected`. Legacy fields
remain available, but API endpoint/playing/order fields still describe the API,
not MIDI. A disconnected non-owner's transport error does not become a top-level
error on the healthy unified connection. Plugin health describes lifecycle-task
health, not proof of connectivity. The input lifecycle is registered under
`midi_playback` for initial real MIDI and `playback_api` otherwise; its managed
name remains stable while switching source.

## Verification and limits

All transport/discovery/integration tests use synthetic IDs and fake loopback
WebSockets. The application-test default finder is disabled by an autouse fixture
so migration does not make unrelated tests scan a real production LAN. No real
Playback/hardware validation, deployment, release or merge has been performed.
See `playback-settings-contract.md`, `playback-transport-handoff.md`, and
`playback-mapping-handoff.md` for slice contracts. Consolidated verification is
recorded in the integration handoff, not inferred from older slice counts.

## Local Network access on macOS

If Scan Network reports that macOS blocked StagePilot, open **System Settings >
Privacy & Security > Local Network** and turn on every StagePilot entry
(including `stagepilot-backend` if listed), then quit and reopen StagePilot. The
scan result's "Copy diagnostic details" and "Send logs to developer" include the
last scan (networks, probed counts, per-class errors, loopback/gateway self-test).
Backend log lines are `playback_scan_start`, `playback_scan_result`,
`playback_connect_failed` and `sidecar_identity` in
`~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`.


## Explicit Planning Center song-time updates

A scan only saves Playback order and measured lengths. It never writes to Planning
Center. **Update Planning Center** previews the next upcoming plan in the chosen
category; only the subsequent confirmation writes plan-item `length` attributes.
The category menu saves its choice immediately, independently of the settings Save
footer. Upcoming discovery shares the existing lookahead and preferred-time logic.

Measured lengths are rounded to the nearest second, half up, with no difference
tolerance. Equal rounded values, unmeasured positions and non-song items are skipped.
One preview and confirmation covers all measured songs.

The read-only preview expires after five minutes, is single-use and is bound to the
saved scan and category. Every write checks the item again for conflicting edits,
keeps a durable write-ahead undo record and verifies the remote value. Restore skips
items edited since StagePilot wrote them. Uncertain or denied restores retain their
records for a later retry. Credentials are never included in records or results.
401/403 are ordinary permission failures; 429 observes Retry-After once.

If the written plan is the loaded plan, StagePilot requests one `RELOAD_PLAN` and
verifies its durations. A running timer defers reload until idle; this feature
publishes no timer or light actions. Updating another plan never switches the
loaded plan. Permission, conflict or reload failures do not invalidate the scan.

Endpoints under `/api/v1/playback-api/planning-center-lengths`:

- `GET /categories`: active categories (read-only).
- `PUT /category`: `{ "service_type_id": "42" }`, immediate local persistence.
- `POST /preview`: read-only target-plan rows and a short-lived preview token.
- `POST /confirm`: `{ "token": "…", "confirm": true }`.
- `POST /restore`: `{ "confirm": true }`.

Song-scan progress is smoothed locally with a monotonic, time-driven fill. Real
step events raise a blended target without setting the fill directly. The fill
keeps moving below 90% until actual completion, then finishes and briefly holds.
Failure/cancellation never produces a fake 100%. Reduced motion uses small plain
increments; the unknown-count bar omits determinate ARIA values.
