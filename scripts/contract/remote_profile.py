"""The "remote", "diagnostics" and "static" contract profiles (phase 6b-A).

remote       the demo app with a RemoteStore and a RemoteFeature (no manager), the fixture dashboard
             in `web/`, on a local port; and the same app behind the trusted `RemoteIngress`
             (public origin https://remote.contract.test) on a second port. Steps say which with
             `via` ("local" or "ingress"). No tunnel, no provider, no network.
diagnostics  the demo app with a scripted Remote manager whose bootstrap points at the mock
             control plane (`mock_control.py`); `POST /api/v1/diagnostics/send` uploads there.
static       the fixture dashboard served by the local port of the remote profile.

Both backends run the SAME steps, so status codes, bodies and the headers listed in
`RECORDED_HEADERS` are compared. Step keys, beyond `name`, `method`, `path`:
  via             "local" (default) or "ingress"
  headers         request headers; "@var@" values are replaced by captured values
  json / raw      request body (JSON value, or the exact text for a malformed body)
  pad             {"key": N}: put N 'a' characters into the JSON body under `key` (oversize bodies)
  cookie          name of a stored session cookie to send
  capture         {"var": "key"} remember body[key]
  capture_header  {"var": "header"} remember a response header
  capture_cookie  store the session cookie this response sets under that name (cleared by a delete)
  mask            keys to blank in the recorded body (ids, tokens: random per run)
  mock_log        {"name", "mock_log": True}: record (and clear) what the mock control plane saw
"""

from __future__ import annotations

import http.client
import json
import re
from typing import Any
from urllib.parse import urlsplit

Step = dict[str, Any]

ORIGIN = "https://remote.contract.test"
COOKIE = "__Host-stagepilot_remote"
PASSWORD = "correct horse battery staple"
VIEWER_PASSWORD = "another long password here"
RECORDED_HEADERS = (
    "content-type",
    "cache-control",
    "retry-after",
    "accept-ranges",
    "content-range",
    "content-length",
)
# Present on both sides, but the value is per-file/per-run.
PRESENCE_HEADERS = ("etag", "last-modified")


def step(name: str, method: str, path: str, **extra: Any) -> Step:
    return {"name": name, "method": method, "path": path, **extra}


REMOTE_HEADER = {"X-StagePilot-Remote": "1"}
INGRESS_HEADERS = {"Origin": ORIGIN, "X-StagePilot-Remote": "1"}
OPERATOR = {"email": "Operator@Example.Test", "password": PASSWORD}
VIEWER = {"email": "viewer@example.test", "password": VIEWER_PASSWORD}
RA = "/api/v1/remote-access"
AUTH = "/api/v1/remote-auth"


def ingress(name: str, method: str, path: str, **extra: Any) -> Step:
    return step(name, method, path, via="ingress", **extra)


def csrf(var: str) -> dict[str, str]:
    return {**INGRESS_HEADERS, "X-CSRF-Token": f"@{var}@"}


REMOTE_SCENARIOS: list[Step] = [
    # ---- local (LAN / desktop) access: the product controls with no manager ----------------------
    step("remote status, nothing set up", "GET", RA),
    step("remote enable needs the remote header", "POST", f"{RA}/enable"),
    step("remote enable", "POST", f"{RA}/enable", headers=REMOTE_HEADER),
    step("remote enable again is idempotent", "POST", f"{RA}/enable", headers=REMOTE_HEADER),
    step("remote status after enable (waiting for the supervisor)", "GET", RA),
    step("remote reset needs a manager", "POST", f"{RA}/reset", headers=REMOTE_HEADER),
    step("remote regenerate needs a manager", "POST", f"{RA}/regenerate", headers=REMOTE_HEADER),
    step("remote bootstrap needs the remote header", "POST", f"{RA}/bootstrap", json=OPERATOR),
    step("remote bootstrap rejects a short body", "POST", f"{RA}/bootstrap", headers=REMOTE_HEADER,
         json={"email": "a@b"}),
    step("remote bootstrap rejects an extra field", "POST", f"{RA}/bootstrap", headers=REMOTE_HEADER,
         json={**OPERATOR, "role": "Viewer"}),
    step("remote bootstrap rejects a bad email", "POST", f"{RA}/bootstrap", headers=REMOTE_HEADER,
         json={"email": "no-at-sign", "password": PASSWORD}),
    step("remote bootstrap rejects a short password", "POST", f"{RA}/bootstrap", headers=REMOTE_HEADER,
         json={"email": "op@example.test", "password": "short"}),
    step("remote bootstrap creates the first operator", "POST", f"{RA}/bootstrap", headers=REMOTE_HEADER,
         json=OPERATOR, mask=["id"]),
    step("remote bootstrap refuses a second", "POST", f"{RA}/bootstrap", headers=REMOTE_HEADER, json=OPERATOR),
    step("remote status, operator exists", "GET", RA),
    # ---- local user administration ---------------------------------------------------------------
    step("users list", "GET", f"{AUTH}/users", mask=["id"]),
    step("users create a viewer", "POST", f"{AUTH}/users", json={**VIEWER, "role": "Viewer"},
         mask=["id"], capture={"viewer_id": "id"}),
    step("users create duplicate", "POST", f"{AUTH}/users", json={**VIEWER, "role": "Viewer"}),
    step("users create with a short password", "POST", f"{AUTH}/users",
         json={"email": "short@example.test", "password": "short", "role": "Viewer"}),
    step("users create with an unknown role", "POST", f"{AUTH}/users",
         json={"email": "role@example.test", "password": PASSWORD, "role": "Admin"}),
    step("users create with a malformed body", "POST", f"{AUTH}/users", raw="{not json"),
    step("users list with both", "GET", f"{AUTH}/users", mask=["id"]),
    step("users update unknown", "PATCH", f"{AUTH}/users/nobody", json={"enabled": False}),
    step("users update rejects a short password", "PATCH", f"{AUTH}/users/@viewer_id@",
         json={"password": "short"}),
    step("users update rejects an unknown field", "PATCH", f"{AUTH}/users/@viewer_id@",
         json={"email": "x@example.test"}),
    step("users update disables the viewer", "PATCH", f"{AUTH}/users/@viewer_id@", json={"enabled": False}),
    step("users update re-enables the viewer", "PATCH", f"{AUTH}/users/@viewer_id@", json={"enabled": True}),
    step("users list after updates", "GET", f"{AUTH}/users", mask=["id"]),
    step("users delete unknown", "DELETE", f"{AUTH}/users/nobody"),
    step("users delete the viewer", "DELETE", f"{AUTH}/users/@viewer_id@"),
    step("users create the viewer again", "POST", f"{AUTH}/users", json={**VIEWER, "role": "Viewer"},
         mask=["id"]),
    step("users list the operator and viewer", "GET", f"{AUTH}/users", mask=["id"], capture={}),
    # ---- local disable (before the ingress steps: a pending enable turns into an error after 15 s,
    # which would make a later status read depend on how long the run took) ----------------------------------------------------------------------------
    step("remote disable", "POST", f"{RA}/disable", headers=REMOTE_HEADER),
    step("remote disable needs the remote header", "POST", f"{RA}/disable"),
    step("remote status after disable", "GET", RA),
    # ---- remote-only auth routes refuse local callers ---------------------------------------------
    step("remote login is not a local route", "POST", f"{AUTH}/login", json=OPERATOR),
    step("remote login validation comes first", "POST", f"{AUTH}/login", json={"email": "x"}),
    step("remote session is not a local route", "GET", f"{AUTH}/session"),
    step("remote logout is not a local route", "POST", f"{AUTH}/logout"),
    step("remote revoke is not a local route", "POST", f"{AUTH}/revoke-sessions"),
    # ---- the trusted remote ingress ---------------------------------------------------------------
    ingress("ingress access, no session", "GET", "/api/v1/access"),
    ingress("ingress serves the login shell", "GET", "/"),
    ingress("ingress serves an asset", "GET", "/assets/app.js"),
    ingress("ingress serves the icon", "GET", "/stagepilot-icon.png"),
    ingress("ingress refuses the state without a session", "GET", "/api/v1/state"),
    ingress("ingress refuses settings without a session", "GET", "/api/v1/settings"),
    ingress("ingress refuses LAN PIN auth", "GET", "/api/v1/dashboard-auth/status"),
    ingress("ingress login needs the same origin", "POST", f"{AUTH}/login", json=OPERATOR,
            headers=REMOTE_HEADER),
    ingress("ingress login rejects another origin", "POST", f"{AUTH}/login", json=OPERATOR,
            headers={"Origin": "https://evil.example", **REMOTE_HEADER}),
    ingress("ingress login needs the remote header", "POST", f"{AUTH}/login", json=OPERATOR,
            headers={"Origin": ORIGIN}),
    ingress("ingress login with a bad body", "POST", f"{AUTH}/login", json={"email": "x"},
            headers=INGRESS_HEADERS),
    ingress("ingress login with an oversize body", "POST", f"{AUTH}/login", json={"email": "", "password": "x"},
            pad={"email": 17000}, headers=INGRESS_HEADERS),
    ingress("ingress login, unknown user", "POST", f"{AUTH}/login",
            json={"email": "nobody@example.test", "password": PASSWORD}, headers=INGRESS_HEADERS),
    ingress("ingress login, wrong password", "POST", f"{AUTH}/login",
            json={"email": OPERATOR["email"], "password": "wrong password!"}, headers=INGRESS_HEADERS),
    ingress("ingress login as the operator", "POST", f"{AUTH}/login", json=OPERATOR,
            headers=INGRESS_HEADERS, capture={"op_csrf": "csrf_token"}, capture_cookie="op",
            mask=["csrf_token"]),
    ingress("ingress session", "GET", f"{AUTH}/session", cookie="op", mask=["id", "csrf_token"]),
    ingress("ingress access as the operator", "GET", "/api/v1/access", cookie="op", mask=["csrf_token"]),
    ingress("ingress health with a session", "GET", "/api/v1/health/live", cookie="op"),
    ingress("ingress mutation without a CSRF token", "POST", "/api/v1/actions/stop_timer", cookie="op",
            headers=INGRESS_HEADERS),
    ingress("ingress mutation with a wrong CSRF token", "POST", "/api/v1/actions/stop_timer", cookie="op",
            headers={**INGRESS_HEADERS, "X-CSRF-Token": "0" * 64}),
    ingress("ingress mutation without an origin", "POST", "/api/v1/actions/stop_timer", cookie="op",
            headers={"X-CSRF-Token": "@op_csrf@"}),
    ingress("ingress action with a csrf token", "POST", "/api/v1/actions/stop_timer", cookie="op",
            headers=csrf("op_csrf")),
    # Keyed retries of one-shot actions: the first runs, the repeat replays, other uses refuse.
    ingress("ingress keyed action, first", "POST", "/api/v1/actions/start_next", cookie="op",
            headers={**csrf("op_csrf"), "Idempotency-Key": "contract-key-0000001"}),
    ingress("ingress keyed action, replay", "POST", "/api/v1/actions/start_next", cookie="op",
            headers={**csrf("op_csrf"), "Idempotency-Key": "contract-key-0000001"}),
    ingress("ingress keyed action, key reused for another action", "POST", "/api/v1/actions/stop_timer",
            cookie="op", headers={**csrf("op_csrf"), "Idempotency-Key": "contract-key-0000001"}),
    ingress("ingress keyed action, malformed key", "POST", "/api/v1/actions/stop_timer", cookie="op",
            headers={**csrf("op_csrf"), "Idempotency-Key": "short"}),
    ingress("ingress keyed action, second key", "POST", "/api/v1/actions/stop_timer", cookie="op",
            headers={**csrf("op_csrf"), "Idempotency-Key": "contract-key-0000002"}),
    ingress("ingress operator reads remote access", "GET", RA, cookie="op"),
    ingress("ingress bootstrap is local only", "POST", f"{RA}/bootstrap", cookie="op",
            headers=csrf("op_csrf"), json={"email": "other@example.test", "password": PASSWORD}),
    ingress("ingress operator lists users", "GET", f"{AUTH}/users", cookie="op", mask=["id"]),
    ingress("ingress operator creates a user", "POST", f"{AUTH}/users", cookie="op", headers=csrf("op_csrf"),
            json={"email": "second@example.test", "password": PASSWORD, "role": "Viewer"},
            mask=["id"], capture={"second_id": "id"}),
    ingress("ingress operator deletes that user", "DELETE", f"{AUTH}/users/@second_id@", cookie="op",
            headers=csrf("op_csrf")),
    # ---- a viewer is read-only --------------------------------------------------------------------
    ingress("ingress login as the viewer", "POST", f"{AUTH}/login", json=VIEWER, headers=INGRESS_HEADERS,
            capture={"vw_csrf": "csrf_token"}, capture_cookie="vw", mask=["csrf_token"]),
    ingress("ingress viewer access", "GET", "/api/v1/access", cookie="vw", mask=["csrf_token"]),
    ingress("ingress viewer reads health", "GET", "/api/v1/health/live", cookie="vw"),
    ingress("ingress viewer cannot read settings", "GET", "/api/v1/settings", cookie="vw"),
    ingress("ingress viewer cannot act", "POST", "/api/v1/actions/stop_timer", cookie="vw", headers=csrf("vw_csrf")),
    ingress("ingress viewer cannot manage remote", "GET", RA, cookie="vw"),
    ingress("ingress viewer cannot list users", "GET", f"{AUTH}/users", cookie="vw"),
    ingress("ingress viewer logs out", "POST", f"{AUTH}/logout", cookie="vw", headers=csrf("vw_csrf")),
    ingress("ingress viewer session is gone", "GET", f"{AUTH}/session", cookie="vw"),
    # ---- logging in again replaces the old session; revoking ends them all ------------------------
    ingress("ingress operator logs in again", "POST", f"{AUTH}/login", json=OPERATOR, cookie="op",
            headers=INGRESS_HEADERS, capture={"op2_csrf": "csrf_token"}, capture_cookie="op2",
            mask=["csrf_token"]),
    ingress("ingress the replaced session is gone", "GET", f"{AUTH}/session", cookie="op"),
    ingress("ingress revoke all sessions", "POST", f"{AUTH}/revoke-sessions", cookie="op2",
            headers=csrf("op2_csrf")),
    ingress("ingress revoked session is gone", "GET", f"{AUTH}/session", cookie="op2"),
    # ---- the login throttle: five misses per account, then 429 -------------------------------------
    *[
        ingress(f"ingress throttle miss {n}", "POST", f"{AUTH}/login",
                json={"email": "throttle@example.test", "password": f"wrong password {n}"},
                headers=INGRESS_HEADERS)
        for n in range(1, 6)
    ],
    ingress("ingress throttled", "POST", f"{AUTH}/login",
            json={"email": "throttle@example.test", "password": "wrong password 6"}, headers=INGRESS_HEADERS),
]

# The Remote manager here is scripted (`remote_app.ContractManager`); the control plane is the mock.
DIAGNOSTICS_SCENARIOS: list[Step] = [
    step("diagnostics send a text bundle", "POST", "/api/v1/diagnostics/send", json={"bundle": "log contents"}),
    step("diagnostics upload as the control plane saw it", "GET", "", mock_log=True),
    step("diagnostics send a JSON bundle", "POST", "/api/v1/diagnostics/send",
         json={"bundle": json.dumps({"app": "stagepilot", "logs": ["a", "b"]})}),
    step("diagnostics upload of the JSON bundle", "GET", "", mock_log=True),
    step("diagnostics send with a non-ASCII bundle", "POST", "/api/v1/diagnostics/send",
         json={"bundle": "caf\u00e9 \u2603"}),
    step("diagnostics upload of the non-ASCII bundle", "GET", "", mock_log=True),
    step("diagnostics upstream rate limit", "POST", "/api/v1/diagnostics/send", json={"bundle": "MODE:429"}),
    step("diagnostics upstream error", "POST", "/api/v1/diagnostics/send", json={"bundle": "MODE:500"}),
    step("diagnostics requests of the failed uploads", "GET", "", mock_log=True),
    step("diagnostics send with no bundle", "POST", "/api/v1/diagnostics/send", json={}),
    step("diagnostics send with a non-string bundle", "POST", "/api/v1/diagnostics/send", json={"bundle": 5}),
    step("diagnostics send with a malformed body", "POST", "/api/v1/diagnostics/send", raw="nope"),
    step("diagnostics send with a list body", "POST", "/api/v1/diagnostics/send", json=[1]),
    step("diagnostics send with an empty body", "POST", "/api/v1/diagnostics/send"),
    step("diagnostics send an oversize bundle", "POST", "/api/v1/diagnostics/send", json={"bundle": ""},
         pad={"bundle": 2 * 1024 * 1024 + 1}),
    step("diagnostics send at the limit is not refused locally", "POST", "/api/v1/diagnostics/send",
         json={"bundle": ""}, pad={"bundle": 2 * 1024 * 1024}),
    step("diagnostics limit upload (the playback section is dropped)", "GET", "", mock_log=True, summary=True),
    # The scripted manager: status, enable/disable, regenerate, reset.
    step("manager status, off", "GET", RA),
    step("manager enable", "POST", f"{RA}/enable", headers=REMOTE_HEADER),
    step("manager enable again is a conflict", "POST", f"{RA}/enable", headers=REMOTE_HEADER),
    step("manager regenerate while enabled", "POST", f"{RA}/regenerate", headers=REMOTE_HEADER),
    step("manager disable", "POST", f"{RA}/disable", headers=REMOTE_HEADER),
    step("manager regenerate while disabled fails", "POST", f"{RA}/regenerate", headers=REMOTE_HEADER),
    step("manager reset needs the remote header", "POST", f"{RA}/reset"),
    step("manager reset", "POST", f"{RA}/reset", headers=REMOTE_HEADER),
    step("diagnostics send after the identity was reset", "POST", "/api/v1/diagnostics/send",
         json={"bundle": "log contents"}),
]

FIXTURE_ETAG = "@etag@"
STATIC_SCENARIOS: list[Step] = [
    step("static index", "GET", "/", capture_header={"etag": "etag", "modified": "last-modified"}),
    step("static index.html", "GET", "/index.html"),
    step("static HEAD index", "HEAD", "/"),
    step("static script", "GET", "/assets/app.js"),
    step("static stylesheet", "GET", "/assets/style.css"),
    step("static icon", "GET", "/stagepilot-icon.png"),
    step("static json file", "GET", "/data.json"),
    step("static text file", "GET", "/empty/file.txt"),
    step("static dot segments collapse", "GET", "/assets/../index.html"),
    step("static directory with an index redirects", "GET", "/sub"),
    step("static directory with an index", "GET", "/sub/"),
    step("static directory without an index", "GET", "/empty/"),
    step("static assets directory listing is refused", "GET", "/assets/"),
    step("static unknown path is a JSON 404, no SPA fallback", "GET", "/dashboard/settings"),
    step("static unknown asset", "GET", "/assets/missing.js"),
    step("static encoded traversal", "GET", "/%2e%2e/%2e%2e/etc/passwd"),
    step("static traversal", "GET", "/../../etc/passwd"),
    step("static encoded NUL", "GET", "/index.html%00"),
    step("static POST", "POST", "/"),
    step("static PUT", "PUT", "/assets/app.js"),
    step("static DELETE", "DELETE", "/index.html"),
    step("static unknown API route is a 404", "GET", "/api/v1/nope"),
    step("static unknown API route, POST", "POST", "/api/v1/nope"),
    step("static range", "GET", "/assets/app.js", headers={"Range": "bytes=0-5"}),
    step("static range suffix", "GET", "/assets/app.js", headers={"Range": "bytes=-4"}),
    step("static range open ended", "GET", "/assets/app.js", headers={"Range": "bytes=10-"}),
    step("static range not satisfiable", "GET", "/assets/app.js", headers={"Range": "bytes=9999-"}),
    step("static range malformed", "GET", "/assets/app.js", headers={"Range": "lines=1-2"}),
    step("static range with no ranges", "GET", "/assets/app.js", headers={"Range": "bytes="}),
    step("static if-none-match", "GET", "/", headers={"If-None-Match": FIXTURE_ETAG}),
    step("static if-none-match, other tag", "GET", "/", headers={"If-None-Match": '"0"'}),
    step("static if-modified-since", "GET", "/", headers={"If-Modified-Since": "@modified@"}),
    step("static if-range matching", "GET", "/", headers={"Range": "bytes=0-3", "If-Range": FIXTURE_ETAG}),
    step("static if-range stale", "GET", "/", headers={"Range": "bytes=0-3", "If-Range": '"0"'}),
]


# ---- running ------------------------------------------------------------------------------------


def _fill(node: Any, variables: dict[str, Any]) -> Any:
    if isinstance(node, str) and node.startswith("@") and node.endswith("@") and node[1:-1] in variables:
        return variables[node[1:-1]]
    if isinstance(node, str) and "@" in node:
        return re.sub(r"@(\w+)@", lambda m: str(variables.get(m.group(1), m.group(0))), node)
    if isinstance(node, dict):
        return {k: _fill(v, variables) for k, v in node.items()}
    if isinstance(node, list):
        return [_fill(v, variables) for v in node]
    return node


def _mask(node: Any, keys: list[str]) -> Any:
    if isinstance(node, dict):
        return {k: ("<masked>" if k in keys else _mask(v, keys)) for k, v in node.items()}
    if isinstance(node, list):
        return [_mask(v, keys) for v in node]
    return node


def _cookie_value(set_cookies: list[str]) -> tuple[str | None, bool]:
    """(value, deleted) of the session cookie among Set-Cookie headers."""
    for line in set_cookies:
        name, _, rest = line.partition("=")
        if name != COOKIE:
            continue
        value = rest.split(";", 1)[0]
        return value, value in ("", '""')
    return None, False


def _normal_cookie(line: str) -> str:
    name, _, rest = line.partition("=")
    attributes = rest.split(";")[1:]
    value = rest.split(";", 1)[0]
    shown = '""' if value in ("", '""') else "<token>"
    cleaned = [re.sub(r"^\s*expires=.*$", " expires=<date>", a, flags=re.I) for a in attributes]
    return ";".join([f"{name}={shown}", *cleaned])


def _exchange(
    base: str, step_: dict[str, Any], headers: dict[str, str], body: bytes | None
) -> tuple[dict[str, Any], list[str], dict[str, str]]:
    parts = urlsplit(base)
    connection = http.client.HTTPConnection(parts.hostname, parts.port, timeout=30)
    try:
        connection.request(step_["method"], step_["path"], body=body, headers=headers)
        response = connection.getresponse()
        raw = response.read()
        lowered = {k.lower(): v for k, v in response.getheaders()}
        set_cookies = [v for k, v in response.getheaders() if k.lower() == "set-cookie"]
        status = response.status
    finally:
        connection.close()
    content_type = lowered.get("content-type", "")
    text = raw.decode("utf-8", errors="replace")
    parsed: Any = text
    if "json" in content_type:
        try:
            parsed = json.loads(text)
        except ValueError:
            parsed = text
    recorded: dict[str, str] = {}
    for name in RECORDED_HEADERS:
        # A JSON body's length follows volatile values (timestamps, ids), so only files and
        # other fixed bodies have a comparable content-length.
        if name == "content-length" and "json" in content_type:
            continue
        if name in lowered:
            recorded[name] = lowered[name]
    for name in PRESENCE_HEADERS:
        if name in lowered:
            recorded[name] = "<present>"
    if "location" in lowered:
        recorded["location"] = lowered["location"].replace(base.rstrip("/"), "<base>")
    if set_cookies:
        recorded["set-cookie"] = " | ".join(_normal_cookie(c) for c in set_cookies)
    return {"status": status, "headers": recorded, "body": parsed}, set_cookies, lowered


def _mock_log(mock_url: str, summary: bool) -> Any:
    parts = urlsplit(mock_url)
    connection = http.client.HTTPConnection(parts.hostname, parts.port, timeout=30)
    try:
        connection.request("GET", "/__log")
        log = json.loads(connection.getresponse().read())
    finally:
        connection.close()
    if summary:
        # The 2 MB body would bloat the transcript: keep its size and whether the playback
        # section was added.
        return [
            {
                **{k: v for k, v in entry.items() if k != "body"},
                "body_bytes": len(entry["body"]) if isinstance(entry["body"], str) else None,
                "has_playback_section": isinstance(entry["body"], str) and "--- playback ---" in entry["body"],
            }
            for entry in log
        ]
    return log


def run(steps: list[Step], urls: dict[str, str], mock_url: str | None = None) -> list[dict[str, Any]]:
    """Run `steps` against `urls` ({"local": ..., "ingress": ...}); return the results."""

    variables: dict[str, Any] = {}
    cookies: dict[str, str] = {}
    results: list[dict[str, Any]] = []
    if mock_url:
        _mock_log(mock_url, False)  # clear anything left over
    for item in steps:
        if item.get("mock_log"):
            assert mock_url is not None
            results.append({"name": item["name"], "mock_requests": _mock_log(mock_url, bool(item.get("summary")))})
            continue
        entry: dict[str, Any] = {"name": item["name"]}
        request = {k: item[k] for k in ("method", "path", "json", "raw", "pad", "headers", "via", "cookie") if k in item}
        entry["request"] = request
        headers = {k: str(_fill(v, variables)) for k, v in item.get("headers", {}).items()}
        if item.get("cookie") and item["cookie"] in cookies:
            headers["Cookie"] = f"{COOKIE}={cookies[item['cookie']]}"
        body: bytes | None = None
        if "raw" in item:
            body = item["raw"].encode()
            headers.setdefault("Content-Type", "application/json")
        elif "json" in item:
            payload = json.loads(json.dumps(item["json"]))
            for key, size in item.get("pad", {}).items():
                payload[key] = "a" * size
            body = json.dumps(_fill(payload, variables)).encode()
            headers.setdefault("Content-Type", "application/json")
        path = _fill(item["path"], variables)
        wire = {**item, "path": path}
        response, set_cookies, lowered = _exchange(urls[item.get("via", "local")], wire, headers, body)
        for var, key in item.get("capture", {}).items():
            if isinstance(response["body"], dict) and key in response["body"]:
                variables[var] = response["body"][key]
        for var, name in item.get("capture_header", {}).items():
            if name in lowered:
                variables[var] = lowered[name]
        if item.get("capture_cookie"):
            value, deleted = _cookie_value(set_cookies)
            if value is not None and not deleted:
                cookies[item["capture_cookie"]] = value
        elif set_cookies and item.get("cookie"):
            value, deleted = _cookie_value(set_cookies)
            if deleted:
                cookies.pop(item["cookie"], None)
        if "mask" in item:
            response = {**response, "body": _mask(response["body"], item["mask"])}
        entry["response"] = response
        results.append(entry)
    return results
