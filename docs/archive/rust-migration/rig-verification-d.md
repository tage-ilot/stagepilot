# Rust migration: real-rig verification D (60 min soak, Lightkey Python compare, Look, recovery)

Status: tests 1-5 run on 2026-10-21 (America/Los_Angeles, inside the owner authorisation window).
Branch `docs/rust-rig-d`, based on the stack head `origin/feat/rust-test-gaps` plus the stack merges already on this branch.

## Short answers

| # | Question | Answer |
|---|---|---|
| 1 | Does Rust RSS plateau? | **Yes, it plateaus.** 10.2 MB -> 12.3 MB in the first 30 min, then 12.3 -> 12.6 MB over the next 30 min (0.64 MB/h by regression, 12.86 MB maximum). Rig B's 11.7 -> 18.2 MB over 15 min was not reproduced. No leak found, no fix commit needed (see 1.3). |
| 2 | Lightkey, Python vs Rust | Same behaviour. OSC-to-DMX latency: single-cue activate 31 ms (Rust) / 43 ms (Python); deactivate about 507 / 521 ms (cue fade); Stars/Triangles pair about 277 / 262 ms. Replay of a pair lands about +210 ms late in both, so yes, `REPLAY_LEAD_SECONDS` should depend on the cue's fade (see 2.3). Original 11-cue set restored and proven. |
| 3 | Look mismatch | **ProPresenter quirk, not a client bug.** `/v1/look/current` returns a per-instance uuid that is stable but never appears in `/v1/looks`; name and index match. Both clients match on index, so both resolve it. |
| 4 | Transport offsets | Playback `transportPlay` -> MIDI cue byte is the same for both (median 46.5 ms Rust / 47.9 ms Python beyond the 1.000 s cue). ProPresenter timer start differs by poll phase only (about 45 ms or 250 ms). Rust stamps `started_at` about 22 ms before its MIDI cue schedule, Python about 1.5 ms (see 4). |
| 5 | Recovery | Rust reconnected 0.986 s after the listener returned, republished state at once. Python took 2.975 s. Both recovered with no restart. |

## Build under test

- CI run https://github.com/tage-ilot/stagepilot/actions/runs/39632032708 (workflow `rust`, branch `docs/rust-rig-d`, SHA `ca87dbf7d6925ef425f40b42a17c8a7a5260f150`, which contains the stack head `feat/rust-test-gaps`). Artifact `rig-headless-macos-15-intel` (built with `stagepilot-headless/midir-backend`).
- `stagepilot-headless` sha256 `d9f2acc83d6b33117161fd5a32a30b8214b69ab4f7287cda08e360af1fc6ca46`; `midi-probe` sha256 `cc1101094d6548bf5adfc4f2aca6b55ce7b30a7602c683525006d8fbe5df6577`.
- On jpl-graphics `./stagepilot-headless --version` prints `stagepilot-headless 0.0.0` / `midi-backend: midir`.
- The macOS Intel, Apple Silicon, Windows and Linux Rust jobs of that run are green. Only the "Contract baseline - Python backend (Linux)" job failed (not this card's area; the integration card owns it).
- Python comparison: the Python backend from `main` (contains #248), installed in a scratch venv `~/sprig/venv` on graphics (`stagepilot` entry point), as a second scratch process.
- Scratch isolation for every process: dir `~/sprig/<name>`, own `STAGEPILOT_SETTINGS_PATH`, `STAGEPILOT_PLAN_CACHE_PATH`, `STAGEPILOT_KEYRING_SERVICE=sprig-rig-d-scratch`, loopback-only non-default ports (18901-18952), dashboard PIN off. The installed app, its settings and keychain were not touched.

## Pre-test state (recorded before any traffic)

Stored on CT120 in `~/.hermes/profiles/task-defined-agents/rig-d/` (`pretest-A-20261021-1612.json`, `midi-sessions-pre.txt`, `pretest-live-app-settings-summary.txt`) and, with the raw data, `rig-d-raw-data.tgz` and `posttest-20261021.json` in the same directory.

| System | State before |
|---|---|
| Playback (heartbeat) | song 29712070, stopped, position 0, pad **stopped**, setlistState ready, cloud version 12 (recorded 09:12 PDT, identical to the rig B start). As in rig B the pad started playing by itself after the first `transportPlay` (Playback's own behaviour; the soak's start heartbeat already showed it playing) and I stopped it once at the end. |
| ProPresenter | timer `Song Countdown` (4C06E95B-...), allows_overrun, countdown 234, stopped, 00:03:54; Looks `Worship` (E94606BB-..., index 0) and `Sermon` (AD203048-..., index 1); `/v1/look/current` = `Sermon` with uuid CAB6703A-..., index 1; no `/v1/version` route (404). |
| Lightkey agent | online, seq 127, 11 active cues: 25% Frost, J-outer, Middle stage, Stage top, Triangles, WIde Zoom, Warm White 70%, back stage light, middle front light, spots, zooms (Stars off). |
| MIDI sessions (midir) | 2 in / 2 out: `Network Playback to GFX`, `Network GFX to Lights`. None was opened by any test. |
| Live StagePilot app | modes planning_center / playback_api / propresenter timer; PP timer `Song Countdown`; MIDI lights to `Network GFX to Lights`; unchanged. |

## 1. Soak, 60 minutes

Both scratch servers connected to the real Playback (verified `connected: true`, `last_error: null` on every poll). Rust `stagepilot-headless` and the Python scratch server ran side by side for 59.8 min (120 samples every 30 s). A command was sent at most every 10 s, alternating `waveformSeek`, `setlistSelectSong` (alternate song and back) and `transportPlay` true/false, after `transportFade{direction:1}` and `mixerMuteMIDI{active:true}`; 369 commands, restored at the end (see Restore).

### 1.1 Curve (RSS every 5 minutes; CPU seconds are cumulative)

| min | Rust RSS MB | Python RSS MB (launcher + worker) | Rust CPU s | Python CPU s |
|---|---|---|---|---|
| 0 | 10.2 | 67.0 | 0.08 | 1.07 |
| 5 | 11.0 | 67.5 | 0.43 | 2.58 |
| 10 | 11.6 | 67.8 | 0.78 | 4.20 |
| 15 | 12.0 | 68.4 | 1.13 | 5.99 |
| 20 | 12.2 | 68.6 | 1.48 | 7.80 |
| 25 | 12.2 | 68.7 | 1.84 | 9.67 |
| 30 | 12.3 | 68.7 | 2.19 | 11.48 |
| 35 | 12.3 | 68.7 | 2.55 | 13.29 |
| 40 | 12.3 | 68.8 | 2.90 | 15.14 |
| 45 | 12.3 | 68.8 | 3.26 | 16.95 |
| 50 | 12.4 | 68.8 | 3.61 | 18.80 |
| 55 | 12.5 | 68.8 | 3.97 | 20.61 |

| | Rust | Python |
|---|---|---|
| first / last / **max** RSS | 10.2 / 12.6 / **12.6 MB** (max sample 12 860 kB) | 67.0 / 68.8 / 68.8 MB |
| growth after minute 30 (60 samples) | 12.27 -> 12.55 MB, 0.64 MB/h | 68.72 -> 68.80 MB, 0.17 MB/h |
| growth by window (min/max kB) | 0-10 min 10 448-11 884; 10-20 11 900-12 480; 20-30 12 480-12 560; 30-45 12 552-12 632; 45-60 12 624-12 860 | |
| CPU average | 0.12 % | 0.59 % |
| `/state` polls (361 each, 1 per 10 s) | 0 failures, median 1.0 ms, max 7.9 ms | 0 failures, median 3.4 ms, max 38.9 ms |
| Playback reconnects / `ws` reconnects | 0 / 0 | 0 / 0 |
| errors | 0 | 0 |
| `/ws` frames received by the soak client | 118 | 243 |

Rust is monotonic only in the first 30 minutes (28 of 119 consecutive samples decreased overall); after that the curve is a few hundred kB, not a ramp.

### 1.2 Verdict

Memory **plateaus**. Rig B's 6.5 MB rise in 15 min is the same warm-up as here (the first 15 min rose 1.8 MB from a 10.2 MB start; rig B started at 11.7 MB and ended at 18.2 MB, a figure I could not reproduce in this run). The remaining slope is 0.64 MB/h, below what a sampling run of this length can separate from allocator behaviour.

### 1.3 Cause analysis (no fix needed)

I looked for the suspects named in the card and found bounded structures in each:

- Playback event history: `HISTORY_CAPACITY = 100`, log `LOG_CAPACITY = 200`, trimmed with `while ... len() > CAPACITY` (`crates/stagepilot-playback/src/plugin.rs:60-61, 93, 1266`).
- Event bus: `broadcast::channel(OBSERVER_CAPACITY = 256)` (`crates/stagepilot-core/src/bus.rs:55, 69`), lagging receivers drop old items rather than queue.
- `/state` `recent_events`: trimmed with `keep_last(..., limit)` (`crates/stagepilot-core/src/state_service.rs:927-933`).

No unbounded queue was found on the paths exercised here. Lightkey/OSC link state was not part of the soak (Lightkey was off in the soak config) and a 60 min Lightkey soak was not run (see "What I could not verify"). No fix commit was added.

## 2. Lightkey, Python vs Rust

Both servers (Rust headless; Python from `main`) ran as scratch processes (`demo` service, simulated MIDI and timer) talking to the Lightkey agent at 10.30.0.211:8770 and OSC 21600; the agent accepted the graphics address. For each server and each schedule I recorded a take against a simulated 160 s countdown (the harness sends the operator's OSC edges every 10 s, like the operator), then replayed it (`lights.lightkey.replay: true`) and read the agent's `/log` events (effective DMX time) with clock offset measured by `/time` before and after (RTT about 10 ms, offsets between -97 and -145 ms drifted during a run, so offsets are linearly interpolated).

Schedules: `pairs` = Stars activate + Triangles deactivate / Triangles activate + Stars deactivate alternating (32 sends per record run); `single` = Triangles deactivate / activate alternating (16 sends).

### 2.1 OSC send to DMX latency (record runs)

| Schedule | Python | Rust |
|---|---|---|
| pairs (n=16) | median 262.4 ms, range 248.2..274.8 | median 276.5 ms, range 255.7..301.2 |
| single, all (n=16) | median 283.1, range 30.6..532.1 | median 272.5, range 28.6..513.5 |
| single, activate (n=8) | median 42.5, range 30.6..60.0 | median 30.9, range 28.6..40.4 |
| single, deactivate (n=8) | median 521.3, range 506.1..532.1 | median 506.7, range 504.6..513.5 |

(The latency is the agent's observed DMX change, so the cue's own fade is part of it; deactivating Triangles fades for about 0.5 s.) The difference between Rust and Python is the same size as the run-to-run spread, so it is the same path; the OSC bytes are identical to the golden vectors.

### 2.2 Replay accuracy and start/seek (replay minus recorded DMX song time)

| Schedule | Python | Rust |
|---|---|---|
| pairs, 15 edges | median +214.8 ms, range +210.7..+217.7 | median +208.4, range +162.7..+230.0 |
| single, activate (n=7) | median -34.7, range -45.6..+2.5 | median -8.5, range -9.6..+15.5 |
| single, deactivate (n=8) | median -22.2, range -39.2..+459.0 | median +8.9, range -10.6..+466.5 |

- Added/removed sets matched on all 15/15 edges in all four replays; the replay produced 15 events against 16 recorded: the 16th record event (a `Triangles` activate at 160 s, after the countdown was stopped) is not part of the take.
- The first replayed edge is about 0.46 s later than recorded in the single schedule for both servers (first replay event 10.98 s against 10.51 s recorded); all later edges are on time. This is the +459/+466 outlier above and appears in Python and Rust alike.
- `cmd_to_started_ms` (start command to timer `started_at`): Rust 0.5-1.4 ms, Python 5.2-6.5 ms.

### 2.3 Should `REPLAY_LEAD_SECONDS` (40 ms) depend on cue fade?

Evidence says yes, or at least that a fixed 40 ms is wrong for cues with a visible fade. The recorded edge is the time the agent *observed* the DMX change, and the replay sends the command at that time minus 40 ms; the command then needs its own latency before DMX moves. For the activate-only single cue (about 31-43 ms to DMX) the replay is early by 9-35 ms, which a 40 ms lead explains. For the pairs schedule (about 260-280 ms to DMX because of the fade) the replay is consistently +210 ms late, in both implementations. A lead of "recorded observed latency" (or a per-cue fade) would fix this; with a fixed lead, cues with long fades are late by roughly their fade time minus 40 ms. This is the same in Rust and Python, so it is a design question, not a migration bug.

### 2.4 Restore

After each run the harness reset the cue set (`Triangles` on, `Stars` off) and compared it with the recorded set; every comparison passed. After the last test I read the agent `/log` again: 11 active cues, equal to the pre-test list (25% Frost, J-outer, Middle stage, Stage top, Triangles, WIde Zoom, Warm White 70%, back stage light, middle front light, spots, zooms; Stars off).

## 3. ProPresenter Look mismatch (GET only)

GET `/v1/looks`: `Worship` (E94606BB-..., index 0) and `Sermon` (AD203048-..., index 1). GET `/v1/look/current` returned, on 12 reads over 60 s (5 s apart), always the same id: uuid CAB6703A-... name `Sermon`, index 1. Further GETs:

| Request | Result |
|---|---|
| `/v1/look/CAB6703A-...` (the "current" uuid) | **404** |
| `/v1/look/AD203048-...` (list uuid) | 200 `Sermon` |
| `/v1/look/Sermon` and `/v1/look/1` | 200, same Look as the list entry |
| `/v1/look/E94606BB-...`, `/v1/look/0` | 200 `Worship` |

So ProPresenter reports a uuid for the *current look instance* that does not exist as a saved Look; name and index are the stable keys. That is a ProPresenter quirk. Neither client trusts the uuid:

- Rust: `snapshot` maps current to a saved Look by `index` (`crates/stagepilot-propresenter/src/plugin.rs:686-692`), and `apply_look` verifies a trigger by index or case-folded name (`plugin.rs:177-182`).
- Python: `_current_saved_look_id` matches by `id.index` (`backend/src/stagepilot/plugins/propresenter/plugin.py:503-510`) and verification uses index too (`:196`).

On this rig both servers report `current_look_id` as the saved `Sermon` uuid. No Look was applied.

## 4. Transport correctness under realistic play

Path: Playback `transportPlay` -> StagePilot (`start_next`) -> ProPresenter timer start and MIDI cue (notes 60/61/72 at 1/2/3 s) to a scratch CoreMIDI virtual destination `SPRIG Capture` (a throwaway listener on graphics; the live `Network ...` sessions were never opened). Each server ran with `midi_source: playback_api`, `timer_output: propresenter`, saved song order and `captured_version: 12`, and `transportFade 1` + `mixerMuteMIDI true` before play. Only one server was active against ProPresenter at a time per run; runs alternated Rust/Python. Each run played 4-7 s then paused and restored.

Rig note: the first attempts did not produce a countdown because the server only maps a Playback song to a plan song when its saved order includes the heartbeat song and the version equals `captured_version`; the config I used in the first attempts lacked `captured_version`. That is the expected "monitor-only" behaviour, not a bug.

| Measure (ms, run-to-run spread) | Python | Rust |
|---|---|---|
| `transportPlay` sent -> server `timer.started_at` (every run, n=9 each) | median 46.3 (38.9..57.8) | median 24.7 (18.9..37.9) |
| `transportPlay` sent -> first MIDI note-on, minus the 1.000 s cue (n=6 Rust, n=3 Python) | median 47.9, range 40.2..48.8 | median 46.5, range 40.6..65.3 |
| MIDI note-on minus (server `started_at` + 1.000 s) | median **+1.5**, range +1.3..+2.0 | median **+22.0**, range +17.5..+27.4 |
| ProPresenter timer first `running` seen (200 ms poll) | 40, 247, 250, 252, 253, 255 | 42, 50, 244, 246, 254, 256 |
| Playback -> ProPresenter, per run | poll-limited (0.2 s grid) | poll-limited (0.2 s grid) |
| MIDI wire bytes (`90 3c 64`, `90 3d 7f`, `90 48 01` with note-offs 100 ms later) | identical | identical |

Reading: from the operator's side (Playback play -> MIDI byte) Rust and Python are indistinguishable (about 47 ms past the cue time). The difference is *where* the time shows up: Rust stamps `started_at` about 22 ms earlier than Python (it uses the Playback event time, Python the receipt time), so against its own `started_at` its MIDI cue is 22 ms late where Python's is 1.5 ms. Rig B measured +2 ms for Rust with a `transportPlay` that also started the demo timer without song mapping, so the 22 ms is specific to the `playback_api`-driven start. Lightkey replay in the same flow was not run (the lights plugin has one method at a time; a second 4-minute song play would need the Lightkey replay config, which I covered in section 2 against a simulated countdown instead). Several MIDI runs have no usable MIDI capture (the listener was not ready or had exited; those are excluded, not estimated).

## 5. Restart and recovery

Method: each scratch server's Playback address was a scratch TCP forwarder on loopback (scratch port -> real Playback 10.20.0.102:8080). The forwarder was killed (listener closed, established connections reset) for 5 s and then started again; a monitor polled `/playback-api/status` every 50 ms and a client listened on the server's `/ws`. No live service was touched or restarted. For Rust I ran two variants (a first one with a 6 s hold and a repointing of settings, then the forwarder variant reported in the table). In the Rust summary of the forwarder run the final "restored" poll read `connected: false` because it was taken in the instant the settings were switched back; the status monitor and later steps (4, 5) show it connected again, and both servers were stopped afterwards.

| | Rust | Python |
|---|---|---|
| connection drop visible in `/playback-api/status` | 9 ms (variant 2), 62 ms (variant 1) | 58 ms |
| listener back -> first accept | 2 ms | **0.999 s** |
| listener back -> `connected: true` in status | **0.986 s** | **2.975 s** |
| first Playback frame on `/ws` after the listener returned | 0.732 s | 2.934 s |
| frames on `/ws` during the outage / total | 1 / 4 | 1 / 3 |
| status errors | 0 | 0 |
| `last_error` during the outage | `Playback unavailable; remote connections may be off` | same |

Rust reconnected on the 1 s cadence (0.986 s) and republished the state; Python reconnected on its own 1 s retry plus a probe and took about 3 s. After the test both scratch servers were back on the real Playback. No restart was needed.

### 5.1 Why Python stayed in `not_playback` in rig B (and the same here)

Observed repeatedly: a scratch server started while a previous scratch process's connection had just been killed (FIN_WAIT_2 sockets from graphics to Playback at 10.20.0.102:8080 were still visible in `netstat`) sat in `playback_connect_failed ... not_playback / WSClosed` retries (66 attempts, one per second) although a plain probe from the same machine (`pbws.py`/`probe1.py`) connected and received heartbeats. Restarting the process made it connect at once. In this run the **Rust** server, started at the same moment, also stayed `connected: false` for over 4 minutes with no log line, then connected immediately after a restart. So the stuck state is not Python-specific; it is a start-up condition (connection attempted while Playback still holds the half-closed old sockets, then the retry path does not recover for minutes). I did not root-cause it on the Playback side. Setup fix applied for every kept number: before keeping any number I proved `connected: true` on both servers (the soak and recovery logs show it on every poll), and I restarted a scratch server that was stuck. I recommend the product retry path be looked at separately (Rust stuck 4 min is the more worrying one); it did not occur once a server was connected.

## Restore proof

| Item | Before | After |
|---|---|---|
| Playback song / state / position | 29712070, stopped, 0 | 29712070, stopped, 0 |
| Playback pad | stopped (09:12 PDT), playing from 10:30 PDT on (Playback started it after `transportPlay`; found playing when this run resumed at 12:25 PDT) | stopped with one `transportPad{playing:false}` at 12:51 PDT; heartbeat confirmed `padPlayerPlayState: stopped` |
| Fade / MIDI mute | not in heartbeat | last commands of each run: `waveformSeek` to the original position, `mixerMuteMIDI false`, `transportFade 0` |
| ProPresenter timer | uuid 4C06E95B-..., countdown 234, allows_overrun, stopped, 00:03:54 | same (reset with PUT `/v1/timer/<uuid>/reset` as the Python client does); `/v1/timers` shows `countdown.duration 234`, `/v1/timers/current` `stopped 00:03:54` |
| Look | `Sermon` (CAB6703A...) | `Sermon` (CAB6703A...) |
| Lightkey active set | 11 cues listed above | same 11 cues, Stars off |
| MIDI sessions | `Network Playback to GFX`, `Network GFX to Lights` | unchanged; only scratch `SPRIG Capture` virtual destination used |
| Live StagePilot app | running | still running, untouched (pid 87447/87437) |
| Scratch dirs and binaries on Macs | n/a | `~/sprig` removed on graphics; nothing on tracks; nothing on lights |

## Exact traffic sent

- Playback (graphics -> 10.20.0.102:8080, at least 0.25 s apart, never more than 5 per second): `transportFade{direction:1}`, `mixerMuteMIDI{active:true}`, `waveformSeek{sequenceTime}`, `setlistSelectSong{setlistSongID}` (original and one alternate), `transportPlay{playing:true|false}`, `transportNextSong` once (soak song change, restored), restore `waveformSeek`, `mixerMuteMIDI{false}`, `transportFade{0}`, and `transportPad{playing:false}` once at the end. 369 commands in the soak, a few hundred in tests 2, 4, 5. No loops, no pad on-commands, no `transportNextSong` beyond the one.
- ProPresenter (127.0.0.1:1025 on graphics): GET `/v1/timers`, `/v1/timers/current`, `/v1/looks`, `/v1/look/current`, `/v1/look/<uuid|name|index>`, `/v1/version`; PUT `/v1/timer/<uuid>/reset` (restore 234 s, and the servers' own countdown set); GET `/v1/timer/<uuid>/start|stop` through the servers. No Look was triggered. No slide or presentation commands.
- Lightkey (graphics -> 10.30.0.211:21600 OSC): `/live/*/cue/Stars|Triangles/activate|deactivate`, 16 or 32 per record run, 8 record runs, plus the harness's restore messages; GET `/log`, `/time` on 8770.
- MIDI: scratch virtual destination only: notes 60, 61, 72 on channel 1, 100 ms pulse.

## What I could not verify

- A 60 min soak with the Lightkey/OSC link enabled (the soak used simulated lights); so the OSC link state as a growth source is untested.
- Playback -> ProPresenter -> MIDI -> Lightkey replay in one single session (the lights plugin runs one method at a time); Lightkey replay and MIDI were measured in separate sessions.
- Real audio/stage output; the song was played faded out with MIDI muted.
- The root cause of the "stuck at start" connect state in section 5.1 (Playback-side half-open sockets are the likely trigger).
- The Rust scratch server sampled `rss_kb` through `ps` (RSS, not heap profile).
