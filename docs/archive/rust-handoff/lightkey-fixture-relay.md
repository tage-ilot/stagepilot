# Lightkey fixture capture: notes for the Rust migration

This feature (branch `feat/lightkey-fixture-relay`) is **Rust only**. It builds on the cue-edge
record/replay that PR #248 added (`lightkey-record-replay.md`) and was already ported. There is no
Python implementation and none is planned: Python stays as the fallback until phase 6, and it keeps
cue capture only.

## What it is

`lightkey-api` 0.4 (<https://github.com/huntrw6/lightkey-api>) runs on the Lightkey Mac. It reads the
project's fixture map, decodes Lightkey's sACN output into per-fixture state, converts every change
to Lightkey's own OSC (`/fixture/MH1/overrides/panAngle 90.0`) and streams it to subscribers over
UDP. The spec is `docs/relay.md` in that repository. StagePilot subscribes, records the stream
against the song while the countdown runs, and replays it with the same OSC output as the cues.

## Files

| File | Role |
|---|---|
| `stagepilot-lights/src/lightkey_fixtures.rs` | **pure logic, no I/O, no clock**: packet decoding, clock sample, sequence tracking and part joining, `FixtureState`, `FixtureRecorder`, `FixtureTrack`, `fixture_replay_plan` |
| `stagepilot-lights/src/lightkey_relay.rs` | async UDP client `RelayClient`: subscribe and renew, clock pings, keyframe repair, live state |
| `stagepilot-lights/src/lightkey_link.rs` | the existing link, extended: a `FixtureRecorder` per recording session, `run_fixture_replay`, `release_fixtures` |
| `stagepilot-lights/src/lightkey.rs` | `LightkeyTake.fixtures: Option<FixtureTrack>`, `FixtureStatus`, `LightkeyStatus.fixtures`, summary counts |
| `stagepilot-lights/src/plugin.rs` | provides `send_command` (OSC with arguments) to the link |
| `stagepilot-lights/tests/fixtures/relay_packets.json` | golden packets written by the Python encoder |

## Contract changes (all optional fields, so the Python contract still passes)

- `LightkeyStatus.fixtures?: FixtureStatus`: left out until the relay has answered once. An older agent
  produces none.
- `LightkeyTakeSummary.fixture_count?`, `fixture_event_count?`: left out for takes without a track.
- `lightkey-takes.json`: each take may carry `fixtures: {initial, events, lead_seconds}`. A take
  without it serialises byte-for-byte as before, so the existing file layout test still holds.
- No new route, no new setting. The relay port (21700) is not a setting: it is the agent's default.
  If a rig needs another port, add `relay_port` to `LightkeySettings` then.

## Rules to preserve (pinned by tests)

- The bundle **time tag is the DMX-change time on the Lightkey Mac's clock**. Convert with the offset
  from the lowest-round-trip `/relay/ping` sample (12 samples at connect, again every 120 s).
  `offset = ((t2 - t1) + (t3 - t4)) / 2`, `round_trip = (t4 - t1) - (t3 - t2)`.
- `/relay/seq` is numbered per subscriber and wraps at 2^31. The **subscribe reply (hello, seq 0)
  is outside the count; periodic hellos are inside it**. A gap drops any half-joined part set and
  asks for a keyframe. A new `stream` id in a hello means the agent restarted: forget everything.
- A command set can be split in parts (MTU 1400). Never apply a partial set.
- Only commands for a single named fixture are accepted (`/fixture/<name>/overrides/<prop>`). A name
  with an OSC pattern character (`* ? [ ] { } ,`) is refused at decode **and** when a take is
  validated, so replay can never hit more fixtures than were recorded.
- The recorder stores only commands that **change** something. The default scope starts only fixtures that
  change during the song (`finish(duration, all_fixtures=false)`); the rest stay with the cues and the
  operator. A song where nothing moved stores no track at all.
- Replay sends the state at the start position, in the order the agent sent it (dimmer before
  color), then each event `lead_seconds` (0.045) before its time. `lead_seconds` is stored in the take.
- **Release is mandatory.** Whenever replay ends, is cancelled (stop, seek, reset, a new song),
  or is switched off, send `/fixture/<name>/overrides/clear` for every fixture replay held, **twice**.
  Never use a `*` wildcard clear: on the real rig a wildcard clear left fixtures held until a
  second, specific clear was sent.
- Replay is blocked by the same reasons as cue replay (`replay_blocked_reason`), and
  `LightkeyLink::refresh_replay` (called from `lightkey_apply_settings`) cancels a running
  replay when the switch goes off.

## Measured on the real rig (wired path, iMac to Lights Mac)

- clock ping round trip 0.9 ms; OSC command to DMX change 37 to 51 ms; DMX change to packet on the
  receiver 1 to 2 ms; 30 of 30 changes received, 0 gaps.
- Over Wi-Fi the agent's multicast sACN stalled for up to 0.8 s. Unicast over the wired router is
  what this feature uses.

## Not done / next

- Fixtures were unpowered during agent testing, so DMX output is verified, not the lights themselves.
- Settings: a visible switch to record fixtures separately from cues was left out on purpose
  (one Record switch). Add it only if operators ask.
- The relay port is fixed. See above.
- The Rust relay in `lightkey-api` itself (port of `Relay`, `Fixtures`, `LevelTracker`) is a separate
  repository task. The wire format is the interface; `relay_packets.json` and the Python tests are
  the vectors.
