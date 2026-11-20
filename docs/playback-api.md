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
Playback application commands.

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

Opaque song IDs do not encode position. StagePilot never sorts IDs or matches
them to plan titles. A scan is the only way to get the exact order and measured
lengths: run Discover Song Order while Playback is stopped, after confirming
that the complete Playback setlist order matches the StagePilot plan order.
Number N is the discovered list index plus one (the existing MIDI position path).
A scan finishes (or cleans up after itself) even if the browser that started it goes away.

In the Playback panel the scan is the **Scan Playback Songs** button in the header row of the **Song order** section
(its confirmation dialog is titled "Scan Playback Songs?", and the activity list marks events as "while scanning
Playback songs"). The section can be folded with its chevron; the choice is remembered on this device and the section
starts expanded. A folded section still shows a one-line summary ("Saved - 6 songs", "Learning - 2 of 6 placed",
"No order yet") and the scan button.

### Scan again in the hour before the service

Timings are often changed at the last minute, and a fresh scan also corrects a song order StagePilot only learned, so
scan Playback again **within the hour before the service**. Nothing starts a scan for you and a scan writes nothing to
Planning Center by itself. To remind the operator the dashboard's header status pill lists **Scan Playback again**
(a warning row; the pill reads it when nothing else is wrong) when all of this is true: Playback is the connected
source and is not playing or scanning; a Planning Center plan is loaded; the next service start on it (its
`service_times`, read on the plan's date in the saved time zone) is less than 60 minutes away; the last completed
scan (`captured_at` of the Playback status) is older than the start of that hour; and **the plan is not going**.

The reminder stays quiet while the loaded plan is going. Two different things count, and a sound check is not the
service:

- **A service in progress.** From a service's start time until the plan's last song has ended, including the time
  between songs (a sermon between two songs stays quiet). A run of songs that was still playing when the start passed
  counts as part of the service from the start on, so there is no gap.
- **A run-through that is not part of a service in progress** (a sound check or rehearsal whose songs stopped before the
  next service start, before the first service or between two services). It counts as going only while a song is
  playing and for **5 minutes** after the last song activity (the timer stopped, or the countdown reached 0:00). After
  5 minutes with no song the run is over and the reminder returns if it is due and no scan has completed since; a song
  played later starts a new run. A run-through that plays the plan's last song to its end is over as soon as that song
  has ended (no 5-minute wait). So one or two songs at 40 minutes before the service hide the reminder only while they
  play and for 5 minutes after, not for the rest of the hour.

The dashboard reads the end of a service from the last song's own countdown: the song is the plan's last one, it has a
length, and its countdown has reached 0:00 (or the timer stopped with nothing left). A song paused part-way is not the
end. When the app cannot tell (the last song has no length, or no song has played since the service began) it uses an
estimate: the service start plus the sum of the song lengths plus 30 minutes. In every case a service stops counting as
going 3 hours after its start (or after the first song of the run that was playing at the start), so the reminder can
never stay hidden for good. If the operator skips the last song of a real service the end is never seen and the service
is counted as going until that 3-hour limit. After the plan is over the rule is the plain one again: a later service of
the same plan that is still ahead, with Playback not scanned since the hour before it began, brings the reminder back
(that is the moment to scan between two services).

What the dashboard remembers (while the page is open, from the state it already receives; no API change): when the
first song of the run was started, when the last song ended, and the time of the last song activity.

Any number of services work the same way. The start times are sorted and repeated times count once; the reminder is
always about the next start that has not happened yet, whether the services are an hour apart, closer than that, or
overlap the song time (a start that passes while the plan is going simply needs no reminder). One service a Sunday: the
reminder shows in the hour before it, is quiet from the start to the last song, and is not shown again.

The row goes away by itself when a scan completes, when the service start time passes, or when the plan starts going.
A scan that is out of date for another reason (the setlist or the plan changed) keeps its own notice, and this row
is not added next to it. It is worked out in the dashboard from the status it already has; the API did not change.

### Learned order (provisional, before a scan)

With **Learn the order as songs play** on (the default; `playback_api.learn_order`),
StagePilot may pair a Playback song with a plan position while no valid scanned
order exists. The Playback panel labels this "Learning the order as songs play
(N of M placed)"; a scanned order reads "Order scanned"; with neither it reads
"No order yet - set Up next or scan". A scan always replaces the learned order.

When a song starts, the first matching rule applies:

1. A song already bound to a plan position keeps that position (starting it
   again is a restart).
2. An auto-advance from a bound song takes the next plan position.
3. Otherwise the song takes the plan's Up next position, if that position is not
   bound to another song. Set Up next (Previous/Next) before pressing play.
4. Otherwise it is not placed: the panel says so and nothing is started.

A new binding is tentative until the song has played for 10 s of heartbeat time.
Stopping, pausing or starting another song before that releases it and puts Up
next back, so accidental plays do not consume plan slots. Learned bindings reset
when the Playback setlist id or version changes, when a different plan is loaded,
on a conflict (one song, two positions or the reverse) and when the switch is
turned off. Pause, resume and seek use a learned binding like a scanned one.
A song whose length differs from the Planning Center length by more than the
larger of 20 s and 15% is marked doubtful; the panel then suggests a scan.

Known limits: extra Playback songs that are not in the plan shift later
placements (use the Skip arrangement with a scan); connecting mid-service needs
Up next set correctly; a Planning Center song without a length still cannot
count down. Older StagePilot versions reject the `learn_order` setting key.

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

### Heartbeats with no song yet

While Playback loads a setlist it keeps sending valid heartbeats (position, transport and pad state, setlist version) but omits `setlistSongID`. StagePilot treats such a beat as liveness only: the connection stays up and the 5 s heartbeat timer resets, the status reports `loading_setlist`, and no song, state or transport event is produced and nothing is queued. It is not an unknown song id, so a saved Discover Song Order stays valid. Discover Song Order, song-length measurement and any control that needs a current song refuse with "Playback is still loading the setlist." until a beat with a song id arrives. A beat whose `setlistSongID` is present but malformed is still rejected.

## Event mapping

| Observation | StagePilot action |
| --- | --- |
| `song.started`, known non-active position, valid order | `dispatch_song_position(N)` |
| `song.started`, already active position, valid order | `restart_current` |
| `song.paused` outside discovery, valid order and current song | `stop_timer_at(position)`: stops and keeps the remaining time |
| `song.paused`, otherwise (stale order, other song, no position) | `stop_timer`, even while stale |
| `song.stopped` outside discovery | `stop_timer`, even while stale |
| `song.resumed` of the song just selected in Playback (`song.selected`, no start or song change since), when that song is not StagePilot's current song, valid order (or learned order) | Handled as `song.started` at that position: `dispatch_song_position(N)` with the Playback position and clock, so the song becomes current and the countdown, ProPresenter timer and lights start from the jump position |
| `song.resumed`, valid order, Playback song is StagePilot's current song | `resume_timer_at(position)`: countdown continues from the Playback position |
| `seek`, same conditions, and not after a Playback section loop or jump (monitor-only) | `seek_timer(position)`: remaining time re-applied from the new position; keeps running if running, otherwise only the frozen value changes |
| First heartbeat after a reconnect, playing, same conditions | `resume_timer_at(position)` (alignment); ignored when Playback is not playing |
| `song.resumed`/`seek`/reconnect, stale order or a different song (not just selected) | Monitor only |
| Position safety net: a playing heartbeat more than 3 s from the running timer's position, twice in a row, same conditions | `seek_timer(heartbeat position)` (a `seek` event with reason `resync`) and a `playback_position_resync` log line with `app_position` and `playback_position` |
| selected/changed/ended, section, jump, fade, pad and unknown | Monitor only |

How a song is recognised. A play inside a song counts as `song.started` only below 2 s (the
usual play from the top, or after a return to the start); further in it is `song.resumed`.
StagePilot also treats a `song.resumed` as a start when it is the first play of the song that
was selected last in Playback (`song.selected` for that song, nothing started or changed
since) and that song is not StagePilot's current song: select a song, jump into it and press
play, and StagePilot switches to that song and follows it from the jump position (it is not
reset to 0). This is a deliberate difference from the Python app, which dropped that resume
and kept the previous song; the Rust port had inherited the rule. A resume inside the current
song, a resume of a song that was not just selected, auto-advance, navigation without play,
pause and stop behave as before. A resume of a song that is not in the saved or learned order
stays ignored; it is logged once per song as `playback_resume_ignored`.

Pause, jump, play: a `transportPlay` shows the resume at once (provisional) and the
heartbeat that confirms it arrives later. A `waveformSeek` made while the player is not
playing is remembered, and a play that follows uses that position until a heartbeat
agrees with it (or 3 s have passed with heartbeats that disagree), so the resume carries the
jump position and not the position of the last heartbeat from before the jump. A seek made
while playing never changes the position of a later pause. Normal playback drift (under 1 s) never
triggers the safety net, and it never acts while an event or queued action is being handled.

Section loops and section jumps stay monitor-only and never move the timer. When a playing
heartbeat of the current song moves more than 1.5 s away from the clock (a `position.jump`,
or a `section.loop` / `section.jump` event), the safety net is suspended for that song, so a
loop that repeats every 22 s does not pull the countdown back each time. The next start,
resume, seek, snapshot or song change re-arms it, so pause, jump, play is still corrected.

Stop and pause are the same state: there is one stopped timer status, not a separate
paused one. A stop (Playback pause or stop, or the manual Stop timer control) keeps the
remaining seconds and song position instead of blanking them; the dashboard keeps
its existing stopped wording and shows the frozen remaining time. Reset position
still clears it. Remaining time is the Planning Center duration minus the Playback
position, rounded and clamped at 0. A song start at position under two seconds
(seek to start, then play) is still the existing song-started restart.

Countdown accuracy. The countdown is anchored to Playback's song clock, not to the moment
StagePilot finishes handling the event. The first playing heartbeat of a song reports a
position (0.57 s on the rig, up to 2 s for a start StagePilot still accepts); the Song event
carries that `position_seconds` and `observed_at` (when the heartbeat was received), and the
countdown starts at `observed_at - position`, so the clock agrees with Playback instead of
running that much behind. Resume and seek (`resume_timer_at`, `seek_timer`) take the same
receive time as the moment the new position was true. Remaining error: Playback sends
heartbeats about every 100 ms, so the reported position can be up to one heartbeat interval
older than Playback's clock when it arrives (worst case about 0.1 s, plus network delay, which
is not measured and not corrected), and the ProPresenter countdown is started by a separate call
that takes a few tens of milliseconds more (not corrected; the displayed StagePilot clock
and the lights are anchored to Playback, the ProPresenter countdown is not shifted). A
heartbeat older than 10 s, or in the future, is not used and the timer falls back to the old
behaviour.

Elapsed and remaining on the Now playing card are both derived from one position (song length
from the playing song; position = the timer's `position_seconds` plus the time since
`started_at` while running, the frozen position or remaining time while stopped), so
elapsed + remaining always equals the song length, also after a resume or seek (the backend
re-bases the timer's `duration_seconds` to the time left, which is no longer the song
length). Past the end the card holds 00:00 remaining and the full length elapsed; the
ProPresenter countdown's own negative count-up is unchanged.

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
  `last_error` rather than treating every 200 as discovery success. The scan runs in
  its own task, not in the request: a client that disconnects (a reload, a closed tab,
  a locked phone, a flaky tunnel) does not stop it, and it still returns the song to
  the start and the original selection and ends as `done` or `failed`. Application
  shutdown, or a hard stop of the task, ends it as `failed` (no result saved) rather
  than leaving it `running`.
- `POST /song-order/refresh-from-library` (a Rust-only extension): the "Scan Playback Songs" button when the Library Link works. No body. It reads the helper fresh and never touches Playback's transport (see "Playback library"). 200 with the status (`order_source` `library`, or `order_library_note` saying why not); 409 when a scan is running, the Library Link cannot be used or the order cannot be saved just now.
- `POST /discover-song-order/cancel` (a Rust-only extension): asks a running scan to
  stop. It returns the status at once; the scan walks back to the original selection,
  saves nothing and ends as `failed` ("Song order discovery cancelled…"). Harmless when
  no scan runs.

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

## Playback library (optional)

A small helper (`huntrw6/playback-library`, a Python script, port 8780, `GET` only) can run on the Playback computer and tell StagePilot what Playback knows about each song of a setlist: the key it is played in (and the original), the BPM, the song's length, the arrangement's name and its sections. In StagePilot it is called the **Library Link**. It is **optional** and nothing else in StagePilot depends on it. Scanning, pairing, timers, lights, ProPresenter and Planning Center behave exactly as before whether it is installed or not, and a computer without it shows no warning anywhere.

**The Playback Live Link block.** The first block of the Playback API panel is built like the Lighting "sACN DMX Live Link" block. It has two separate status lines, each a dot and words: `Playback API: Connected to Playback at 10.30.0.141` (or Searching, Not connected, MIDI) and `Library Link: Ready, version 0.3.0` (or `Not installed`, `Update available, version 0.2.0`, `Needs the command again`, `Not answering`). The pill beside the title summarises them: `Connected` (both), `API only`, `Script only`, `Not connected`. The Scan Network button, the progress, the computer chooser, "Other computer" and Connect are in this block; choosing a computer sets the Playback API host and, with it, the Library Link's address (the script is on the same computer, port 8780).

**The address.** The Library Link is asked at the saved Playback computer (`playback_api.host`), port 8780, so when the API works on 10.30.0.141 or 10.20.0.102, so does the script. There is no address box: the Library Link is always on the Playback computer. (The setting `playback_library.address` still exists in the file for older installs; the window clears a value left by an older version once, and choosing another computer clears it.) Nothing is asked until a Playback computer is saved. There is no switch: the script is probed whenever a computer is saved.

**The key.** The script answers only requests signed with a key StagePilot makes (32 random bytes, base64url, the operating system's random numbers), once, the first time a setup session is opened. The key is kept in the credential store (the keychain, like the Planning Center secrets), never in `settings.json`, never returned by any route and never logged (its name, the value and the `X-PBL-Sig` header are redacted). Every request to the script carries `X-PBL-Time` (unix seconds) and `X-PBL-Sig` (hex HMAC-SHA256 of `GET\n<path and query>\n<time>` with the key); the script accepts a time up to 120 s off and answers a bad signature with 401 (and 429 for 60 s after ten of them). **The key is made once and kept**: there is no way to forget or replace it from the window, and running the setup command again on the Playback computer reuses the same key (it is only the helper that is installed again). An old settings file with `playback_library.enabled: false` (an earlier version could write it) loads without a problem and means nothing now: the Library Link's state comes from what the script answers (`not_installed` when nothing answers and it was never in use), exactly as for a file without the field. The field is still read and written back unchanged, so an older StagePilot can open the same file.

**The song matchup.** The "Song arrangement" block of the Song order section reads the library's setlist for the loaded plan's date (the newest of that date; `GET /api/v1/playback-library/plan`), and pairs plan songs with its songs by title in three steps, each looking only at what the step before left: **exact** (the same title once case, punctuation, accents, "&", a thousands comma and a "(Live)" ending are set aside), **close** (the same once every bracket and a "feat." tail are also set aside) and **uncertain** (one is the other's first words, or they differ by one or two letters, or they share most of their words; one to one both ways; never used for a cover, key or waveform). A title that is on one side more than once is not paired. The answer also lists the setlist's songs that go with no plan song (`playback_only`) and, when Playback has no setlist for the date, its other setlists (`setlists`, newest first, each with Playback's own number); `PUT /api/v1/playback-library/setlist {"setlist_id": N | null}` makes the loaded plan use one of them (kept in memory for that plan only; `null` goes back to the date). The library never writes the saved song order: that is still only a scan (the scan's `setlistSongID` and `captured_version` come from Playback's own heartbeats, which the library cannot vouch for). When the scan's song numbers are the library's `setlist_song_id` numbers, the window ties them together by number; when they share none and the counts are equal it ties them by place and says so; otherwise it shows the scan alone.

**The states** (`GET /api/v1/playback-library/status` has `helper: {state, version, bundled_version, message, paired, updater?, last_update_error?, update?}`):

| State | When | Library features |
|---|---|---|
| `not_installed` | nothing answers (refused, timeout) and never paired, or no Playback computer chosen | no, and no warning anywhere |
| `needs_key` | 401: the script does not know this StagePilot's key (or its key is under 32 characters) | no; "Needs the command again" |
| `outdated` | answers without a key check, or is older than the bundled version | yes; an update is offered |
| `ready` | valid signature, version current | yes |
| `unreachable` | was in use, now silent | no; the one state with the quiet header-pill notice |

Only `ready` and `outdated` feed the library (the chips, the arrangement bar, the notices and the timeline song map). Every other state looks like a build without the library.

**Setting up the Playback computer.** The block's **Set up the Playback computer** button (**Update the Library Link** when outdated) opens a setup session like the Lightkey helper's: `POST /api/v1/playback-library/helper-install {address?}` returns `{command, url, expires_at, lan_ip, port, bundled_version}` and `DELETE` ends it (the address is the local address that routes to the Playback host; when the Playback host is this computer it is this computer's own private network address, so nobody is asked; only a host name that does not resolve, or no network at all, gives 409 `no_lan_address`, one plain sentence and a Try again button; the optional `address` in the request is still accepted but the window no longer asks for it). The command is only `curl -fsS http://<this computer>:<port>/playback-helper/<one-time token>/install.sh | sh`: it contains no key. It works for 30 minutes, from this local network only (never through a remote tunnel), and ends itself when the script answers StagePilot with a valid signature. The served `install.sh` is a small POSIX `sh` wrapper that downloads the three bundled files from the same one-time address, checks them against the bundled `SHA256SUMS`, writes the key to a file only its owner can read (mode 600, in a private temporary folder), runs the bundled `install.sh --key-file <file>` and removes the folder. It needs no internet. The files are in `crates/stagepilot-playback/assets/playback-helper/` (`playback_library.py`, `install.sh`, `SHA256SUMS`; `BUNDLED_HELPER_VERSION` is `0.3.0` and a test checks it against `__version__` and the sums against the files). To try it without Playback, see `docs/preview-live-link.md`.

**What StagePilot shows.** The loaded plan's songs are paired with Playback's setlist **for the plan's date** (`GET /api/v1/playback-library/plan`). If Playback has none for that date StagePilot says `Playback has no setlist for <date>; its newest is <title> from <date>.` and never falls back to the newest silently. A pair is made by title: lower case, accents removed, `&` as `and`, punctuation and repeated spaces ignored, and an ending of `(Live)`, `[Live]` or `- Live` (also `- Live at ...`) ignored, so `A / B` and `A/B` agree. It must be the only song with that title on **both** sides; otherwise the song is "not matched" (the same song twice in the plan is never matched). Neither list is reordered. Each matched song shows on its chip in the Songs strip (Lighting panel) and in the Playback API panel: key (and `was X`), BPM, length as `m:ss`, number of sections and the arrangement. In the Playback API panel each plan song is a **chip** (see "The chips" below).

**The length is where the countdown ends.** Countdown length: the start of the song's last measure; if that is more than 6 seconds before the end of the song, or the song has no bar lines, the end of the song. StagePilot's countdowns end at the start of the song's last measure, because that is where the song goes quiet, so the library's length is first the helper's `scan_length_s` (what StagePilot's own scan measures). It is used unless (1) the song has no beat grid (no `scan_length_s`), or (2) it is more than 6 seconds before the helper's `length_s` (the end of the song, the sum of the arranged sections; compared at three decimals, exactly 6 seconds keeps the last measure), or it is after it (inconsistent data); then the end of the song is used. A song with only a last measure uses it; a song with neither shows no length and raises no notice (a missing, non-numeric, negative or absurdly large value counts as missing). The chip's hover text says why when it is the end of the song: `Playback has no bar lines for this song, so this is where the song ends.` or `This song's last measure is longer than 6 seconds, so this is where the song ends.` The rule is one function in the backend (`countdown_length`, limit `LAST_MEASURE_LIMIT_S`); the window gets the one resolved number as `countdown_length_s` and `length_source` (`last_measure`, `song_end_no_grid` or `song_end_long_last_measure`), and `scan_length_s` is still passed as the helper gave it. The length is only shown: it is never used to change the plan, the scan result or a timer.

**The chips.** While the helper has current data, the Playback library section shows each plan song, in plan order, as a chip in a grid of columns at least 270 px wide: a 28 px **cover** on the left (only when Playback holds a picture for the song), the title, and under it `key · BPM · m:ss` (the countdown length above; the key reads `Ab (was G)` only for a day after StagePilot noticed the key being changed in Playback, see "Key changes" below). Under both is a thin **bar of the arrangement**: one block per section, as wide as the time the section plays (repeats are separate blocks), tinted the way Playback tints them (30 % of the section's color over the chip, a 3 px line in the full color). There is no text on the bar: hover a block for its name and times (`Verse 1 · 0:18–0:47`); a screen reader hears the whole bar as one picture (`Arrangement: Count Off, Intro, ... and 18 more (30 sections)`). The number of sections and the arrangement's name are in the chip's hover text. A song Playback is missing, or cannot tell apart, keeps its words as a chip with no cover and no bar; so does everything when the helper has no current data (the status words stand instead). The section colors are one fixed table (Playback's, the same in every song; the helper does not send it) kept in `frontend/src/components/sectionColors.ts`, looked up by the exact section name; a name it does not know is grey, and plain `Vamp` is red while every `Vamp on ...` is blue. A helper older than 0.2 sends no pictures: the chips then have no cover.

**MultiTracks number.** Every `LibrarySong` carries `content_id`, the song's MultiTracks number as the helper's setlist and song answers give it (`content_id`; null when the helper gives none or an odd value). It is the same number as inside the song file ProPresenter has, which is how the ProPresenter Link finds the song there (`docs/propresenter.md`, "Adding Playback songs to a playlist"). Nothing else about the library changes.

**Key changes.** The helper reports a song's `key` and `key_original` but not when a key was changed, so StagePilot notices it itself, with no extra request: each time a setlist's songs are read it compares every song's `key` with the one it saw the time before for the same `setlist_song_id`. A song seen for the first time is only remembered (no history, so no "was"); the same key does nothing; another key records `key_was` (the old key) and `key_changed_at` (now, UTC, RFC 3339) and remembers the new key; a change back to an earlier key is a new change. Both fields are on every `LibrarySong` of `GET /api/v1/playback-library/plan` for as long as the change is remembered; the window shows `(was X)` only while `key_changed_at` is less than 24 hours old (it knows the time) and says so in the chip's hover text ("Key changed in Playback from G to Ab in the last day"). `key_original` is still sent but no longer shown. The table is `playback-keys.json` beside the settings file (`STAGEPILOT_PLAYBACK_KEYS_PATH` moves it), written like `service-pin.json` (temp file, fsync, rename), only when a song was seen for the first time or a key changed, never on an ordinary read; it holds only the Playback song number, key names and times, at most 500 songs (the one read longest ago goes first), drops songs not read for 7 days when next written, and a missing or damaged file is ignored (a warning once in the log). Forgetting the Library Link or saving another Playback computer deletes it.

**Cover pictures** reach the window only through StagePilot: `GET /api/v1/playback-library/songs/<setlist song id>/artwork` (the window never calls the helper, which answers only the StagePilot computer). It answers only for a song of the **current paired plan** that the plan says has a picture (anything else is a 404, so the route cannot fetch arbitrary content from the helper), and nothing at all, no request to the helper either, while the Library Link is not ready or outdated (a Library Link that is not active makes no request). The id is read as a whole number and never passed on as text. The picture is asked from the helper with the same limits as everything else here (3 s, no redirect, no proxy), at most 2,000,000 bytes, and only if the type is `image/jpeg` or `image/png` **and** the first bytes are those of a JPEG (`FF D8 FF`) or a PNG (`89 50 4E 47 0D 0A 1A 0A`); anything else is "no picture" (404). **What is cached, and where:** the pictures are kept **in memory only** (never on disk, in the settings, in the plan or in a log), at most 64 pictures and 8 MB together, the oldest out first, and gone when StagePilot quits. A picture held with the etag the plan names is served from memory for 10 minutes; after that, or when the helper's etag changed, the helper is asked with `If-None-Match` (a `304` keeps the picture held). The window gets the helper's `ETag`, its `Content-Type`, `Cache-Control: private, max-age=3600` and `X-Content-Type-Options: nosniff`, answers `If-None-Match` with `304`, and asks for `...artwork?v=<etag>`, so a changed picture is fetched again.

**Waveforms** reach the lighting timeline only through StagePilot: `GET /api/v1/playback-library/songs/<setlist song id>/waveform`. It answers only for a song of the **current paired plan** (anything else is a 404), the id is read as a whole number, and while the Library Link is not active it answers 404 and makes no request to the helper at all. It is asked **only when the window asks**, that is, while the lighting timeline of that song is on screen with the **Song map** switched on; the 30 s check never fetches one. StagePilot then asks the helper `GET /songs/<id>/waveform?points_per_second=8&clock=performed` with the same limits as everything else here (no redirect, no proxy, same address rules), except that it waits up to 6 s (the helper decodes the song's picture the first time) and reads at most 600,000 bytes. The answer is accepted only if `peak` is a list of 1 to 20,000 finite numbers from 0 to 1 and `step_s` is a finite number above 0 and at most 10; anything else (and a 404 from the helper) is "no waveform" and the timeline shows the arrangement bar alone. The window gets only `{"step_s": 0.125, "peak": [0 to 255, ...]}` (each number the peak share times 255, rounded); `mean` and every other field are dropped. **What is cached, and where:** in memory only (never on disk, in the settings, the plan or a log), at most 16 songs, the oldest out first, keyed by the song and by the helper's last good `library_revision` (a new token fetches again; a helper with no token is trusted for 10 minutes), and gone when StagePilot quits. The window gets `ETag` (made from the bytes), `Cache-Control: private, max-age=300`, `X-Content-Type-Options: nosniff`, and answers `If-None-Match` with `304`. The scale of the helper's numbers (linear or dB) is not verified, so the timeline draws them as given, normalised to the song's loudest point, without a unit. All of this is read-only: no write to Planning Center, the saved scan, cues, pairing, timers, ProPresenter or lights, and no Playback control command.

**What the plan carries.** `GET /api/v1/playback-library/plan` adds to each paired song `artwork` (`{type, bytes, etag}` or null; never the picture) and `sections` (`[{name, start, end}]` in seconds on Playback's own played clock, from `arrangement_detail.sections`), next to `section_count` (the number of sections the helper counts). An entry whose times are missing, not numbers, negative, over 86,400 or whose end is not after its start is left out; a name is trimmed and cut at 40 characters (a missing one reads `Section`); at most 200 sections are kept; a song with no usable section has an empty list and no bar. This changes nothing about `scan_length_s`, the countdown length or the saved scan.

**Notices** (plain words; the header pill carries one combined line with the actionable ones only, and nothing when everything agrees; the length is information on the chip and in the panel):

| Notice | Wording | Pill |
|---|---|---|
| Plan song not in Playback | `<Song> is not in Playback's setlist.` | yes |
| Different order | `<Song> is song 2 in the plan but song 3 in Playback.` (judged among songs in both lists) | yes |
| Rental ends before the service | `<Song> is a rental in Playback that ends before the service starts.` (the plan's last service time; else `...before the end of <date>.`) | yes |
| No setlist for the date | `Playback has no setlist for <date>; its newest is <title> from <date>.` | yes |
| Length differs by more than 15 s | `<Song>: Playback counts this song down to 6:29; the plan says 5:55.` | no |

**How it asks.** A window asks every 30 s while it is visible, and the backend asks the helper at most once per 30 s however many windows there are (Check may ask again after 2 s). A window is not the only thing that asks. When StagePilot starts with a key paired (the Library Link was set up before), it connects to the helper by itself, with no window open and without waiting for anything: one task of its own reads the helper's health, then the setlist Playback has open and its songs, fresh, and decides the song order (see "The song order" below). While the helper does not answer it tries again after 2 s, 5 s, 15 s and then every 30 s, until the helper has answered once; this also covers a Playback computer that starts after StagePilot does. Start-up never waits for it, nothing about the key is logged, and a computer with no key paired (the Library Link never set up) is never asked in the background. After the first answer the Library Link counts as in use and a watcher looks every 5 s whether the song order can come from it (the helper itself is still asked at most once per 30 s). Each request is signed (above) and takes 3 s at most (6 s for a waveform), follows no redirect, and an answer larger than 16 KiB (health) or 256 KiB (a setlist or a song) is refused. Each check asks the signed `/health` first (it returns the full body) and reads its `library_revision` (an opaque token that changes whenever Playback's database changes; it is compared for equality only). **The setlist (`/setlists/<date>`, and `/setlists/latest` when there is none) and each song (`/songs/<id>`: sections and cover description) are read again only when** the token differs from the one at the last good read, the plan (its date or its songs) or the setting changed, the last good read is 10 minutes old (a safety net), the last read failed, or a paired song lacks its own data; otherwise a check is one small request. A helper that sends no `library_revision` (0.1.x) is read in full every time, as before. A helper that restarts with the same token is not read again. The check never asks for a waveform or the beats (a waveform is asked for only when a lighting timeline shows that song, see above). A failure is one plain sentence (for example `The Playback computer did not answer.`); the last good answer stands in for up to 60 s and is marked as old. Library data is held in memory only.

**The song order.** The Library Link also gives the song order (and the lengths) without a scan: when the helper is ready, Playback is connected, and the loaded plan's songs are the songs of the setlist Playback has open, in the same places, StagePilot saves that order with `order_source` `library` (the watcher, and the start-up connection above, do it by themselves; a scan you ran stays in charge until the plan, the setlist or Playback's setlist version changes). When any of that fails, the Song order section says why in one line (`order_library_note`) and nothing changes from before.

**"Scan Playback Songs" with the Library Link working.** When the Library Link is ready (or outdated) and its data is current, the button does not step through Playback at all. It has no confirmation and works while a song is playing, because it only reads the helper: `POST /api/v1/playback-api/song-order/refresh-from-library` (a Rust-only extension, no body) asks the helper again right now (its health, the open setlist and its songs, whatever the 30 s gap or an unchanged `library_revision` would say), decides with the same conditions and, when they hold, saves the order as `library` even over an older scan (the press asks for exactly that). It answers the Playback status: `order_source` `library` and no `order_library_note` when the order was read, or the plain reason in `order_library_note` when a condition fails (nothing is changed then, and the window offers a second, quieter button, **Scan with Playback instead**, which opens the usual confirmation and steps through Playback as before, source `scan`). A Library Link that cannot be used (no key, no answer) answers 409 with one plain sentence. While the call runs the button itself shows the loading animation over its own text, for at least 0.7 s; then the Song order section says `Read N songs from the Library Link`. Without a working Library Link the button is exactly what it always was. `POST /discover-song-order` still means "step through Playback". The header reminder **Scan Playback again** stays quiet while the order comes from the Library Link and its line has no reason; for a scanned or learned order it applies as before.

**What it never does.** It never writes to Planning Center, never changes the saved scan result (`playback_api` order and lengths) except in the two ways above (the automatic library order, and a press on the button), pairing, timers, ProPresenter, lights or Auto/Record, never sends Playback a command, holds no secret except the key it signs with (credential store only) and fetches a waveform only when a lighting timeline shows that song.

**Automatic updates (helper 0.4.0 and newer; the Lightkey helper 0.6.0 and newer).** StagePilot keeps a copy of each helper. When StagePilot starts, and again when a helper that was silent answers, it looks at each helper and, when the one that answers is older than its copy, **sends the new files to it by itself** (`POST /update`); there is no Update-now button. Nothing is sent unless every one of these holds: the switch `helper_auto_update` is not `false` in `settings.json` (there is no control for it; it is on unless that file says `false`, and the field is left out of the file while it is on); the helper says `"updater": 1` in `/health`; its version is older than the copy StagePilot carries; the copy's `SHA256SUMS.sig` is a valid Ed25519 signature of its `SHA256SUMS` by the project's key (StagePilot checks this itself and never sends an unsigned or wrongly signed bundle); the helper's address is on this local network (loopback, private or link-local, and for a name every address it resolves to; the request goes to the address that was checked); and it is quiet: no song is playing in Playback (the Playback API connected or not), the StagePilot song timer is not running and the Lightkey helper is not recording a take. When it is not quiet it looks again every 2 minutes for up to 30 minutes after the start (or after the helper came back), then waits for the next start or the next time the helper comes back. After it sent the files the helper has 60 seconds to say it runs the new version; an error reply, no answer in that time, or a `last_update_error` is a failed attempt, retried after 2 and then 10 minutes, at most 3 attempts per helper per bundled version per run of StagePilot, after which the setup command box is shown again with one plain sentence (`The Library Link could not update itself. Run the setup command on the Playback computer to update it.`). The block's status line says `Updating the Library Link to 0.4.0…` while it does and `Updated to 0.4.0 automatically.` once. A helper older than 0.4.0 (0.6.0 for Lightkey) cannot update itself: it is updated once by hand with the setup command, and then updates itself from then on. The request carries the HMAC (`POST\n<target>\n<time>\n<hex SHA-256 of the body>`, so the body cannot be swapped under a good signature); the key is not what authorizes the code, though: the helper installs only files whose `SHA256SUMS` carries a signature by the project's key (its public half is pinned in the helper and in StagePilot), every file must match the signed list, the names must be exactly the ones the helper takes, the version must be higher than the one that runs, and every `.py` must compile. The helper makes no outbound connection. The helper keeps the previous files (`*.prev`); a small launcher the installer writes (never replaced by an update) puts them back when the new version exits with an error within 20 seconds of starting three times in a row, and the helper then reports `last_update_error`. Signing happens on the release machine only (`node scripts/sign_helpers.mjs --key <pem>`, `docs/releasing-stagepilot.md`).

**Installing the helper.** See "Setting up the Playback computer" above. A helper installed by hand (version 0.2) keeps working, as `outdated`, and answers any caller on its network, so the bundled helper (0.3.0 and newer) with a key is the way to limit it.

## Local Network access on macOS

If Scan Network reports that macOS blocked StagePilot, open **System Settings >
Privacy & Security > Local Network** and turn on every StagePilot entry
(including any other StagePilot entry that is listed), then quit and reopen StagePilot. The
scan result's "Copy diagnostic details" and "Send logs to developer" include the
last scan (networks, probed counts, per-class errors, loopback/gateway self-test).
Backend log lines are `playback_scan_start`, `playback_scan_result`,
and `playback_connect_failed` in
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
