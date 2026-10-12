# Playback pause / resume / seek / reconnect audit

Factual snapshot of `main` for the Playback resume work (root t_704ecff8). Line numbers are
relative to `backend/src/stagepilot/` unless a path says otherwise, and will drift as cards B-F
land; search by symbol. No behaviour is changed by this card.

Owner decision (2026-10-12): stop and pause are the same state; the existing STOPPED path will
keep remaining time/position. MIDI has no pause message.

## 1. Pause / stop / resume / seek handling

Wire to events (`plugins/playback_api/normalizer.py`)
- `:15-21` event-type list includes `song.resumed`, `song.paused`, `song.stopped`, `seek`.
- `:95-117` `parse_heartbeat`: `setlistSongID`, `sequenceTime` (position), `sequencerPlayState`
  (playing/stopped), `setlistCloudVersion`. A heartbeat missing any of them is dropped.
- `:210-216` `waveformSeek` command -> `PlaybackEvent("seek", song, position)`; arms restart
  (`_armed = position < 2.0`). Seek is monitor-only downstream.
- `:255` command-driven classification: `song.started` if armed else `song.resumed`; `song.paused`
  when playing is turned off.
- `:174,198,407` `song.stopped` emitted by fast-transport provisional events and song change.
- `:325-332` heartbeat-time reconciliation (resumed/started both normalise to "play";
  stopped/paused to themselves; `returning` handling).
- `:438` heartbeat edge: `song.started` (armed) / `song.resumed`.
- `:450` heartbeat edge: `song.stopped` (stopped at position ~0) / `song.paused`.
- `:464` `song.stopped` at position 0.0 when a snapshot shows the song changed while playing.
- `ws.py:159-170` `_discovery_step`: `previous`, `next`, `seek_end` (waveformSeek 86400),
  `return_to_start`; `client.py:257-262` wrapper; `client.py:293-342` `measure_song_length`
  (only used by length measurement / discovery).

Mapping (`plugins/playback_api/mapping.py`)
- `:106-107` `song.paused` and `song.stopped` -> `ActionName.STOP_TIMER` (source `playback_api`),
  regardless of `stale` (current "pause even while stale" rule).
- `:108-113` `transport.reverted` with reason `song.started` and not playing -> `STOP_TIMER`.
- `:114-126` `song.started` only: needs not stale, a heartbeat, song in `song_order`; same current
  song index -> `RESTART_CURRENT`, else `dispatch_song_position`.
- `song.resumed` and `seek` fall through to `return None` (monitor only).
- `:70-100` `observe`: staleness (setlist id change, cloud version change, song not in order).
- `:61-64` `discard_pending` bumps `_generation` (queued observations dropped) without touching the
  order or `stale`.

Plugin (`plugins/playback_api/plugin.py`)
- `:475-537` `_observe`: freshness, `mapper.observe`, drops queued work when disconnected, publishes
  `ConnectionPayload`, forwards observations only while `arbiter.allows("playback_api")`.
- `:545-559` `_consume` dispatches each observation via `mapper.dispatch`.

Actions / events / state
- `core/events.py:47-50` `ActionName.RESTART_CURRENT`, `STOP_TIMER`; `:35-38` `TIMER_STOP_REQUESTED`,
  `TIMER_RESET_REQUESTED`, `TIMER_STARTED`, `TIMER_STOPPED`.
- `services/state_service.py:118-121` action table; `:190-198` `_restart_current` (needs a positive
  duration); `:230-235` `_stop_timer` publishes `TIMER_STOP_REQUESTED`;
  `:242-257` `_reset_position` (publishes `TIMER_RESET_REQUESTED`, clears song, timer STOPPED);
  `:307-345` service reload: running timer on date rollover publishes stop (`:317-320`), may blank
  timer (`:340`); `:376-388` `_handle_timer_started` -> `TimerStatus.RUNNING`, `duration_seconds`,
  `started_at`; `:390-397` `_handle_timer_stopped` -> `TimerStatus.STOPPED`, keeps
  `duration_seconds` only (started_at dropped: this is the "blank" the owner wants changed);
  `:399-406` failure -> `TimerStatus.ERROR`.
- `models/state.py:26` `TimerStatus` (IDLE, RUNNING, STOPPED, ERROR); `:147` `TimerState`.
- `services/planning_center_lengths.py:466` checks `RUNNING` before applying lengths.

ProPresenter (`plugins/propresenter/plugin.py`)
- `:93-97` subscriptions to `TIMER_STOP_REQUESTED` / `TIMER_RESET_REQUESTED`.
- `:207-238` `_on_song_event`: starts the countdown for the song duration (no start offset),
  publishes `TIMER_STARTED`.
- `:240-257` `_on_timer_stop_requested`: `_stop_timer_locked` (`:280-289`), then publishes
  `TIMER_STOPPED` with `duration_seconds=0`.
- `:259-278` `_on_timer_reset_requested`: `_reset_timer_to_zero_locked` (`:303,321`), publishes
  `TIMER_STOPPED`.

Lighting (`plugins/lights/plugin.py`)
- `:79-86` subscribes `TIMER_STARTED`, `TIMER_STOP_REQUESTED`, `TIMER_RESET_REQUESTED`,
  `TIMER_STOPPED`.
- `:201-205` `_on_song_event` remembers the pending song; `:207-226` `_on_timer_started` builds the
  cue list (`at_seconds <= duration`) and starts `_run_timeline(cues, started_at)`;
  `:228-229` `_on_timer_stopped` cancels the timeline; `:232-` `_run_timeline` only offsets by time
  since `started_at` (no position/resume concept).

MIDI (`plugins/midi_playback/plugin.py`, `core/midi.py`, `core/config.py`)
- `core/midi.py:22-32` `MidiCueName` (incl. `STOP_TIMER`, `RESTART_CURRENT`) maps by value to
  `ActionName`; `core/config.py:78,99` default note for `stop_timer` (105).
- `midi_playback/plugin.py:545-549,661-665` `action_guard` consulted before every action;
  `:126` field. MIDI has no pause/resume cue: stop = `STOP_TIMER`.

Tests that currently pin this: `tests/test_playback_api_transport.py` (normalizer lifecycle),
`test_playback_fast_transport.py`, `test_playback_arbitration.py`, `test_playback_mapping.py`.

## 2. Automatic MIDI fallback (to be removed by card C)

- `plugins/playback_api/arbiter.py:34` `midi_connected` flag; `:39` `observe_api`; `:53-95`
  `snapshot()` owner choice: API owns when heartbeats are < 5 s old and (already owner, or MIDI
  is not connected, or stable >= 3 s with heartbeat <= 1.5 s old); otherwise `midi` when
  `midi_connected`, else `none`.
- `arbiter.py:73-77` reason strings: `"Connected through MIDI (backup). Find Playback for full
  control."` (the fallback message), `"Connected to Playback."`, `"Not connected."`.
- `arbiter.py:84-92` per-source `SourceStatus`; `:96-97` `allows(source)`; `:109-183` the
  `ArbitratedDispatcher` dispatch gate: `:156` simulated MIDI blocked unless API owns,
  `:161-165` "ignored: Playback API active" / "ignored: MIDI active", `:167` duplicate suppression.
- `plugin.py:187` and `:338` `_wire_midi` (called when the MIDI controller is created in
  `_start_selected`, `:331-345`); `:244-249` `_wire_midi` sets `action_dispatcher`, `action_guard`,
  `ownership_revision`, `connection_observer`.
- `plugin.py:255-260` `_midi_guard`: rejects MIDI actions during discovery or while API is active.
- `plugin.py:262-265` `_midi_connection` -> `arbiter.midi_connected`, republish.
- `plugin.py:267-291` `connection` property overrides the reason: scanning -> "Looking for
  Playback…"; API -> "Connected to Playback on <name>."; none -> `NOT_FOUND_REASON` /
  scan-block / unreachable / "Not connected. Looking for Playback…" / "Not connected. Choose
  Scan Network to find Playback."; the `midi` owner keeps the arbiter's "(backup)" text.
- `plugin.py:331-345` `_start_selected` starts MIDI whenever `midi.enabled` and the source is not
  SIMULATED, even when Playback API is the selected source; `:353-370` `_stop_selected(keep_midi=)`.
- `plugin.py:293-315` `_publish_connection` (50 ms tick `:317-320`) publishes the combined state as
  a `ConnectionPayload(integration="midi")`; `:511-531` publishes the API-side connection too.
- Frontend: `frontend/src/components/playbackStatus.ts:37` `BannerTone` includes `"backup"`,
  `:42` `active_source === "midi" ? "backup"`; `PlaybackApiPanel.tsx:21` backup tone class;
  `PlaybackApiPanel.test.tsx:36,513` cases using the "Connected through MIDI (backup)" reason.
  The string comes from the backend; no frontend literal beyond the tone name.

## 3. Reconnect / probe loop

`plugins/playback_api/client.py`
- `:97-125` `PlaybackClient(reconnect_delays=(1, 2, 4, 8, 15), heartbeat_timeout=5.0)`.
- `:177-255` `_run`: loop { `find()` (`:163`, worker thread, `_find` `:83-86` -> `find_playback`,
  default probe timeout 3.0 s) -> `socket_factory(host, port, heartbeat_timeout)` -> read loop }.
  Heartbeat timeout: each `recv` waits at most the remaining 5 s since the last VALID heartbeat;
  expiry raises `WSClosed` (also covers a stalled-but-open socket). On any `OSError`/`ValueError`:
  log `playback_connect_failed`, optionally `saved_address_blocked` (`find.py:653`), publish a
  disconnected status with `last_error`, close socket.
- `:238-255` backoff: `delay = delays[min(attempt, 4)]`; `attempt` resets to 0 only after a VALID
  heartbeat on that connection; the wait is interruptible by `stop`. So after a long outage the
  retry interval is 15 s, not about 1 s.
- `:166-175` `_publish` / history (cap 100).

`plugins/playback_api/find.py`
- `:34-62` `probe(host, port=8080, timeout=3.0)`: connects, needs a valid heartbeat.
- `:224-250` `find_playback`: manual host -> probe only that host, never falls back.
  Otherwise ALWAYS probes `127.0.0.1` first, then, if `auto_scan`, scans every attached /24
  (32 worker threads, 3 s per-host timeout). There is NO "probe the last known good address
  first" step: the client does not remember the last discovered host between attempts, so with
  `auto_scan=True` each retry (1/2/4/8/15 s apart) can start a full LAN scan, and the scan itself
  can take longer than the delay.
- `:481-650` `scan_network` is the user-triggered scan with progress (separate from the loop).
- `:653-662` `saved_address_blocked`: only called after `permission_denied` / `no_route`.

`plugins/playback_api/plugin.py`
- `:234-242` `_options` -> `ConnectionOptions(enabled and selected, host, port, auto_scan,
  fast_transport)`; `host` from `settings.playback_api` is the saved address (manual override).
- `:579-615` `find()` = user "Scan Network"/typed host scan (`_scan` `:637-717`); a hit calls
  `_adopt` (`:616-627`), which saves `enabled=True`, `host=<found>`, midi_source=PLAYBACK_API in one
  save. This is the only place the saved address is written automatically.
- A saved host means `host_override` is set, so it is exclusive (no fallback, no scan).
- `:475-500` `_observe` on disconnect: `heartbeat=None`, `mapper.discard_pending()` (generation
  bump, queued actions dropped). It does not change the saved address, `song_order`,
  `captured_version` or `setlist_id`. Staleness after reconnect comes only from `mapper.observe`
  (setlist id or cloud version change, song missing from order); once invalidated, a matching
  heartbeat cannot silently repair it (`mapping.py:70-100`).
- `:395-440` `save_settings`: the song order can only be changed by Discover Song Order.
- On reconnect the normalizer is recreated per connection (`client.py:192`), so the first
  heartbeat is a `state.snapshot` event and no start/stop transition is inferred across a drop.

## 4. Fake-server harness (this card)

`backend/tests/playback_fake.py`: `FakePlayback` (scriptable loopback server), `ScriptedFinder`
(finder for `PlaybackClient`), `FakeClock`. Self-tests in `backend/tests/test_playback_fake_harness.py`.
Reuses `frame()` and `heartbeat()` from `tests/test_playback_api_transport.py`.
