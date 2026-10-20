# Rust migration: real-rig verification A

Date: 2026-10-20 (America/Los_Angeles), owner-authorised for that day only.
Tested: `stagepilot-headless` 0.0.0 (app version string 2.41.1), macOS Intel build.
Tested SHA: `be52ce614fec0f1b1595bee6bdd62287fdf08223` (head of `feat/rust-phase6d`, PR #247, no newer head existed).
The binary actually pushed was built by the `rust` workflow from branch `docs/rust-rig-a`, which is that SHA plus two small commits (see "Changes on this branch"). Run: https://github.com/tage-ilot/stagepilot/actions/runs/39507467030 (artifact `rig-headless-macos-15-intel`). All Rust jobs in that run passed; its `Contract baseline` job failed at "Frozen OpenAPI and transcripts are current" (not investigated here; it is not part of this card, and the same job was green on the unchanged SHA).

Rig: jpl-graphics (iMac, x86_64, macOS 15.7.9) running the Rust server beside the live Python StagePilot; Playback on jpl-tracks (10.20.0.102:8080); ProPresenter on graphics (127.0.0.1:1025); Lightkey agent on jpl-lights.

## Setup (all scratch, all deleted)

- `~/sprig/` on graphics: the binary, a scratch `settings.json`, scratch data dir, scratch PIN (random, 8 digits, never printed), logs.
- Rust: `STAGEPILOT_SETTINGS_PATH=~/sprig/settings/settings.json STAGEPILOT_PLAN_CACHE_PATH=~/sprig/data/plan.json STAGEPILOT_KEYRING_SERVICE=sprig-rig-a-scratch stagepilot-headless --host 127.0.0.1 --port 18901`. Settings: demo service source, `midi_source=playback_api`, Playback 10.20.0.102:8080 (auto scan off, song order copied from the live app's read-only `/playback-api/status`), ProPresenter timer output on for check 3 only.
- Python comparison: the installed `/Applications/StagePilot.app/Contents/MacOS/stagepilot-backend` run a second time as a separate scratch process on 127.0.0.1:18902 with its own settings path (`STAGEPILOT_SETTINGS_PATH`, `STAGEPILOT_PLAN_CACHE_PATH`, `STAGEPILOT_PORT`). No keychain use (demo source). The live app (PIDs 87437/87444/87447, port 8765) was never stopped or changed; it was only read (login with the install PIN, GET `/api/v1/playback-api/status`, `/settings`, `/state`, `/propresenter`, `/midi/inputs`, `/ws`).
- Probe client: `harness.py`, a stdlib Python raw WebSocket client that connects to Playback as a third reader (`pr-protocol`), sends the commands, and records Playback's own heartbeats plus both servers' `/ws` snapshots, 2 s resource samples (`ps`), and a 0.5 s `/api/v1/state` poll of each server.
- Clock offset: UDP echo pair graphics<->tracks (60 pings, port 19321, 25 s server): RTT min 1.18 ms / median 1.49 ms, offset tracks-minus-graphics -24.47 ms, spread 0.25 ms. All latency/drift numbers below are taken from timestamps on graphics (the harness, Rust, scratch Python and the live app all run there), so the offset does not enter them; the one-way network delay (~0.6 ms) is below the resolution that matters. The offset is reported for completeness.

## Results

### 1. Playback timer accuracy (5 passes, 41 commands; plus one 150 s hold)

Latency from the command being sent to the first state snapshot in which the timer is running/stopped (WebSocket `/ws`), seconds. Playback's own heartbeat reaching the harness is shown for reference (heartbeats are 1 Hz, so it is cadence, not latency).

| Event | Python (scratch) | Rust | Playback heartbeat |
|---|---|---|---|
| play (pass 2..5) | 0.032 - 0.043 | 0.063 - 0.075 | 0.478 - 0.493 |
| pause (10 samples) | 0.034 - 0.095 | 0.066 - 0.084 | 0.959 - 0.991 |
| first play after song change (5) | 0.042 - 0.070 | 0.061 - 0.085 | 0.478 - 0.508 |

(Pass 1: neither server showed a running timer for the first play because the arrangement/plan song for the current Playback song is unmapped in the demo plan; later passes follow song changes. Not a protocol difference: both behaved the same.)

150 s hold (play from 0 on song 29552201, then pause): timer model (`started_at` + position) minus Playback `sequenceTime`, over 149 heartbeats:

| | min | max | first | last | pause latency | stopped position |
|---|---|---|---|---|---|---|
| Python | +0.006 s | +0.023 s | +0.015 s | +0.008 s | 0.051 s | 150.336 s |
| Rust | +0.022 s | +0.039 s | +0.031 s | +0.024 s | 0.082 s | 150.336 s |

No drift over 150 s in either; Rust sits a constant ~16 ms later than Python (and ~30 ms behind its own command), both within one 50 ms tick of Playback.

Earlier Python numbers (play/pause 0.10-0.21 s, first play after song change about 0.82 s) were NOT reproduced: both servers now react to Playback's command acknowledgement far faster (0.03-0.09 s). I did not find out why the old number was larger (different build, `fast_transport`, or the older measurement being heartbeat-based).

### 2. Stress shape

406 commands in 278 s (seeded random mix of song select 30%, play/pause 45%, seek 15%, 10% extra toggles), max 3 commands in any 1 s window, 0.25 s minimum gap. Both servers polled `/api/v1/state` every 0.5 s and held a `/ws` session.

| | Python (scratch) | Rust |
|---|---|---|
| poll failures | 0 / 559 | 0 / 559 |
| max poll request time | 0.143 s | 0.004 s |
| max gap between polls | 0.550 s | 0.511 s |
| WS reconnects / errors | 0 | 0 |
| WS snapshots received | 566 | 244 |
| RSS | worker 71.4 MB (launcher process 9.8 MB) | 12.1 -> 12.9 MB |
| CPU mean / max (`ps %cpu`) | 1.6 % / 17.1 % (CPU time 15.0 s) | 0.3 % / 33.4 % single sample (CPU time 0.85 s) |

Playback max heartbeat gap during the run: 1.17 s (1 Hz cadence, no stall). Playback never stalled. The Rust max CPU is one sampling point at a song change.

### 3. Read-only ProPresenter (GET only)

Rust with `timer_output=propresenter`, same host/port/timer name as the live app (settings `look_id` left unset so it could not trigger a Look). Rust issued `GET /v1/timers`, `/v1/looks`, `/v1/look/current` (the probe path) and nothing else; the timer-write path (PUT reset, `/stop`, `/trigger`) only runs on a cue, which this test never sent. `/api/v1/propresenter` compared with the live Python app:

- Same connection status (`connected`), same detail text, same timer (`Song Countdown`, id `4C06E95B-...`, index 0, not a countdown, state null), same 2 Looks (Worship index 0, Sermon index 1), same current Look id (`AD203048-...`).
- Only differences: `look_id` (live app has one saved; the scratch one has none by design) and JSON key order (Rust sorted, Python declaration order; JSON-equivalent).
- Before I enabled `timer_output=propresenter` Rust reported "plugin is disabled", as designed.

### 4. Read-only MIDI

`midi-probe` (Rust `midir`, listing only, nothing opened or sent): 2 inputs, 2 outputs: `Network Playback to GFX`, `Network GFX to Lights` (both directions). The live Python app's saved settings reference the same names (`midi.input_name` "Network Playback to GFX", lights output "Network GFX to Lights"). The Python backend's own `/api/v1/midi/inputs` returned an empty `inputs` list because MIDI input is disabled in the live settings (`midi.enabled=false`, `selected_input_name=null`), so there is no live Python listing to compare with beyond the saved names. The 4 names above match the Audio MIDI Setup network sessions.

### 5. Lightkey agent (read-only, from graphics)

- `GET http://10.30.0.211:8770/time`, `/log`: 200 (graphics' wired path). `GET http://10.40.0.45:8770/...` (the lights Mac's other-interface address): 403 `{"error": "not available on this interface"}` as expected.
- Clock offset against the agent over 40 `/time` reads at 4 Hz: RTT min 10.4 ms / median 13.0 ms, offset (agent minus graphics, midpoint method) -69.6 ms best-8 mean, spread 0.97 ms across the best 8, full range -70.2 .. -64.9 ms. So the offset is stable to about 1 ms, with an HTTP-sized (10-13 ms) RTT, much looser than the UDP pair (1.2 ms) but fine for a 1-second-scale correction.
- `GET /log` returned the 11-cue snapshot (reads: 1242, `read_ms` 803, no error).
- Not exercised: the Rust server's own Lightkey client (none exists in this build; `stagepilot-lights` has OSC/MIDI only). No OSC was sent.

### 6. Keychain

The Rust binary created, read back after a restart, and deleted its own item under service `sprig-rig-a-scratch` (account `planning-center-secret`, a dummy non-secret value) through `POST /api/v1/planning-center/settings`: `planning_center_secret_saved` went false -> true -> (fresh process) true -> false, and `security find-generic-password -s sprig-rig-a-scratch` showed the item while present and "could not be found" before/after. No existing StagePilot item was read (service name is overridden by the new `STAGEPILOT_KEYRING_SERVICE` variable; see below).

Caveat: over the plain SSH/`hermes-remote run` session the same save fails with HTTP 503 "The secure credential could not be saved." (`security add-generic-password` there also says "User interaction is not allowed": the SSH session is not the logged-in GUI session). It succeeded only when run through `hermes-remote gui` (the permissioned helper app in the GUI session). A headless Rust server started over SSH cannot use the login keychain; the live app does not hit this because it runs in the GUI session.

### 7. Resource comparison

| | Python sidecar | Rust headless |
|---|---|---|
| binary size | 28,394,560 B (frozen `stagepilot-backend`) | 17,340,456 B |
| start to healthy | not timed (scratch launch showed the listening line after ~10 s) | health/live answered within the 3 s wait after launch; `stagepilot_server_listening` logged in the same second as start |
| RSS idle | ~71 MB worker (+ ~10 MB launcher) | ~11-12 MB |
| RSS under stress | 71.4 MB | 12.9 MB |
| CPU (mean over 278 s stress) | 1.6 % | 0.3 % |

Start times were not measured with a proper stopwatch; treat those cells as indicative only.

## What I changed on this branch

1. `.github/workflows/rust.yml`: also trigger on `docs/rust-rig-a`, and build/upload `stagepilot-headless` + `midi-probe` as `rig-headless-<runner>` (3-day retention, not published) in the existing matrix job.
2. `crates/stagepilot-core/src/settings.rs`: `KeyringCredentialStore::entry` honours `STAGEPILOT_KEYRING_SERVICE` (default unchanged: `StagePilot`). Needed so a test run cannot touch the real StagePilot keychain entries. This is a behaviour addition to production code on a docs branch; keep or drop it when merging the verification.

## Restored state and proof

The harness fades out and mutes MIDI first and always runs the restore (pause, select original song, seek, MIDI mute off, fade in 0) even on failure. Pad and loops were never sent a command.

Final read-only `rig_latency ... listen 4` after everything: song 29552198 (same as before), `playing=false`, `pad=true` (same as before), heartbeat cadence 1.0 s. MIDI mute and fade state are not in the heartbeat, so they are proven only by the restore commands being sent last (`mixerMuteMIDI false`, `transportFade direction 0`, both logged). Loops were not touched.

One difference: the song position was 32.405 s before the first pass and 30.857 s after it, and stayed 30.857 s from then on. A `waveformSeek` to 32.405 lands on 30.857 on this Playback (it snaps), and I did not try to push it back. The live StagePilot timer, MIDI services, Playback, ProPresenter, Lightkey, OLA were not stopped, restarted or reconfigured.

## Surprises

- Playback accepted four simultaneous clients later (live Python, scratch Python, Rust, harness), but my very first scratch-Python launch sat in `not_playback / WSClosed` retries (134 attempts over ~3 minutes) while the live app, Rust and the harness were connected, until I killed it; a later launch connected immediately. I could not reproduce it or find the cause (possible per-client limit or stale socket). That attempt's data was discarded.
- The Python backend refuses a second launch silently? No: it started fine on another port, but it forks a launcher plus a worker process; the sampler counts both.
- The first stress-shape claim ("400 commands in ~270 s") matched: 406 in 278 s.

## Not verified

- The Rust server's real Planning Center, OSC/MIDI send, lights, and Remote paths (not in scope or read-only only).
- Real ProPresenter timer writes (would trigger the timer), Look triggering, MIDI input of the Rust server (listing only).
- Start-to-healthy timing, long (>150 s) timer drift, behaviour across Playback restarts.
- Whether the live app's MIDI/lights paths behave the same under the Rust server (only the names were compared).
