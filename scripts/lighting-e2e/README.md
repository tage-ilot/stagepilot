# Lighting end-to-end test (no rig)

A rig-in-a-box for the Lightkey live link: the **capture** path (relay stream in, smooth fit, save)
and the **replay** path (saved rows and effects out as OSC) run end to end against a real
`stagepilot-server` process, with every external piece faked on loopback.

```
scripts/lighting-e2e/run.sh              # about 50 minutes, every run is real time
scripts/lighting-e2e/run.sh --stages replay,control --out /tmp/e2e   # a part of it
```

Not run in CI: it needs the real take file (13 MB, never committed) and takes most of an hour.
The report (`REPORT.md`) lands in the output directory (default
`~/AgentHub-data/projects/stagepilot-lighting-e2e/run-<time>/`).

## Needs

Rust toolchain, `python3` 3.11+ with `websockets` (`pip install websockets`), a takes file in the
shape `{"takes": [{"song_title", "duration_seconds", "fixtures": {"initial": [{a, v}], "events":
[{"at_seconds", "osc": [{a, v}]}]}}]}` (set `TAKES=` or `--takes`), and these free ports: 21600
(OSC), 21700 (relay), 8770 (Lightkey status API), 18981 (fake Playback), 18900 (backend). The
script refuses to run if one is taken, and never uses 18766 or 8101 (the preview) or any real
settings file: the backend gets a temporary `STAGEPILOT_SETTINGS_PATH`, `STAGEPILOT_DATA_DIR`
and `HOME`.

## What is faked

| Fake | Port | What it does |
|---|---|---|
| relay (`lib/fakes.py` `FakeRelay`) | UDP 21700 | the lightkey-api relay (its docs/relay.md): `/relay/subscribe` answered by a hello (seq 0) and a keyframe, numbered hellos every 2 s and keyframes every 10 s, events as OSC bundles whose time tag is the DMX-change time on the relay's own clock (3.25 s ahead of ours), command sets split in parts under 1300 bytes, `/relay/ping` answered with `/relay/pong`, 15 s subscriber TTL. It replays a take's `initial` state and `events` at their `at_seconds` in real time. |
| Lightkey OSC sink | UDP 21600 | timestamps every datagram on arrival; echoes each override back through the relay 20 ms later (8 bit for levels), as the real DMX decode would |
| Lightkey status API | HTTP 8770 | `/time` and `/log` the way the agent answers; can be taken down to simulate a link drop |
| Playback (`FakePlayback`) | WS 18981 | a `pr-protocol` heartbeat ten times a second: play, pause, resume, seek, stop and song end for a song id. StagePilot learns the song order from it. |

The backend runs the demo service (four songs), the fake Playback as its MIDI source and the
simulated timer output. Everything is driven **over its HTTP API only**: Lighting settings
(`POST /api/v1/lights/settings`, `PUT /api/v1/lights/lightkey/settings`), Record
(`POST /api/v1/lights/lightkey/recording`), Release lights, status and the saved cue map
(`GET /api/v1/settings`). Song-time zero is read from the backend's own timer
(`GET /api/v1/state`), so the numbers are in StagePilot's clock.

The two real takes are fed to demo songs that are long enough: Unstoppable God (255 s) to "Battle
Belongs" (281 s), Able (290 s) to "Holy Forever" (336 s). The take is silent for the rest of the song.

## Stages (`--stages`, comma separated, run in this order)

1. `capture`: Record, play, feed the take, end the song; read the saved cue map (rows, effects,
   status, message). Compared with the offline S1 report (`replay_budget`, run by `run.sh`).
2. `replay`: Replay, play, record every datagram; rate, deviation from the take (outside/inside
   effects), step lateness, stale packets, process cost.
3. `control`: operator take-over (inject an unexpected change for one fixture, check it is dropped
   for 30 s and taken back), pause, resume, seek forward and back, Release lights, SIGTERM and
   SIGKILL mid-song (does it send clears).
4. `soak`: Able replayed three times with a capture between; RSS, threads, file descriptors, CPU.
5. `auto` (own backend, nothing captured): a song with no lighting change stays "not captured";
   a complete capture replays next time; a 30 s link drop still captures, a 70 s drop replays.

`lib/analysis.py` holds the numbers; `report.py` renders the tables.

## What it does not prove

Real Lightkey behaviour, a real network, DMX decode, the real Playback, audio loudness, the UI.
See section 6 of the report.
