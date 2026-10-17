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
]
