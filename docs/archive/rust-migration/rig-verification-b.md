# Rust migration: real-rig verification B (active outputs)

Date: 2026-10-20 evening to 2026-10-21 (America/Los_Angeles 21:55 to 22:50), owner-authorised active tests
(update of 2026-10-21 replaced the "today only" limit; traffic limits and the forbidden list still applied).
Binary: `stagepilot-headless` 0.0.0, macOS Intel CI artifact `rig-headless-macos-15-intel` from run
39547406118 (all Rust jobs green; the Linux and contract jobs were still queued when the tests ran).
Tested SHA: `af453579ecee281610b3a873c824b345ab1513fd` = PR #249 head `f10523f4` (Lightkey port) plus the rig
stack (#247/#250 base `be52ce61`, keyring-service override) plus one commit that builds the headless binary with
`stagepilot-headless/midir-backend` (see Surprises). sha256 of the binary: `aec67216242a4e5518907b88c08f19846d1c328ac681bc927c199ae8d28e876e`.
Python side: the installed `/Applications/StagePilot.app` sidecar (2.41.1), started as a second scratch process.

Rig: jpl-graphics (Rust and scratch Python side by side, ports 18901/18902, scratch settings/data dirs under `~/sprig`,
scratch keyring service), Playback on jpl-tracks (10.20.0.102:8080), ProPresenter on graphics (127.0.0.1:1025), Lightkey
agent + OLA on jpl-lights (10.30.0.211:8770, OSC 21600). The live app (PIDs 87437/87444/87447, port 8765) was never
stopped, restarted or reconfigured.

## Pre-test state (recorded before anything was sent; files kept on CT120)

| System | State recorded |
|---|---|
| Playback (heartbeat) | song 29712070, stopped, position 0, pad stopped, setlistState ready, cloud version 12. Fade, loops and MIDI mute are not in the heartbeat; they were never commanded except by the fade-out / mute-on at the start of each run, always followed by `mixerMuteMIDI false` + `transportFade 0`. |
| ProPresenter | timer `Song Countdown` (uuid 4C06E95B-...), allows_overrun, countdown duration 234, state overran, time -00:01:12. Active presentation null, slide index null, Look `Sermon` (CAB6703A-...). |
| Lightkey (agent `/log`, seq 59) | active (11): 25% Frost, J-outer, Middle stage, Stage top, Triangles, WIde Zoom, Warm White 70%, back stage light, middle front light, spots, zooms. Agent history seq 1..59 reconstructs this: seq 1 is the initial snapshot (same 11 cues incl. Triangles), seq 2..9, 35..59 are earlier Stars/Triangles toggles (net result Triangles on, Stars off) and seq 10..34 spots/zooms/Frost/etc. toggles that each returned to on. |

## Restore proof

- Lightkey: agent `/log?since=999999` state after every Lightkey run is the identical 11-cue set above (script compared sorted lists: `restored True` after each run, and once more at the end). Stars is off, Triangles on. Show file untouched.
- ProPresenter: the timer was left at countdown 234 / stopped / 00:03:54 (the original 234-second definition, reset+stop through the same PUT `/v1/timer/<uuid>/reset` + GET `/stop` the Python client uses). The recorded pre-test state was an overran timer at -00:01:12; ProPresenter cannot be put back into "overran" without running it for 234 s, so that one detail differs (duration, name, uuid, allows_overrun restored; state stopped instead of overran). Look `Sermon`, presentation null unchanged.
- Playback: final heartbeat song 29712070, stopped, position 0, pad **stopped** (same as recorded). The pad started playing after the first run's `transportPlay` (Playback's own behaviour, I never sent a pad command until the end), so it was stopped with one `transportPad false` at the end. Fade 0 and MIDI mute off were the last commands of every run.
- MIDI: every note-on was followed 100 ms later by its note-off (see traffic); the capture listener showed 20 on / 20 off pairs per 5-cue run and no unmatched note. All of it went to a scratch virtual destination `SPRIG Capture` created by a throwaway CoreMIDI listener on graphics, not to the `Network ...` sessions.
- Scratch dirs `~/sprig`, `/tmp` helpers on graphics and lights deleted; no stagepilot-headless or listener process left. Live processes untouched.

## 1. MIDI out (lights plugin, `midir` backend) vs Python

Cue set sent (same through both servers, sequentially): channel 1, pulse 100 ms, song "Battle Belongs" (demo), cues at 1/2/3/4/5 s with (note, velocity) = (60,100) (61,127) (72,1) (0,64) (127,127); Playback started via `transportPlay`, 7 s of play, then pause. Path: server -> CoreMIDI virtual destination -> capture block logging wall-clock ms.

| | Python | Rust |
|---|---|---|
| bytes on the wire (3 runs each) | `90 3c 64 / 80 3c 00 / 90 3d 7f / 80 3d 00 / 90 48 01 / 80 48 00 / 90 00 40 / 80 00 00 / 90 7f 7f / 80 7f 00` | identical, byte for byte, all 3 runs |
| first byte minus server `started_at` minus the 1.000 s cue time (2 e2e runs each) | +0.6, +1.5 ms | +2.0, +2.2 ms |
| cue-to-cue spacing (4 gaps x 3 runs) | 999.0 - 1001.6 ms | 997.7 - 1001.2 ms |
| note length (on to off, 15 samples each) | 100.2 - 102.7 ms | 100.6 - 103.1 ms |
| Playback `transportPlay` sent to first byte (3 runs) | 1030.7 - 1052.2 ms | 1028.4 - 1030.9 ms |

Rust sits about 1 ms later than Python on this measure (two runs each, so treat it as indicative); cue spacing jitter is the same (about +-2 ms) for both. The server adds roughly 1 to 2 ms on top of the cue time.

## 2. ProPresenter (real API on graphics)

Same call set from each server (`GET /propresenter`, `POST /propresenter/test`, `POST /propresenter/timers/refresh`, `POST /propresenter/settings` with `look_id` = the current Look, then timer driving through play, seek to 30 s, pause, resume, pause on Playback).

- Status/test/refresh responses: same `connected`, same detail text, same current Look id, same `timer_found`/`look_found` in Rust and Python.
- Settings with `look_id` = the current Look id returned in both servers the same `accepted:false, "ProPresenter settings were saved, but the Look was not applied: ProPresenter Look \"CAB6703A-...\" was not found."` (the ids from `/v1/look/current` do not appear in `/v1/looks`, whose ids are `AD203048-...` / the other Look). ProPresenter's Look did not change (still `Sermon`). So no Look trigger was exercised on either side.
- Timer behaviour (polling `/v1/timers/current` at 4 Hz): both servers show the identical sequence: running 04:40 about 190 ms after the play command, then 1 s ticks 04:39, 04:38, 04:37; after the seek to 30 s: stopped at 04:11 at +3.55 s (Rust) / +3.81 s (Python), running 04:10 at +4.33 / +4.32 s; pause at +7.17 / +7.18 s showing 04:08; resume shows 04:34 stopped at +10.29 s then running at +10.8 s; final pause 04:31 at +13.67 / +13.65 s. Differences between Rust and Python are within the 250 ms poll interval (the one ~250 ms difference, +3.55 vs +3.81 s, is a poll-phase effect).
- End-to-end play-to-timer: first `running` sample 154 and 157 ms (Rust), 150 and 140 ms (Python) after the Playback command (poll resolution 200 ms).

## 3. Lightkey (jpl-lights agent + OLA)

Recorded and replayed through the Rust link; Playback played "Battle Belongs" (the demo song, 281 s), Lightkey OSC edges sent from graphics to 10.30.0.211:21600 as the operator would (Stars/Triangles toggles, 14 edges, every 10 s, `/live/*/cue/<name>/activate`, no fade argument).

- Connect: `GET /lights/lightkey` -> `link: connected`, 11 active cues, replay on, not recording.
- Clock offset via the agent's `/time` (best of 7-9 samples): RTT 10.6 - 12.1 ms, offset spread between runs stays within a few ms for a run (e.g. -82.4 then -76.2 ms around one 150 s record, so about 6 ms of change over 150 s; the offset differed from run to run, -156 to -82 ms, and I did not find out why).
- Record: 1 song, 14 edges, 12 cues, saved in `lightkey-takes.json` ("Saved ... 14 changes. Song finished.").
- Replay: with recording off, started the song again from 0; edges came back as OSC: Stars on / Triangles off DMX seen at +3574 ms and +3575 ms after `started_at` (recorded edge time 3.347 s) and Triangles on at +13475 / +13697 ms (recorded 13.248 / 13.47). That is 227 ms after the recorded time. The replay sends 40 ms early (`REPLAY_LEAD_SECONDS`), so the delivered latency of these two cues is ~265 ms, not 41 ms.
- OSC-to-DMX latency, measured with graphics -> agent over the wire (n=14, Stars/Triangles pair): median 264.8, min 251.3, max 276.2 ms; measured locally on jpl-lights (n=12 pair, same cues): median 267.7, 255.2 - 278.0 ms. A single-cue test on cue `Triangles` (n=12): activate 33.0 - 47.0 ms (median ~41 ms, matches the PR's 41 ms / 28-52 ms), deactivate 502 - 527 ms (fade-out). `Warm White, 70%` (n=12) gave 45.4 ms for the first sample and then negative values (-497 to -845 ms); the agent logged about 20 DMX changes per event for this slow cue, and I did not work out why the stamps precede the send, so these numbers are discarded. So the 41 ms PR figure holds for "activate" of `Triangles`, not for Stars on / Triangles off pairs where the other cue's fade-out is what the agent sees first.
- Python plugin run "the same way" NOT done: the installed Python app (2.41.1) predates #248 and returns 404 for `/api/v1/lights/lightkey`. The Lightkey code is only on main/the rust stack, and I did not install a second Python build on the rig.
- Cue set after all Lightkey runs: identical to the recorded 11-cue set (see Restore proof).

## 4. End-to-end song flow

Playback `transportPlay` (fade out, MIDI mute on) -> server -> ProPresenter timer + MIDI cue + Lightkey replay.

| Output | Python | Rust |
|---|---|---|
| Playback command to server `started_at` | 42.7, 50.2 ms | 35.3, 37.1 ms |
| ProPresenter timer first `running` seen (4-5 Hz poll) after the command | 140 - 150 ms | 154 - 157 ms |
| MIDI cue at 1.000 s, byte time minus `started_at` | +1000.6 / +1001.5 ms | +1002.0 / +1002.2 ms |
| Lightkey replay DMX, edge at 3.347 s minus `started_at` | not available (no Lightkey in installed Python) | +3574 ms (recorded 3347, +227) |

The MIDI cue and timer ran in the same session (MIDI-only settings, ProPresenter timer output on). Lightkey replay was run in a separate session from MIDI because the lights plugin has one method at a time (OSC or MIDI), so "offsets between the three outputs" are each measured against the server's own `started_at`: ProPresenter about +0.15 s (poll-limited), MIDI +1.0 ms past its cue time, Lightkey +227 ms past its recorded time (cue fade, see above). Not all three at once.

## 5. Soak (15 min, shortened from 30 per the task comment)

Rust and the scratch Python server side by side, `waveformSeek` (5 s / 0 s alternating) every 10 s through Playback, `/api/v1/state` polled on both every 10 s, 90 commands, 90 polls each, 30 resource samples.

| | Python (scratch) | Rust |
|---|---|---|
| request failures | 0 / 90 | 0 / 90 |
| max `/state` request time | 43 ms | 6 ms |
| RSS first -> last (max) | 80.0 -> 83.4 MB (launcher + worker) | 11.7 -> 18.2 MB |
| CPU time first -> last | worker 1.39 s -> 6.93 s | 0.11 s -> 3.20 s (about 0.35 % average) |
| Playback reconnects | **never connected** (875 `not_playback / WSClosed` retries over the run) | none (0) |
| log errors/warnings | n/a | 1 WARN at start (settings schema migration), 0 errors |

Caveat: the Python scratch server again sat in `not_playback / WSClosed` retries for the whole soak (same effect as rig A's first launch). Its CPU/RSS are therefore not comparable with a connected server; earlier in the evening the same Python build connected fine when it was the only scratch server. Rust's RSS grew 6.5 MB in 15 min (11.7 -> 18.2 MB); the series is monotonic, so I cannot say it plateaus. That needs a longer soak.

## Exact traffic sent

- Playback (graphics -> tracks, 0.25 s minimum gap): `transportFade{direction:1}`, `mixerMuteMIDI{active:true}`, `waveformSeek{0.0}`, `transportPlay{true/false}`, `waveformSeek{5.0|30.0}`, then `mixerMuteMIDI{false}`, `transportFade{direction:0}`, `transportPad{false}` once at the end.
- MIDI: 20 note-on / 20 note-off pairs per capture session, notes 60, 61, 72, 0, 127 channel 1 to a virtual destination only.
- ProPresenter: GET `/v1/timers`, `/v1/looks`, `/v1/look/current`, `/v1/timers/current`; PUT `/v1/timer/<uuid>/reset` (countdown 281, 234) and GET `/v1/timer/<uuid>/start|stop|reset`; look trigger not performed.
- Lightkey: 14 edges per record run to UDP 21600 on the lights Mac (`/live/*/cue/Stars|Triangles/activate|deactivate`, plus 12 toggles of `Triangles` and 12 of `Warm White, 70%` for latency, each restored); plus read-only `GET /time` and `/log` on 8770.

## Surprises

- The first CI artifact for this branch was built without the `midir-backend` feature, so the Rust server reported "Lighting MIDI outputs could not be listed" and sent nothing. The rig A workflow only built `midi-probe` with MIDI. I added `--features stagepilot-headless/midir-backend` to the rig build step in `.github/workflows/rust.yml`; the second run is the one tested. This is a build-config trap for any future "headless with MIDI" release job.
- When the recorded Stars/Triangles take is replayed with the 40 ms lead, DMX lands ~227 ms after the recorded time for these cues (their transition is slow). The 40 ms constant matches the 41 ms PR measurement for single-cue activates only.
- The Playback pad started on its own after the first `transportPlay` (pad was stopped before). It was stopped at the end.
- The installed Python app does not contain the Lightkey feature, so no Python comparison for Lightkey.

## Not verified

- Python Lightkey record/replay (no build with the feature on the rig).
- All three outputs (ProPresenter timer, MIDI, Lightkey) in one session (the lights method is one-at-a-time).
- ProPresenter slide and presentation triggers and Look triggers (the Python and Rust clients only drive the timer and Look; the Look test could not apply because the current-Look id is not in the Looks list; I did not force a different Look onto graphics).
- Rust RSS plateau and a full 30 minute soak; connected Python soak numbers.
- Sending to the real `Network GFX to Lights` MIDI session (a scratch virtual destination was used instead).
- The Linux Rust job and the contract job of run 39547406118 were still queued when the tests were run.
