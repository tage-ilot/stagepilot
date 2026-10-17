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
]
