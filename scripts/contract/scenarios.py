"""The contract scenarios: an ordered list of steps run against a fresh demo-mode backend.

Each step is a dict:
  {"name": ..., "method": "GET", "path": "/api/v1/health", "json": {...}?}   one HTTP exchange
  {"name": ..., "ws": "/ws", "receive": 1, "after_receive": [<http step>...], "then_receive": 1}
      a WebSocket session: collect `receive` messages, run the HTTP steps, collect `then_receive` more.

Only read-mostly and demo-safe endpoints are exercised: no Planning Center, MIDI, OSC,
ProPresenter or Playback traffic, no remote access, nothing that touches the OS keychain.
"""

from __future__ import annotations

from typing import Any

Step = dict[str, Any]


def http(name: str, method: str, path: str, body: Any = None) -> Step:
    step: Step = {"name": name, "method": method, "path": path}
    if body is not None:
        step["json"] = body
    return step


SCENARIOS: list[Step] = [
    http("health", "GET", "/api/v1/health"),
    http("health live", "GET", "/api/v1/health/live"),
    http("access (LAN, no PIN enforced)", "GET", "/api/v1/access"),
    http("state, demo plan loaded", "GET", "/api/v1/state"),
    http("settings", "GET", "/api/v1/settings"),
    http("planning center status", "GET", "/api/v1/planning-center/status"),
    http("planning center service types (not configured)", "GET", "/api/v1/planning-center/service-types"),
    http("midi inputs", "GET", "/api/v1/midi/inputs"),
    http("midi messages", "GET", "/api/v1/midi/messages"),
    http("propresenter status", "GET", "/api/v1/propresenter"),
    http("lights status", "GET", "/api/v1/lights"),
    http("playback status", "GET", "/api/v1/playback-api/status"),
    http("playback connection", "GET", "/api/v1/playback-api/connection"),
    http("playback events", "GET", "/api/v1/playback-api/events"),
    http("dashboard auth status", "GET", "/api/v1/dashboard-auth/status"),
    http("remote access status", "GET", "/api/v1/remote-access"),
    {
        "name": "ws: initial snapshot, then a snapshot after start_next",
        "ws": "/ws",
        "receive": 1,
        "after_receive": [http("start_next", "POST", "/api/v1/actions/start_next")],
        "then_receive": 1,
    },
    http("state while running", "GET", "/api/v1/state"),
    http("next song", "POST", "/api/v1/actions/next"),
    http("previous song", "POST", "/api/v1/actions/previous"),
    http("stop timer", "POST", "/api/v1/actions/stop_timer"),
    http("reset position", "POST", "/api/v1/actions/reset_position"),
    http("restart current", "POST", "/api/v1/actions/restart_current"),
    http("stop timer again", "POST", "/api/v1/actions/stop_timer"),
    http("unknown action is a 422", "POST", "/api/v1/actions/bogus"),
    http("unknown route is a 404", "GET", "/api/v1/does-not-exist"),
    http("wrong method is a 405", "DELETE", "/api/v1/state"),
    http("settings PUT with an invalid body is a 422", "PUT", "/api/v1/settings", {"timezone": 5}),
    http("playback find, invalid body", "POST", "/api/v1/playback-api/find", {"bogus": True}),
    http("midi input selection, unknown input", "POST", "/api/v1/midi/input-selection", {"input_name": "No Such Port"}),

    # ---- phase 2: dashboard PIN, access and settings flows -----------------------------------
    # Appended after the phase 1 steps so those keep their recorded positions. The backend runs
    # with in-memory settings and PIN enforcement off, so these steps exercise the PIN endpoints
    # and cookie sessions without locking the runner out.
    http("PIN login, wrong default PIN is a 401", "POST", "/api/v1/dashboard-auth/login", {"pin": "9999"}),
    http("PIN login, PIN too short is a 422", "POST", "/api/v1/dashboard-auth/login", {"pin": "12"}),
    http("PIN login, default PIN", "POST", "/api/v1/dashboard-auth/login", {"pin": "1234"}),
    http("dashboard auth status after login", "GET", "/api/v1/dashboard-auth/status"),
    http("access after login", "GET", "/api/v1/access"),
    http("PIN set", "POST", "/api/v1/dashboard-auth/settings", {"enabled": True, "pin": "8642"}),
    http("dashboard auth status after PIN change (sessions cleared)", "GET", "/api/v1/dashboard-auth/status"),
    http("PIN login, old PIN is a 401", "POST", "/api/v1/dashboard-auth/login", {"pin": "1234"}),
    http("PIN login, new PIN", "POST", "/api/v1/dashboard-auth/login", {"pin": "8642"}),
    http("PIN set, too short is a 422", "POST", "/api/v1/dashboard-auth/settings", {"enabled": True, "pin": "12"}),
    http("PIN set, missing enabled is a 422", "POST", "/api/v1/dashboard-auth/settings", {"pin": "8642"}),
    http("PIN protection disabled", "POST", "/api/v1/dashboard-auth/settings", {"enabled": False}),
    http("dashboard auth status, protection off", "GET", "/api/v1/dashboard-auth/status"),
    http("PIN login while protection is off", "POST", "/api/v1/dashboard-auth/login", {"pin": "anything"}),
    http("PIN protection re-enabled, PIN kept", "POST", "/api/v1/dashboard-auth/settings", {"enabled": True}),
    http("PIN login, kept PIN", "POST", "/api/v1/dashboard-auth/login", {"pin": "8642"}),
    http("settings after PIN changes (hash never exposed)", "GET", "/api/v1/settings"),
    http("settings PUT, unknown key is a 422", "PUT", "/api/v1/settings", {"bogus_key": 1}),
    http("settings PUT, invalid timezone is a 422", "PUT", "/api/v1/settings", {"timezone": "Not/AZone"}),
    http("settings PUT, out of range port is a 422", "PUT", "/api/v1/settings", {"server_port": 70000}),
    http("settings PUT, legacy release_channel is dropped", "PUT", "/api/v1/settings", {"release_channel": "beta"}),
    http("settings PUT, schema_version 1 body is migrated", "PUT", "/api/v1/settings", {"schema_version": 1, "integration_modes": {"service_source": "demo", "midi_source": "real", "timer_output": "simulated"}, "midi": {"enabled": False}}),
    http("settings round trip, change timezone and log level", "PUT", "/api/v1/settings", {"timezone": "America/New_York", "log_level": "WARNING", "web_dashboard_pin_enabled": True}),
    http("settings read back", "GET", "/api/v1/settings"),
    http("settings PUT, nested unknown key in planning_center is a 422", "PUT", "/api/v1/settings", {"planning_center": {"bogus": 1}}),
    http("settings PUT, non-object body is a 422", "PUT", "/api/v1/settings", [1, 2]),

    # ---- phase 3b-B: Playback routes -----------------------------------------------------------
    # Recorded from Python. Steps with `fake` drive scripts/contract/playback_fake.py (a scripted
    # loopback Playback on FAKE_PORT); steps with `wait` poll a GET until the connection settles.
    # No Planning Center, so the length routes answer their "not connected" errors.
    http("playback settings, out of range port is a 422", "PUT", "/api/v1/playback-api/settings", {"port": 0}),
    http("playback settings, port too high is a 422", "PUT", "/api/v1/playback-api/settings", {"port": 70000}),
    http("playback settings, bad host is a 422", "PUT", "/api/v1/playback-api/settings", {"host": "bad host!"}),
    http("playback settings, unknown key is a 422", "PUT", "/api/v1/playback-api/settings", {"bogus": 1}),
    http("playback settings, bad boolean is a 422", "PUT", "/api/v1/playback-api/settings", {"enabled": "maybe"}),
    http("playback settings, non-object body is a 422", "PUT", "/api/v1/playback-api/settings", [1]),
    http("playback find cancel while idle", "POST", "/api/v1/playback-api/find/cancel"),
    http("playback find, host too long is a 422", "POST", "/api/v1/playback-api/find", {"host": "x" * 300}),
    http("playback find, host with a space is a 422", "POST", "/api/v1/playback-api/find", {"host": "bad host"}),
    http("playback arrangement before a scan is a 409", "PUT", "/api/v1/playback-api/arrangement", {}),
    http("playback arrangement, duplicate skips is a 422", "PUT", "/api/v1/playback-api/arrangement", {"skipped_song_ids": [1, 1]}),
    http("playback arrangement, unknown song is a 422", "PUT", "/api/v1/playback-api/arrangement", {"skipped_song_ids": [5]}),
    http("playback arrangement, unknown key is a 422", "PUT", "/api/v1/playback-api/arrangement", {"x": 1}),
    http("playback arrangement, wrong item type is a 422", "PUT", "/api/v1/playback-api/arrangement", {"skipped_song_ids": ["a"]}),
    http("playback discover without confirm is a 400", "POST", "/api/v1/playback-api/discover-song-order", {}),
    http("playback discover while disconnected is a 409", "POST", "/api/v1/playback-api/discover-song-order", {"confirm": True}),
    http("playback lengths restore without confirm is a 400", "POST", "/api/v1/playback-api/planning-center-lengths/restore"),
    http("playback lengths restore with nothing to restore", "POST", "/api/v1/playback-api/planning-center-lengths/restore", {"confirm": True}),
    http("playback lengths preview, Planning Center not connected", "POST", "/api/v1/playback-api/planning-center-lengths/preview"),
    http("playback lengths confirm, Planning Center not connected", "POST", "/api/v1/playback-api/planning-center-lengths/confirm", {"token": "x", "confirm": True}),
    http("playback lengths confirm, bad body is a 422", "POST", "/api/v1/playback-api/planning-center-lengths/confirm", {"token": "x", "confirm": False}),
    http("playback lengths categories, Planning Center unavailable", "GET", "/api/v1/playback-api/planning-center-lengths/categories"),
    http("playback lengths category, Planning Center unavailable", "PUT", "/api/v1/playback-api/planning-center-lengths/category", {"service_type_id": "abc"}),
    http("playback lengths category, bad characters is a 422", "PUT", "/api/v1/playback-api/planning-center-lengths/category", {"service_type_id": "a b"}),
    {"name": "start the fake Playback", "fake": "start"},
    http("playback settings, manual host and port of the fake", "PUT", "/api/v1/playback-api/settings", {"enabled": True, "host": "127.0.0.1", "port": 47901, "auto_scan": False, "fast_transport": True}),
    {"name": "wait until Playback is connected", "wait": {"path": "/api/v1/playback-api/connection", "key": "connected", "equals": True}},
    http("playback connection, connected", "GET", "/api/v1/playback-api/connection"),
    http("playback status, connected and stopped", "GET", "/api/v1/playback-api/status"),
    {"name": "fake Playback starts playing", "fake": "play"},
    {"name": "wait until Playback reports playing", "wait": {"path": "/api/v1/playback-api/status", "key": "playing", "equals": True}},
    http("playback status, playing", "GET", "/api/v1/playback-api/status"),
    http("playback settings saved while playing", "PUT", "/api/v1/playback-api/settings", {"enabled": True, "host": "127.0.0.1", "port": 47901, "auto_scan": False, "fast_transport": False}),
    {"name": "wait until the reconfigured client is playing again", "wait": {"path": "/api/v1/playback-api/status", "key": "playing", "equals": True}},
    {"name": "fake Playback pauses", "fake": "pause"},
    {"name": "wait until Playback reports stopped", "wait": {"path": "/api/v1/playback-api/status", "key": "playing", "equals": False}},
    http("playback find, typed host of the fake", "POST", "/api/v1/playback-api/find", {"host": "127.0.0.1"}),
    {"name": "wait until the typed scan is done", "wait": {"path": "/api/v1/playback-api/status", "key": "scan.state", "equals": "found"}},
    http("playback status after the typed scan", "GET", "/api/v1/playback-api/status"),
    http("playback find cancel after a scan", "POST", "/api/v1/playback-api/find/cancel"),
    {"name": "fake Playback goes offline", "fake": "go_offline"},
    {"name": "wait until Playback is disconnected", "wait": {"path": "/api/v1/playback-api/status", "key": "last_error", "equals": "Playback unavailable; remote connections may be off"}},
    http("playback connection, offline", "GET", "/api/v1/playback-api/connection"),
    http("playback status, offline", "GET", "/api/v1/playback-api/status"),
    http("playback events", "GET", "/api/v1/playback-api/events"),
    http("playback settings, disabled", "PUT", "/api/v1/playback-api/settings", {"enabled": False}),
    http("playback status, disabled", "GET", "/api/v1/playback-api/status"),
]


# ---- phase 4: the "outputs" profile ---------------------------------------------------------------
# Run against an app with FAKE MIDI (in-memory ports "StagePilot Test In" / "Second Test In" and the
# output "StagePilot Test Out"), a MOCK ProPresenter (scripts/contract/mock_propresenter.py, a
# deterministic HTTP server on loopback) and OSC to a loopback UDP port nothing listens on. No
# MIDI, OSC or ProPresenter traffic leaves the machine. Extra step kinds:
#   {"sleep": seconds}        pause (not recorded)
#   {"mock": "/__reset"}      call the mock ProPresenter (not recorded)
#   {"mock_log": True}        record (and clear) the requests the mock saw, in order
#   "mask": ["connected"]     blank these keys (any depth) in the recorded response: used where the
#                             value is a race inside Python itself (e.g. the connected flag read
#                             while the supervisor is still switching inputs)

IN_ID = "00cc041df87ca4052a02bf61337770d405343b7704301238f3e34359b8ed63d4"  # sha256("StagePilot Test In")
IN2_ID = "c00596005c680b3d2247ab4fc02a7f4c05957ccaa535a2a3aa62ffaad606c52b"  # sha256("Second Test In")
PAD_OSC = "11111111-1111-4111-8111-111111111111"
PAD_MIDI = "22222222-2222-4222-8222-222222222222"
CUE_OSC = "33333333-3333-4333-8333-333333333333"
CUE_MIDI = "44444444-4444-4444-8444-444444444444"


def sleep(seconds: float) -> Step:
    return {"sleep": seconds}


def mock(path: str) -> Step:
    return {"mock": path}


def mock_log(name: str) -> Step:
    return {"name": name, "mock_log": True}


def masked(step: Step, *keys: str) -> Step:
    step["mask"] = list(keys)
    return step


OUTPUT_SCENARIOS: list[Step] = [
    sleep(1.6),
    mock_log("outputs: mock saw the startup probe"),
    http("outputs: health, every output plugin running", "GET", "/api/v1/health"),
    http("outputs: midi inputs, fake devices discovered", "GET", "/api/v1/midi/inputs"),
    http("outputs: midi inputs refresh", "POST", "/api/v1/midi/inputs/refresh"),
    http("outputs: midi messages start empty", "GET", "/api/v1/midi/messages"),
    http("outputs: propresenter status, timer and looks found", "GET", "/api/v1/propresenter"),
    http("outputs: lights status, OSC ready", "GET", "/api/v1/lights"),
    # ---- cue simulation drives the whole chain: MIDI -> state -> ProPresenter -> lights ----------
    http("outputs: cue simulation, start_next", "POST", "/api/v1/midi/cue-simulation", {"cue": "start_next"}),
    sleep(0.3),
    mock_log("outputs: ProPresenter calls for start_next (stop, define, verify, start)"),
    http("outputs: midi messages after the simulated cue", "GET", "/api/v1/midi/messages"),
    http("outputs: propresenter status while running", "GET", "/api/v1/propresenter"),
    http("outputs: input selection refused while the timer runs", "POST", "/api/v1/midi/input-selection", {"input_id": IN2_ID}),
    http("outputs: cue simulation, next", "POST", "/api/v1/midi/cue-simulation", {"cue": "next"}),
    http("outputs: cue simulation, reload_plan", "POST", "/api/v1/midi/cue-simulation", {"cue": "reload_plan"}),
    http("outputs: cue simulation, reload_plan again is a duplicate", "POST", "/api/v1/midi/cue-simulation", {"cue": "reload_plan"}),
    http("outputs: cue simulation, stop_timer", "POST", "/api/v1/midi/cue-simulation", {"cue": "stop_timer"}),
    sleep(0.3),
    mock_log("outputs: ProPresenter calls for next / reload / stop"),
    http("outputs: midi messages with duplicates and dispositions", "GET", "/api/v1/midi/messages"),
    http("outputs: cue simulation, unknown cue is a 422", "POST", "/api/v1/midi/cue-simulation", {"cue": "bogus"}),
    http("outputs: cue simulation, missing cue is a 422", "POST", "/api/v1/midi/cue-simulation", {}),
    http("outputs: state after the simulated show", "GET", "/api/v1/state"),
    # ---- input selection ------------------------------------------------------------------------
    http("outputs: input selection, unknown input is a 409", "POST", "/api/v1/midi/input-selection", {"input_id": "f" * 64}),
    http("outputs: input selection, short id is a 422", "POST", "/api/v1/midi/input-selection", {"input_id": "abc"}),
    http("outputs: input selection, bad characters are a 422", "POST", "/api/v1/midi/input-selection", {"input_id": "Z" * 64}),
    http("outputs: input selection, same input is a no-op", "POST", "/api/v1/midi/input-selection", {"input_id": IN_ID}),
    masked(http("outputs: input selection, switch to the second input", "POST", "/api/v1/midi/input-selection", {"input_id": IN2_ID}), "connected"),
    sleep(0.4),
    http("outputs: midi inputs after the switch", "GET", "/api/v1/midi/inputs"),
    masked(http("outputs: input selection, disconnect", "POST", "/api/v1/midi/input-selection", {"input_id": None}), "connected"),
    sleep(0.4),
    http("outputs: input selection, disconnect again", "POST", "/api/v1/midi/input-selection", {"input_id": None}),
    http("outputs: cue simulation still works with no input", "POST", "/api/v1/midi/cue-simulation", {"cue": "restart_current"}),
    http("outputs: stop after the simulation", "POST", "/api/v1/actions/stop_timer"),
    sleep(0.3),
    mock_log("outputs: ProPresenter calls after the selection tests"),
    # ---- ProPresenter routes --------------------------------------------------------------------
    http("outputs: propresenter test", "POST", "/api/v1/propresenter/test"),
    http("outputs: propresenter timers refresh", "POST", "/api/v1/propresenter/timers/refresh"),
    http("outputs: propresenter settings, missing fields are a 422", "POST", "/api/v1/propresenter/settings", {"host": "127.0.0.1"}),
    http("outputs: propresenter settings, bad port and timeout are a 422", "POST", "/api/v1/propresenter/settings",
         {"host": "127.0.0.1", "port": 0, "timer_name": "Song Countdown", "request_timeout_seconds": 99}),
    http("outputs: propresenter settings, empty host is a 422", "POST", "/api/v1/propresenter/settings",
         {"host": "", "port": 1025, "timer_name": "Song Countdown", "request_timeout_seconds": 3}),
    sleep(0.5),
    mock_log("outputs: mock log before the settings flows"),
    http("outputs: propresenter settings, a Look is triggered and confirmed", "POST", "/api/v1/propresenter/settings",
         {"host": "127.0.0.1", "port": "@MOCK_PORT@", "timer_name": "Song Countdown", "look_id": "look-worship", "request_timeout_seconds": 4}),
    sleep(0.5),
    mock_log("outputs: ProPresenter calls for the settings save"),
    http("outputs: propresenter status after the settings save", "GET", "/api/v1/propresenter"),
    http("outputs: propresenter settings, an unknown Look is refused after saving", "POST", "/api/v1/propresenter/settings",
         {"host": "127.0.0.1", "port": "@MOCK_PORT@", "timer_name": "Song Countdown", "look_id": "look-nope", "request_timeout_seconds": 4}),
    http("outputs: propresenter settings, a missing timer is reported", "POST", "/api/v1/propresenter/settings",
         {"host": "127.0.0.1", "port": "@MOCK_PORT@", "timer_name": "No Such Timer", "request_timeout_seconds": 4}),
    http("outputs: propresenter settings, back to the real timer", "POST", "/api/v1/propresenter/settings",
         {"host": "127.0.0.1", "port": "@MOCK_PORT@", "timer_name": "song countdown", "request_timeout_seconds": 3}),
    # ---- lights ---------------------------------------------------------------------------------
    http("outputs: lights settings, switch to MIDI output", "POST", "/api/v1/lights/settings",
         {"enabled": True, "method": "midi", "output_name": "StagePilot Test Out", "channel": 3, "pulse_ms": 20}),
    http("outputs: lights status, MIDI connected", "GET", "/api/v1/lights"),
    http("outputs: lights refresh lists the outputs", "POST", "/api/v1/lights/outputs/refresh"),
    http("outputs: lights test cue (MIDI)", "POST", "/api/v1/lights/test", {"note": 60, "velocity": 100}),
    http("outputs: lights test cue, note out of range is a 422", "POST", "/api/v1/lights/test", {"note": 200}),
    http("outputs: lights test OSC refused while MIDI is saved", "POST", "/api/v1/lights/test-osc", {"address": "/x", "args": []}),
    http("outputs: lights settings, an OSC save keeps the MIDI output", "POST", "/api/v1/lights/settings",
         {"enabled": True, "method": "osc", "osc_host": "127.0.0.1", "osc_port": 47999}),
    http("outputs: lights test OSC cue", "POST", "/api/v1/lights/test-osc",
         {"address": "/cue/1/go", "args": [{"type": "int", "value": 7}, {"type": "float", "value": 0.5}, {"type": "string", "value": "go"}, {"type": "bool", "value": True}]}),
    http("outputs: lights test MIDI refused while OSC is saved", "POST", "/api/v1/lights/test", {"note": 60}),
    http("outputs: lights test OSC, bad address is a 422", "POST", "/api/v1/lights/test-osc", {"address": "no-slash", "args": []}),
    http("outputs: lights test OSC, address with a space is a 422", "POST", "/api/v1/lights/test-osc", {"address": "/a b", "args": []}),
    http("outputs: lights settings, bad channel is a 422", "POST", "/api/v1/lights/settings", {"method": "midi", "channel": 99}),
    http("outputs: lights settings, empty body is a 422", "POST", "/api/v1/lights/settings", [1]),
    http("outputs: lights cue map, save OSC cues", "PUT", "/api/v1/lights/cue-map",
         {"song_key": "demo-battle-belongs", "song_title": "Battle Belongs", "method": "osc",
          "osc_cues": [{"id": CUE_OSC, "at_seconds": 12.34, "address": " /cue/2 ", "args": [{"type": "float", "value": 1}], "label": " Verse "}]}),
    http("outputs: lights cue map, save MIDI cues keeps the OSC cues", "PUT", "/api/v1/lights/cue-map",
         {"song_key": "demo-battle-belongs", "song_title": "Battle Belongs", "method": "midi",
          "cues": [{"id": CUE_MIDI, "at_seconds": 3, "note": 72, "velocity": 110, "label": "Intro"}]}),
    http("outputs: lights cue map, bad address is a 422", "PUT", "/api/v1/lights/cue-map",
         {"song_key": "s", "song_title": "T", "method": "osc", "osc_cues": [{"at_seconds": 1, "address": "bad"}]}),
    http("outputs: lights cue map, empty key is a 422", "PUT", "/api/v1/lights/cue-map", {"song_key": "  ", "song_title": "T"}),
    http("outputs: settings show the saved lights cue maps", "GET", "/api/v1/settings"),
    http("outputs: lights pads, save", "PUT", "/api/v1/lights/pads",
         {"pads": [{"id": PAD_OSC, "label": "Blackout", "method": "osc", "address": "/blackout", "args": [], "color": "rose"},
                   {"id": PAD_MIDI, "label": "Strobe", "method": "midi", "note": 40, "velocity": 90}]}),
    http("outputs: lights pads, duplicate ids are a 422", "PUT", "/api/v1/lights/pads",
         {"pads": [{"id": PAD_OSC, "label": "A", "method": "osc", "address": "/a"}, {"id": PAD_OSC, "label": "B", "method": "osc", "address": "/b"}]}),
    http("outputs: lights pads, an OSC pad needs an address is a 422", "PUT", "/api/v1/lights/pads",
         {"pads": [{"label": "A", "method": "osc"}]}),
    http("outputs: lights capture fires the pad and adds a cue", "POST", "/api/v1/lights/capture",
         {"pad_id": PAD_OSC, "song_key": "demo-holy-forever", "song_title": "Holy Forever", "at_seconds": 41.26}),
    http("outputs: lights capture of the inactive method's pad is a 409", "POST", "/api/v1/lights/capture", {"pad_id": PAD_MIDI}),
    http("outputs: lights capture of an unknown pad is a 404", "POST", "/api/v1/lights/capture", {"pad_id": "99999999-9999-4999-8999-999999999999"}),
    http("outputs: lights capture needs song fields together is a 422", "POST", "/api/v1/lights/capture", {"pad_id": PAD_OSC, "song_key": "s"}),
    http("outputs: lights undo of an unknown capture", "POST", "/api/v1/lights/capture/undo", {"song_key": "demo-holy-forever", "cue_id": CUE_MIDI}),
    http("outputs: lights undo with nothing saved", "POST", "/api/v1/lights/capture/undo", {"song_key": "nothing", "cue_id": CUE_MIDI}),
    http("outputs: settings show the capture before undo", "GET", "/api/v1/settings"),
    http("outputs: lights settings, disable lighting", "POST", "/api/v1/lights/settings", {"enabled": False, "method": "osc"}),
    http("outputs: lights status, disabled", "GET", "/api/v1/lights"),
    http("outputs: health at the end", "GET", "/api/v1/health"),
]


# ---- the "lightkey" profile -----------------------------------------------------------------------
# Run against the demo plan with Lighting on OSC (to a loopback UDP port nothing listens on) and a
# FAKE lightkey-api (scripts/contract/mock_lightkey.py). One take is seeded for "Battle Belongs".
# Extra step kind: {"mock": "/__change?add=Stars"} changes the fake agent's cue set (not recorded).
# Steps marked "mask": ["message"] blank a message that quotes how many seconds were recorded.

LIGHTKEY = "/api/v1/lights/lightkey"


def lightkey_link_connected() -> Step:
    return {
        "name": "lightkey: wait for the link",
        "wait": {"path": LIGHTKEY, "key": "link", "equals": "connected", "timeout": 20},
    }


LIGHTKEY_SCENARIOS: list[Step] = [
    mock("/__reset"),
    lightkey_link_connected(),
    http("lightkey: status, connected with the seeded take", "GET", LIGHTKEY),
    http("lightkey: settings show the default block", "GET", "/api/v1/settings"),
    masked(http("lightkey: replay off", "PUT", f"{LIGHTKEY}/settings", {"replay": False}), "link", "active_count"),
    lightkey_link_connected(),
    http("lightkey: status, replay switched off", "GET", LIGHTKEY),
    masked(http("lightkey: replay on again", "PUT", f"{LIGHTKEY}/settings", {"replay": True}), "link", "active_count"),
    lightkey_link_connected(),
    http("lightkey: bad address is a 422", "PUT", f"{LIGHTKEY}/settings", {"api_url": "http://a/b c"}),
    http("lightkey: address with a path is a 422", "PUT", f"{LIGHTKEY}/settings", {"api_url": "10.0.0.5:8770/x"}),
    http("lightkey: address too long is a 422", "PUT", f"{LIGHTKEY}/settings", {"api_url": "h" * 301}),
    http("lightkey: replay must be a boolean (422)", "PUT", f"{LIGHTKEY}/settings", {"replay": "maybe"}),
    http("lightkey: api_url must be a string (422)", "PUT", f"{LIGHTKEY}/settings", {"api_url": 5}),
    masked(http("lightkey: replay as the string yes is accepted", "PUT", f"{LIGHTKEY}/settings", {"replay": "yes"}), "link", "active_count"),
    lightkey_link_connected(),
    masked(http("lightkey: empty settings body saves nothing new", "PUT", f"{LIGHTKEY}/settings", {}), "link", "active_count"),
    lightkey_link_connected(),
    http("lightkey: settings body must be an object (422)", "PUT", f"{LIGHTKEY}/settings", [1]),
    # ---- recording -----------------------------------------------------------------------------
    http("lightkey: recording on", "POST", f"{LIGHTKEY}/recording", {"recording": True}),
    http("lightkey: recording on again", "POST", f"{LIGHTKEY}/recording", {"recording": True}),
    http("lightkey: recording needs a body (422)", "POST", f"{LIGHTKEY}/recording", {}),
    http("lightkey: recording must be a boolean (422)", "POST", f"{LIGHTKEY}/recording", {"recording": "nope"}),
    http("lightkey: recording null is not a boolean (422)", "POST", f"{LIGHTKEY}/recording", {"recording": None}),
    http("lightkey: status, armed and replay paused", "GET", LIGHTKEY),
    http("lightkey: start the first song while armed", "POST", "/api/v1/actions/start_next"),
    {"name": "lightkey: wait for the recording to begin", "wait": {"path": LIGHTKEY, "key": "recording_song", "equals": "Battle Belongs", "timeout": 20}},
    http("lightkey: status, recording the song", "GET", LIGHTKEY),
    mock("/__change?add=Stars"),
    {"name": "lightkey: wait for the change to be seen", "wait": {"path": LIGHTKEY, "key": "active_count", "equals": 2, "timeout": 20}},
    http("lightkey: status, a change was seen", "GET", LIGHTKEY),
    http("lightkey: stop the timer mid-song", "POST", "/api/v1/actions/stop_timer"),
    sleep(0.2),
    http("lightkey: status, paused recording", "GET", LIGHTKEY),
    masked(http("lightkey: recording off, too short to keep", "POST", f"{LIGHTKEY}/recording", {"recording": False}), "message"),
    masked(http("lightkey: status, the seeded take is untouched", "GET", LIGHTKEY), "message"),
    masked(http("lightkey: recording off again", "POST", f"{LIGHTKEY}/recording", {"recording": False}), "message"),
    # ---- replay --------------------------------------------------------------------------------
    http("lightkey: back to the start of the plan", "POST", "/api/v1/actions/reset_position"),
    http("lightkey: start the first song, replay runs", "POST", "/api/v1/actions/start_next"),
    {"name": "lightkey: wait for replay to begin", "wait": {"path": LIGHTKEY, "key": "replaying_song", "equals": "Battle Belongs", "timeout": 20}},
    masked(http("lightkey: status, replaying", "GET", LIGHTKEY), "message"),
    http("lightkey: stop the timer, replay stops", "POST", "/api/v1/actions/stop_timer"),
    {"name": "lightkey: wait for replay to stop", "wait": {"path": LIGHTKEY, "key": "replaying_song", "equals": None, "timeout": 20}},
    masked(http("lightkey: status, no longer replaying", "GET", LIGHTKEY), "message"),
    # ---- takes ---------------------------------------------------------------------------------
    masked(http("lightkey: clear a missing take", "DELETE", f"{LIGHTKEY}/takes/nope"), "message"),
    http("lightkey: clear the seeded take", "DELETE", f"{LIGHTKEY}/takes/demo-battle-belongs"),
    http("lightkey: clear it again", "DELETE", f"{LIGHTKEY}/takes/demo-battle-belongs"),
    # ---- Lighting settings keep the Lightkey block ---------------------------------------------
    masked(http("lightkey: replay off before a Lighting save", "PUT", f"{LIGHTKEY}/settings", {"replay": False}), "link", "active_count"),
    lightkey_link_connected(),
    http("lightkey: a Lighting save keeps the Lightkey block", "POST", "/api/v1/lights/settings",
         {"enabled": True, "method": "osc", "osc_host": "127.0.0.1", "osc_port": 47999}),
    lightkey_link_connected(),
    http("lightkey: settings still hold the block", "GET", "/api/v1/settings"),
    http("lightkey: status after the Lighting save", "GET", LIGHTKEY),
    http("lightkey: lighting off", "POST", "/api/v1/lights/settings", {"enabled": False, "method": "osc"}),
    {"name": "lightkey: wait for the link to read off", "wait": {"path": LIGHTKEY, "key": "link", "equals": "off", "timeout": 20}},
    sleep(0.8),
    masked(http("lightkey: status, lighting off", "GET", LIGHTKEY), "active_count"),
    masked(http("lightkey: recording refused while Lighting is off", "POST", f"{LIGHTKEY}/recording", {"recording": True}), "active_count"),
]
