"""The "pco" contract profile (phase 5): Planning Center flows against a deterministic mock.

Both backends run the SAME scenarios against the SAME mock (`mock_pco.py`: the Services API and
the control-plane OAuth relay), so the requests each one makes are compared as well as the
answers it gives. Nothing here contacts the real Planning Center or the real control plane.

Step kinds, besides plain HTTP steps (`http(...)`):
  {"mock_set": {...}}          POST the fault-injection mode to the mock (not recorded)
  {"mock_reset": True}         clear the mock's mode, lengths and log (not recorded)
  {"mock_log": True, "name"}   record (and clear) the requests the mock saw, in order
  {"sleep": seconds}           pause (not recorded)
  "capture": {"var": "key"}    remember body[key] of this response as @var@
  "mask": ["key", ...]         blank these keys (any depth) in the recorded response; used for
                               random values (the OAuth `state`, the PKCE challenge)
Placeholders `@var@` in a request body are replaced by captured values before sending.
"""

from __future__ import annotations

import json
import time
import urllib.request
from typing import Any

Step = dict[str, Any]

ALL_TYPES = "stagepilot:all-service-types"
SECRET_KEYS = ["authorize_url", "state"]


def http(name: str, method: str, path: str, body: Any = None, **extra: Any) -> Step:
    step: Step = {"name": name, "method": method, "path": path}
    if body is not None:
        step["json"] = body
    step.update(extra)
    return step


def mock_set(mode: dict[str, Any]) -> Step:
    return {"mock_set": mode}


MOCK_RESET: Step = {"mock_reset": True}


def mock_log(name: str) -> Step:
    return {"name": name, "mock_log": True}


P = "/api/v1/planning-center"

PCO_SCENARIOS: list[Step] = [
    # ---- configured at boot (personal access token, one service type): the plan is loaded ------
    http("pco status, connected at boot", "GET", f"{P}/status"),
    http("pco health shows the running plugin", "GET", "/api/v1/health"),
    http("pco state, plan loaded with skipped items", "GET", "/api/v1/state"),
    http("pco settings read back never exposes the secret", "GET", "/api/v1/settings"),
    http("pco oauth status, nothing stored", "GET", f"{P}/oauth/status"),
    http("pco pending selection, nothing pending", "GET", f"{P}/plans/pending-selection"),
    http("pco plan selection, nothing pending is a 409", "POST", f"{P}/plans/select", {"plan_id": "1001"}),
    http("pco service types, saved credentials", "GET", f"{P}/service-types"),
    mock_log("pco mock saw the service type listing"),
    # ---- connection test ----------------------------------------------------------------------------------
    http("pco test, wrong secret is a 401", "POST", f"{P}/test", {"app_id": "app-1", "secret": "wrong"}),
    http("pco test, right credentials list the active service types", "POST", f"{P}/test", {"app_id": "app-1", "secret": "secret-1"}),
    http("pco test, empty strings are a 422", "POST", f"{P}/test", {"app_id": "", "secret": ""}),
    http("pco test, unknown key is a 422", "POST", f"{P}/test", {"bogus": 1}),
    http("pco test, non-object body is a 422", "POST", f"{P}/test", [1]),
    http("pco test, saved credentials are used when nothing is given", "POST", f"{P}/test", {}),
    mock_log("pco mock saw the connection tests"),
    # ---- manual settings -------------------------------------------------------------------------------------
    http("pco settings, unknown key is a 422", "POST", f"{P}/settings", {"bogus": 1}),
    http("pco settings, secret and remove_secret together is a 422", "POST", f"{P}/settings", {"secret": "x", "remove_secret": True}),
    http("pco settings, lookahead out of range is a 422", "POST", f"{P}/settings", {"upcoming_lookahead_days": 999}),
    http("pco settings, bad service time is a 422", "POST", f"{P}/settings", {"preferred_service_time": "25:00"}),
    http("pco plan reload action", "POST", f"{P}/plan/reload"),
    {"sleep": 1.0},
    mock_log("pco mock saw the plan reload"),
    http("pco settings, archived service type is applied but reported", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": "333"}),
    http("pco status, configuration error keeps the API connected", "GET", f"{P}/status"),
    http("pco settings, remove the saved secret", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": "111", "remove_secret": True}),
    http("pco status, secret removed", "GET", f"{P}/status"),
    http("pco service types, not configured is a 409", "GET", f"{P}/service-types"),
    http("pco test, nothing to use is a 409", "POST", f"{P}/test", {}),
    http("pco settings, secret saved again", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": "111", "secret": "secret-1"}),
    http("pco status, reconnected", "GET", f"{P}/status"),
    # ---- all service types: two plans on the same day -> the operator chooses -----------------------------------------
    http("pco settings, all service types is ambiguous", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": ALL_TYPES}),
    http("pco pending selection lists both candidates", "GET", f"{P}/plans/pending-selection"),
    http("pco state, ambiguous load", "GET", "/api/v1/state"),
    http("pco plan selection, unknown plan is a 409", "POST", f"{P}/plans/select", {"plan_id": "424242"}),
    http("pco plan selection, unsafe id is a 422", "POST", f"{P}/plans/select", {"plan_id": "../x"}),
    http("pco plan selection, missing id is a 422", "POST", f"{P}/plans/select", {}),
    http("pco plan selection, choose the youth plan", "POST", f"{P}/plans/select", {"plan_id": "2001"}),
    http("pco pending selection, resolved", "GET", f"{P}/plans/pending-selection"),
    http("pco plan selection alias route", "POST", f"{P}/plan-selection", {"plan_id": "2001"}),
    mock_log("pco mock saw the all-types discovery"),
    # a preferred plan title resolves the ambiguity without asking
    http("pco settings, preferred title picks the plan", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": ALL_TYPES, "plan_title_preference": "youth night"}),
    http("pco state, preferred plan loaded", "GET", "/api/v1/state"),
    http("pco settings, preferred service time picks the plan", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": ALL_TYPES, "plan_title_preference": None, "preferred_service_time": "09:00"}),
    http("pco state, plan chosen by service time", "GET", "/api/v1/state"),
    http("pco settings, back to one service type", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": "111", "preferred_service_time": None}),
    # ---- API failures: status codes and Retry-After ----------------------------------------------------------------------
    mock_set({"status": {"/services": 401}}),
    http("pco service types, API 401", "GET", f"{P}/service-types"),
    http("pco plan reload with a rejected token", "POST", f"{P}/plan/reload"),
    {"sleep": 1.0},
    http("pco status after a 401 load", "GET", f"{P}/status"),
    mock_set({"status": {"/services": 403}}),
    http("pco service types, API 403", "GET", f"{P}/service-types"),
    mock_set({"status": {"/services": 429}, "retry_after": "7"}),
    http("pco service types, API 429 keeps Retry-After", "GET", f"{P}/service-types"),
    mock_set({"status": {"/services": 429}}),
    http("pco service types, API 429 without Retry-After", "GET", f"{P}/service-types"),
    mock_set({"status": {"/services": 503}}),
    http("pco service types, API 503 is a 502", "GET", f"{P}/service-types"),
    mock_set({"garbage": ["/services"]}),
    http("pco service types, invalid JSON is a 502", "GET", f"{P}/service-types"),
    mock_set({"delay": {"/services": 2.5}}),
    http("pco settings, one second timeout", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": "111", "request_timeout_seconds": 1}),
    http("pco service types, request timeout is a 502", "GET", f"{P}/service-types"),
    MOCK_RESET,
    http("pco settings, normal timeout again", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": "111", "request_timeout_seconds": 5}),
    http("pco status, recovered", "GET", f"{P}/status"),
    # ---- OAuth sign-in ----------------------------------------------------------------------------------------------------
    http("pco oauth start", "POST", f"{P}/oauth/start", None, mask=SECRET_KEYS, capture={"state": "state"}),
    http("pco oauth callback, wrong state is a 401", "POST", f"{P}/oauth/callback", {"state": "wrong.state", "code": "good-code", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    http("pco oauth callback, missing code is a 422", "POST", f"{P}/oauth/callback", {"state": "x", "redirect_uri": "http://127.0.0.1:1/cb"}),
    http("pco oauth callback, unknown key is a 422", "POST", f"{P}/oauth/callback", {"state": "x", "code": "c", "redirect_uri": "u", "extra": 1}),
    http("pco oauth callback, empty fields are a 422", "POST", f"{P}/oauth/callback", {"state": "", "code": "", "redirect_uri": ""}),
    http("pco oauth callback, rejected grant is a permanent 401", "POST", f"{P}/oauth/callback", {"state": "@state@", "code": "bad-grant", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    http("pco oauth callback, busy control plane is a transient 503", "POST", f"{P}/oauth/callback", {"state": "@state@", "code": "busy", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    http("pco oauth callback, control plane error is a transient 503", "POST", f"{P}/oauth/callback", {"state": "@state@", "code": "boom", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    http("pco oauth status, still disconnected", "GET", f"{P}/oauth/status"),
    http("pco oauth callback, good code connects", "POST", f"{P}/oauth/callback", {"state": "@state@", "code": "good-code", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    http("pco oauth callback, second use of the flow is a 401", "POST", f"{P}/oauth/callback", {"state": "@state@", "code": "good-code", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    http("pco oauth status, connected", "GET", f"{P}/oauth/status"),
    http("pco status, oauth connected", "GET", f"{P}/status"),
    http("pco settings read back, oauth selected", "GET", "/api/v1/settings"),
    mock_log("pco mock saw the sign-in and the bearer load"),
    http("pco settings form keeps the oauth method", "POST", f"{P}/settings", {"app_id": "app-1", "service_type_id": "111", "request_timeout_seconds": 5}),
    http("pco settings read back, still oauth", "GET", "/api/v1/settings"),
    http("pco service types with the bearer token", "GET", f"{P}/service-types"),
    mock_log("pco mock saw the bearer service types"),
    # ---- OAuth refresh and revocation ---------------------------------------------------------------------------------------------
    http("pco oauth disconnect", "POST", f"{P}/oauth/disconnect"),
    http("pco oauth status after disconnect", "GET", f"{P}/oauth/status"),
    http("pco status after disconnect", "GET", f"{P}/status"),
    mock_set({"expires_in": 60}),
    http("pco oauth start, short lived token", "POST", f"{P}/oauth/start", None, mask=SECRET_KEYS, capture={"state": "state"}),
    http("pco oauth callback, short lived token", "POST", f"{P}/oauth/callback", {"state": "@state@", "code": "good-code", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    mock_log("pco mock saw the short lived sign-in"),
    http("pco service types refresh the token first", "GET", f"{P}/service-types"),
    mock_log("pco mock saw the refresh and the bearer call"),
    http("pco oauth status after refresh", "GET", f"{P}/oauth/status"),
    mock_set({"revoke_refresh": True, "expires_in": 60}),
    http("pco oauth disconnect before revocation test", "POST", f"{P}/oauth/disconnect"),
    http("pco oauth start, revocation test", "POST", f"{P}/oauth/start", None, mask=SECRET_KEYS, capture={"state": "state"}),
    http("pco oauth callback, revocation test", "POST", f"{P}/oauth/callback", {"state": "@state@", "code": "good-code", "redirect_uri": "http://127.0.0.1:53682/callback"}),
    http("pco service types, refresh token revoked", "GET", f"{P}/service-types"),
    http("pco oauth status, needs reconnect", "GET", f"{P}/oauth/status"),
    http("pco status, needs reconnect", "GET", f"{P}/status"),
    mock_log("pco mock saw the revoked refresh"),
    MOCK_RESET,
    http("pco oauth disconnect, final", "POST", f"{P}/oauth/disconnect"),
    http("pco status, final", "GET", f"{P}/status"),
]


def _mock_call(mock_url: str, path: str, body: Any = None, method: str = "POST") -> Any:
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(
        mock_url + path,
        data=data,
        method=method,
        headers={"Content-Type": "application/json"} if data else {},
    )
    with urllib.request.urlopen(request, timeout=15) as response:
        raw = response.read()
    return json.loads(raw) if raw else None


def _mask(node: Any, keys: list[str]) -> Any:
    if isinstance(node, dict):
        return {k: ("<masked>" if k in keys else _mask(v, keys)) for k, v in node.items()}
    if isinstance(node, list):
        return [_mask(v, keys) for v in node]
    return node


def _fill(node: Any, variables: dict[str, Any]) -> Any:
    if isinstance(node, str) and node.startswith("@") and node.endswith("@") and node[1:-1] in variables:
        return variables[node[1:-1]]
    if isinstance(node, dict):
        return {k: _fill(v, variables) for k, v in node.items()}
    if isinstance(node, list):
        return [_fill(v, variables) for v in node]
    return node


def run_pco(transport: Any, mock_url: str) -> list[dict[str, Any]]:
    """Run PCO_SCENARIOS through `transport` (anything with `.http(step)`), return the results."""

    _mock_call(mock_url, "/__reset", {})
    variables: dict[str, Any] = {}
    results: list[dict[str, Any]] = []
    for step in PCO_SCENARIOS:
        if "mock_set" in step:
            _mock_call(mock_url, "/__mode", step["mock_set"])
            continue
        if "mock_reset" in step:
            _mock_call(mock_url, "/__reset", {})
            continue
        if "sleep" in step:
            time.sleep(step["sleep"])
            continue
        if "mock_log" in step:
            results.append({"name": step["name"], "mock_requests": _mock_call(mock_url, "/__log", method="GET")})
            continue
        entry: dict[str, Any] = {"name": step["name"]}
        # The recorded request is the template (placeholders intact); the wire request has the
        # captured values filled in.
        entry["request"] = {k: step[k] for k in ("method", "path", "json") if k in step}
        wire = {**step, "json": _fill(step["json"], variables)} if "json" in step else step
        response = transport.http(wire)
        for var, key in step.get("capture", {}).items():
            if isinstance(response["body"], dict) and key in response["body"]:
                variables[var] = response["body"][key]
        if "mask" in step:
            response = {**response, "body": _mask(response["body"], step["mask"])}
        entry["response"] = response
        results.append(entry)
    return results
