"""Identify Playback by negotiated protocol AND validated heartbeat; no commands."""

from __future__ import annotations

import errno
import ipaddress
import re
import socket
import subprocess
import threading
import time
from collections.abc import Callable, Iterable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Literal

from .normalizer import Heartbeat, parse_heartbeat, parse_message
from .ws import WebSocket, WSClosed, WSHandshakeError

DiscoverySource = Literal["manual", "loopback", "lan"]


@dataclass(frozen=True)
class DiscoveryResult:
    host: str
    port: int
    source: DiscoverySource
    heartbeat: Heartbeat


Probe = Callable[[str, int, float], Heartbeat | None]


def probe(host: str, port: int = 8080, timeout: float = 3.0) -> Heartbeat | None:
    deadline = time.monotonic() + timeout
    try:
        ws = WebSocket(host, port, timeout)
        try:
            while time.monotonic() < deadline:
                raw = ws.recv(max(0.001, deadline - time.monotonic()))
                if raw is None:
                    break
                message = parse_message(raw)
                if message and message[0] == "heartbeat":
                    heartbeat = parse_heartbeat(message[1])
                    if heartbeat is not None:
                        return heartbeat
        finally:
            ws.close()
    except (OSError, ValueError):
        pass
    return None


ErrorClass = Literal[
    "permission_denied",
    "no_route",
    "refused",
    "timed_out",
    "not_playback",
    "invalid_address",
    "unexpected_error",
]

_BLOCKED_ERRNOS = {errno.EPERM, errno.EACCES}
_NO_ROUTE_ERRNOS = {
    code
    for code in (
        errno.EHOSTUNREACH,
        errno.ENETUNREACH,
        getattr(errno, "EHOSTDOWN", None),
        getattr(errno, "ENETDOWN", None),
    )
    if code is not None
}


class ProbeFailure(Exception):
    """A probe failed for a classified, reportable reason (never raised for success)."""

    def __init__(self, error_class: ErrorClass, detail: str, step: str | None = None) -> None:
        super().__init__(detail)
        self.error_class: ErrorClass = error_class
        self.detail = detail
        self.step = step


def classify_exception(exc: BaseException) -> tuple[ErrorClass, str, str | None]:
    """Map a probe exception to (class, plain detail, failed step). Never raises."""
    if isinstance(exc, ProbeFailure):
        return exc.error_class, exc.detail, exc.step
    if isinstance(exc, WSHandshakeError):
        return "not_playback", f"Connected, but it did not answer as Playback ({exc})", "handshake"
    if isinstance(exc, WSClosed):
        return "not_playback", f"Connected, but the connection closed early ({exc})", "stream"
    if isinstance(exc, (socket.timeout, TimeoutError)):
        return "timed_out", "No answer before the time limit", "connect"
    if isinstance(exc, ConnectionRefusedError):
        return (
            "refused",
            "The computer answered but nothing is listening on the Playback port",
            "connect",
        )
    if isinstance(exc, PermissionError):
        return (
            "permission_denied",
            f"The operating system blocked the connection ({exc})",
            "connect",
        )
    if isinstance(exc, socket.gaierror):
        return "invalid_address", "That name or address could not be resolved", "resolve"
    if isinstance(exc, OSError):
        if exc.errno in _BLOCKED_ERRNOS:
            return (
                "permission_denied",
                f"The operating system blocked the connection ({exc})",
                "connect",
            )
        if exc.errno in _NO_ROUTE_ERRNOS:
            return "no_route", "No route to that address", "connect"
        if exc.errno in {errno.ECONNREFUSED}:
            return (
                "refused",
                "The computer answered but nothing is listening on the Playback port",
                "connect",
            )
        if exc.errno in {errno.ETIMEDOUT}:
            return "timed_out", "No answer before the time limit", "connect"
        return "unexpected_error", f"{type(exc).__name__}: {exc}", "connect"
    if isinstance(exc, ValueError):
        return "invalid_address", "That is not a valid address", "validate"
    return "unexpected_error", f"{type(exc).__name__}: {exc}", None


def probe_checked(host: str, port: int = 8080, timeout: float = 3.0) -> Heartbeat:
    """Like `probe` but raises so the caller can classify why nothing was found."""
    deadline = time.monotonic() + timeout
    ws = WebSocket(host, port, timeout)
    try:
        while time.monotonic() < deadline:
            raw = ws.recv(max(0.001, deadline - time.monotonic()))
            if raw is None:
                break
            message = parse_message(raw)
            if message and message[0] == "heartbeat":
                heartbeat = parse_heartbeat(message[1])
                if heartbeat is not None:
                    return heartbeat
    finally:
        ws.close()
    raise ProbeFailure(
        "not_playback", "Connected and handshook, but no valid heartbeat arrived", "heartbeat"
    )


def parse_interfaces(text: str) -> tuple[tuple[str, int], ...]:
    interfaces: list[tuple[str, int]] = []
    for match in re.finditer(r"inet\s+(\d+\.\d+\.\d+\.\d+)/(\d+)", text):
        interfaces.append((match[1], int(match[2])))
    for match in re.finditer(
        r"inet\s+(\d+\.\d+\.\d+\.\d+)\s+(?:-->\s+\S+\s+)?netmask\s+(\S+)", text
    ):
        try:
            mask = match[2]
            if mask.lower().startswith("0x"):
                mask = str(ipaddress.IPv4Address(int(mask, 16)))
            prefix = ipaddress.IPv4Network(f"0.0.0.0/{mask}").prefixlen
            interfaces.append((match[1], prefix))
        except ValueError:
            continue
    return tuple(interfaces)


def usable_networks(interfaces: Iterable[tuple[str, int]]) -> tuple[ipaddress.IPv4Network, ...]:
    networks: list[ipaddress.IPv4Network] = []
    for host, prefix in interfaces:
        try:
            address = ipaddress.IPv4Address(host)
            if (
                address.is_loopback
                or address.is_link_local
                or address.is_multicast
                or address.is_unspecified
                or not 0 < prefix < 31
            ):
                continue
            # Never scan more than the attached /24, nor outside a narrower subnet.
            network = ipaddress.IPv4Network(f"{host}/{max(prefix, 24)}", strict=False)
            if network not in networks:
                networks.append(network)
        except ValueError:
            continue
    return tuple(networks)


def local_networks() -> tuple[ipaddress.IPv4Network, ...]:
    for command in (["ip", "-o", "-4", "addr", "show"], ["/sbin/ifconfig"], ["ifconfig"]):
        try:
            output = subprocess.run(command, capture_output=True, text=True, timeout=5, check=False)
        except (OSError, subprocess.SubprocessError):
            continue
        interfaces = parse_interfaces(output.stdout)
        if interfaces:
            return usable_networks(interfaces)
    # Windows uses a stdlib subprocess, not a new Python dependency. CIDR comes
    # from the OS; do not guess attached LANs from hostname DNS/default routes.
    script = (
        "Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { "
        "'inet ' + $_.IPAddress + '/' + $_.PrefixLength }"
    )
    try:
        output = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        return usable_networks(parse_interfaces(output.stdout))
    except (OSError, subprocess.SubprocessError):
        return ()


def find_playback(
    *,
    host_override: str | None = None,
    port: int = 8080,
    auto_scan: bool = True,
    timeout: float = 3.0,
    probe_endpoint: Probe = probe,
    networks: Callable[[], tuple[ipaddress.IPv4Network, ...]] = local_networks,
    cancelled: Callable[[], bool] = lambda: False,
) -> DiscoveryResult | None:
    """Blocking worker-thread operation; manual override never falls back.

    ``cancelled`` is polled before every probe so a stop/config change ends a scan
    within one probe timeout instead of finishing the whole network.
    """
    raw_probe = probe_endpoint

    def guarded(host: str, port: int, timeout: float) -> Heartbeat | None:
        return None if cancelled() else raw_probe(host, port, timeout)

    probe_endpoint = guarded

    if host_override is not None:
        heartbeat = probe_endpoint(host_override, port, timeout)
        return DiscoveryResult(host_override, port, "manual", heartbeat) if heartbeat else None
    heartbeat = probe_endpoint("127.0.0.1", port, timeout)
    if heartbeat:
        return DiscoveryResult("127.0.0.1", port, "loopback", heartbeat)
    if not auto_scan:
        return None
    for network in networks():
        hosts = tuple(str(host) for host in network.hosts())
        with ThreadPoolExecutor(max_workers=32, thread_name_prefix="playback-find") as executor:
            results = executor.map(lambda host: probe_endpoint(host, port, timeout), hosts)
            for host, heartbeat in zip(hosts, results, strict=True):
                if heartbeat:
                    return DiscoveryResult(host, port, "lan", heartbeat)
    return None


@dataclass(frozen=True)
class ScanCandidate:
    name: str
    host: str
    source: DiscoverySource


def computer_name(host: str) -> str:
    """Best-effort friendly name; never raises and falls back to the address."""
    if host == "127.0.0.1":
        return "This computer"
    try:
        name = socket.gethostbyaddr(host)[0].split(".")[0]
    except (OSError, UnicodeError):
        return host
    return name or host


def scan_candidates(
    *,
    host_override: str | None = None,
    port: int = 8080,
    auto_scan: bool = True,
    timeout: float = 3.0,
    probe_endpoint: Probe = probe,
    networks: Callable[[], tuple[ipaddress.IPv4Network, ...]] = local_networks,
    cancel: threading.Event | None = None,
    progress: Callable[[int, int], None] | None = None,
    resolve_name: Callable[[str], str] = computer_name,
) -> list[ScanCandidate]:
    """Blocking: this computer first, then every Playback on the attached LAN."""
    cancel = cancel or threading.Event()
    if host_override is not None:
        hit = probe_endpoint(host_override, port, timeout)
        return [ScanCandidate(resolve_name(host_override), host_override, "manual")] if hit else []
    if probe_endpoint("127.0.0.1", port, timeout):
        return [ScanCandidate(resolve_name("127.0.0.1"), "127.0.0.1", "loopback")]
    if not auto_scan or cancel.is_set():
        return []
    hosts = [str(h) for network in networks() for h in network.hosts()]
    found: list[ScanCandidate] = []
    chunk = 64
    with ThreadPoolExecutor(max_workers=32, thread_name_prefix="playback-scan") as executor:
        for start in range(0, len(hosts), chunk):
            if cancel.is_set():
                break
            batch = hosts[start : start + chunk]
            for host, hit in zip(
                batch, executor.map(lambda h: probe_endpoint(h, port, timeout), batch), strict=True
            ):
                if hit:
                    found.append(ScanCandidate(resolve_name(host), host, "lan"))
            if progress:
                progress(min(start + chunk, len(hosts)), len(hosts))
    return found


# ---------------------------------------------------------------------------
# Diagnosed scan: classified errors, self-test, per-phase progress.
# ---------------------------------------------------------------------------

MAX_LOGGED_ERRORS = 5
SCAN_BLOCK_DETAIL = "macOS is blocking StagePilot's local-network access"
ProgressDetail = Callable[[str, int, int], None]


@dataclass
class ScanReport:
    """Everything a scan learned, safe to log, show, and put in a diagnostics bundle."""

    candidates: list[ScanCandidate] = field(default_factory=list)
    outcome: Literal["found", "not_found", "error", "cancelled"] = "not_found"
    error_class: ErrorClass | None = None
    message: str | None = None
    typed_host: str | None = None
    networks: list[str] = field(default_factory=list)
    interfaces: list[str] = field(default_factory=list)
    hosts_total: int = 0
    hosts_probed: int = 0
    error_counts: dict[str, int] = field(default_factory=dict)
    sample_errors: list[str] = field(default_factory=list)
    loopback_ok: bool | None = None
    lan_check: Literal["ok", "blocked", "unknown", "skipped"] = "skipped"
    duration: float = 0.0
    phases: list[dict[str, object]] = field(default_factory=list)
    classification: str | None = None
    classification_inputs: dict[str, object] = field(default_factory=dict)

    def summary(self) -> dict[str, object]:
        """Privacy-safe dict: addresses only for the typed host the operator entered."""
        return {
            "outcome": self.outcome,
            "error_class": self.error_class,
            "typed_host": self.typed_host,
            "networks": list(self.networks),
            "interfaces": list(self.interfaces),
            "hosts_total": self.hosts_total,
            "hosts_probed": self.hosts_probed,
            "found": len(self.candidates),
            "errors": dict(self.error_counts),
            "sample_errors": list(self.sample_errors),
            "loopback_ok": self.loopback_ok,
            "lan_check": self.lan_check,
            "duration_s": round(self.duration, 2),
            "phases": list(self.phases),
            "classification": self.classification,
            "classification_inputs": dict(self.classification_inputs),
        }


def interface_names(text: str) -> dict[str, str]:
    """Best-effort ip -> interface name from ifconfig / `ip -o addr` output."""
    names: dict[str, str] = {}
    current = ""
    for line in text.splitlines():
        head = re.match(r"^(\S+?):\s", line)
        if head and not line.startswith((" ", "\t")):
            current = head[1]
        for match in re.finditer(r"^\d+:\s+(\S+)\s+inet\s+(\d+\.\d+\.\d+\.\d+)", line):
            names[match[2]] = match[1]
        inet = re.search(r"^\s+inet\s+(\d+\.\d+\.\d+\.\d+)\b", line)
        if inet and current:
            names[inet[1]] = current
    return names


def local_interface_names() -> dict[str, str]:
    for command in (["ip", "-o", "-4", "addr", "show"], ["/sbin/ifconfig"], ["ifconfig"]):
        try:
            output = subprocess.run(command, capture_output=True, text=True, timeout=5, check=False)
        except (OSError, subprocess.SubprocessError):
            continue
        names = interface_names(output.stdout)
        if names:
            return names
    return {}


def default_gateway() -> str | None:
    """Best-effort IPv4 default gateway; None when it cannot be determined."""
    for command, pattern in (
        (["/sbin/route", "-n", "get", "default"], r"gateway:\s*(\d+\.\d+\.\d+\.\d+)"),
        (["ip", "-4", "route", "show", "default"], r"default via (\d+\.\d+\.\d+\.\d+)"),
    ):
        try:
            output = subprocess.run(command, capture_output=True, text=True, timeout=5, check=False)
        except (OSError, subprocess.SubprocessError):
            continue
        match = re.search(pattern, output.stdout)
        if match:
            return match[1]
    return None


def tcp_check(host: str, port: int, timeout: float) -> Literal["ok", "blocked", "unknown"]:
    """TCP connect only (no payload). Refused/connected both prove the OS let packets out."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return "ok"
    except ConnectionRefusedError:
        return "ok"
    except OSError as exc:
        error_class, _detail, _step = classify_exception(exc)
        if error_class in ("permission_denied", "no_route"):
            return "blocked"
        return "unknown"


def loopback_selftest() -> bool:
    """Open and use a loopback socket pair: proves the process can use sockets at all."""
    try:
        with socket.socket() as server:
            server.bind(("127.0.0.1", 0))
            server.listen(1)
            with socket.create_connection(server.getsockname(), timeout=1.0):
                return True
    except OSError:
        return False


def _probe_one(
    probe_endpoint: Probe, host: str, port: int, timeout: float
) -> tuple[Heartbeat | None, ErrorClass | None, str, str | None]:
    try:
        heartbeat = probe_endpoint(host, port, timeout)
    except Exception as exc:
        error_class, detail, step = classify_exception(exc)
        return None, error_class, detail, step
    if heartbeat is None:
        return None, "timed_out", "No Playback answered", None
    return heartbeat, None, "", None


def _tally(report: ScanReport, error_class: ErrorClass, detail: str) -> None:
    report.error_counts[error_class] = report.error_counts.get(error_class, 0) + 1
    if error_class != "timed_out" and len(report.sample_errors) < MAX_LOGGED_ERRORS:
        report.sample_errors.append(f"{error_class}: {detail}")


# A blocked path fails in milliseconds; a genuinely unreachable host takes seconds to time out.
BLOCKED_MIN_SHARE = 0.95
FAST_FAIL_SECONDS = 1.0


def classify_scan_block(
    non_self_counts: dict[str, int], *, loopback_ok: bool | None
) -> tuple[bool, dict[str, object]]:
    """macOS Local Network block signature: loopback works and every other private
    address fails at once with no_route/permission_denied (none timed out). This
    computer's own addresses must already be excluded from `non_self_counts`."""
    total = sum(non_self_counts.values())
    denied = non_self_counts.get("permission_denied", 0) + non_self_counts.get("no_route", 0)
    timed_out = non_self_counts.get("timed_out", 0)
    share = round(denied / total, 3) if total else 0.0
    inputs: dict[str, object] = {
        "loopback_ok": loopback_ok,
        "others_total": total,
        "others_denied": denied,
        "others_timed_out": timed_out,
        "denied_share": share,
    }
    blocked = bool(loopback_ok) and total > 0 and timed_out == 0 and share >= BLOCKED_MIN_SHARE
    return blocked, inputs


def valid_typed_host(host: str) -> bool:
    return bool(host) and len(host) <= 253 and not any(c in host for c in "\r\n /?#\\")


def scan_network(
    *,
    host_override: str | None = None,
    port: int = 8080,
    auto_scan: bool = True,
    timeout: float = 3.0,
    probe_endpoint: Probe = probe_checked,
    networks: Callable[[], tuple[ipaddress.IPv4Network, ...]] = local_networks,
    cancel: threading.Event | None = None,
    progress: Callable[[int, int], None] | None = None,
    on_phase: ProgressDetail | None = None,
    resolve_name: Callable[[str], str] = computer_name,
    gateway: Callable[[], str | None] = default_gateway,
    selftest_loopback: Callable[[], bool] = loopback_selftest,
    selftest_connect: Callable[[str, int, float], Literal["ok", "blocked", "unknown"]] = tcp_check,
    interface_lookup: Callable[[], dict[str, str]] = local_interface_names,
) -> ScanReport:
    """Blocking diagnosed scan. A typed host is ONE direct probe and never scans the LAN."""
    cancel = cancel or threading.Event()
    started = time.monotonic()
    report = ScanReport(typed_host=host_override)

    def phase(name: str, current: int = 0, total: int = 0) -> None:
        if on_phase:
            on_phase(name, current, total)

    def finish() -> ScanReport:
        report.duration = time.monotonic() - started
        return report

    if host_override is not None:
        phase("Connecting", 0, 1)
        if not valid_typed_host(host_override):
            report.outcome, report.error_class = "error", "invalid_address"
            report.message = "That is not a valid address."
            return finish()
        report.hosts_total = 1
        heartbeat, error_class, detail, step = _probe_one(
            probe_endpoint, host_override, port, timeout
        )
        report.hosts_probed = 1
        report.phases.append({"phase": "typed", "probed": 1, "found": int(heartbeat is not None)})
        if heartbeat is not None:
            report.candidates.append(
                ScanCandidate(resolve_name(host_override), host_override, "manual")
            )
            report.outcome = "found"
        else:
            assert error_class is not None
            elapsed = time.monotonic() - started
            if error_class in ("permission_denied", "no_route") and selftest_loopback():
                report.loopback_ok = True
                gate = gateway()
                other_blocked = False
                if gate and gate != host_override:
                    report.lan_check = selftest_connect(gate, 80, 1.0)
                    other_blocked = report.lan_check == "blocked"
                fast = elapsed <= FAST_FAIL_SECONDS
                report.classification_inputs = {
                    "loopback_ok": True,
                    "typed_error": error_class,
                    "typed_elapsed_s": round(elapsed, 2),
                    "second_address_blocked": other_blocked,
                    "gateway_known": bool(gate),
                }
                if error_class == "permission_denied" or (fast and other_blocked):
                    error_class = "permission_denied"
                    detail = SCAN_BLOCK_DETAIL
                    step = None
                    report.classification = "permission_denied"
            report.outcome, report.error_class = "error", error_class
            report.message = detail + (f" (step: {step})" if step else "")
            _tally(report, error_class, detail)
        phase("Connecting", 1, 1)
        return finish()

    # Phase 1: this computer. Self-test loopback first, then probe Playback on it.
    phase("This computer", 0, 1)
    report.loopback_ok = selftest_loopback()
    heartbeat, error_class, detail, _step = _probe_one(probe_endpoint, "127.0.0.1", port, timeout)
    report.phases.append(
        {"phase": "this_computer", "probed": 1, "found": int(heartbeat is not None)}
    )
    report.hosts_probed += 1
    report.hosts_total += 1
    if heartbeat is not None:
        report.candidates.append(ScanCandidate(resolve_name("127.0.0.1"), "127.0.0.1", "loopback"))
        report.outcome = "found"
        return finish()
    if error_class not in (None, "timed_out", "refused"):
        assert error_class is not None
        _tally(report, error_class, detail)
    if not auto_scan or cancel.is_set():
        report.outcome = "cancelled" if cancel.is_set() else "not_found"
        return finish()

    nets = networks()
    report.networks = [str(n) for n in nets]
    names = interface_lookup() if nets else {}
    for ip, name in names.items():
        if name not in report.interfaces and any(ipaddress.IPv4Address(ip) in n for n in nets):
            report.interfaces.append(name)

    # Self-test the LAN path before the sweep so "blocked" is distinguishable.
    gate = gateway()
    if gate:
        report.lan_check = selftest_connect(gate, 80, 1.0)

    hosts = [str(h) for network in nets for h in network.hosts()]
    report.hosts_total += len(hosts)
    chunk = 64
    done = 0
    lan_found = 0
    per_network: dict[str, int] = {str(n): 0 for n in nets}
    owner = {str(h): str(n) for n in nets for h in n.hosts()}
    self_ips = set(names)
    non_self_counts: dict[str, int] = {}
    with ThreadPoolExecutor(max_workers=32, thread_name_prefix="playback-scan") as executor:
        for start in range(0, len(hosts), chunk):
            if cancel.is_set():
                report.outcome = "cancelled"
                return finish()
            batch = hosts[start : start + chunk]
            results = list(
                executor.map(lambda h: _probe_one(probe_endpoint, h, port, timeout), batch)
            )
            for host, (hit, e_class, e_detail, _s) in zip(batch, results, strict=True):
                report.hosts_probed += 1
                per_network[owner[host]] += 1
                if hit is not None:
                    lan_found += 1
                    report.candidates.append(ScanCandidate(resolve_name(host), host, "lan"))
                elif e_class is not None:
                    _tally(report, e_class, e_detail)
                    if host not in self_ips:
                        non_self_counts[e_class] = non_self_counts.get(e_class, 0) + 1
            done = min(start + chunk, len(hosts))
            if progress:
                progress(done, len(hosts))
            phase(
                "Network " + (report.networks[0] if len(report.networks) == 1 else "scan"),
                done,
                len(hosts),
            )
    for net, count in per_network.items():
        report.phases.append({"phase": f"network {net}", "probed": count, "found": 0})
    report.phases.append({"phase": "lan_total", "probed": done, "found": lan_found})

    if report.candidates:
        report.outcome = "found"
        return finish()
    counts = report.error_counts
    block_signature, report.classification_inputs = classify_scan_block(
        non_self_counts, loopback_ok=report.loopback_ok
    )
    report.classification_inputs["self_addresses_excluded"] = len(self_ips)
    report.classification_inputs["lan_check"] = report.lan_check
    if report.loopback_ok and (report.lan_check == "blocked" or block_signature):
        report.outcome, report.error_class = "error", "permission_denied"
        report.classification = "permission_denied"
        report.message = SCAN_BLOCK_DETAIL
    elif not nets:
        report.outcome, report.error_class = "error", "no_route"
        report.message = "No usable network was found on this computer."
    elif counts.get("unexpected_error", 0) and counts["unexpected_error"] >= len(hosts) // 2:
        report.outcome, report.error_class = "error", "unexpected_error"
        report.message = report.sample_errors[0] if report.sample_errors else "Unexpected error"
    else:
        report.outcome = "not_found"
    return finish()


def saved_address_blocked(
    *,
    selftest_loopback: Callable[[], bool] = loopback_selftest,
    gateway: Callable[[], str | None] = default_gateway,
    selftest_connect: Callable[[str, int, float], Literal["ok", "blocked", "unknown"]] = tcp_check,
) -> bool:
    """A saved address just failed immediately: is the OS blocking all local-network use?"""
    if not selftest_loopback():
        return False
    gate = gateway()
    return bool(gate) and selftest_connect(str(gate), 80, 1.0) == "blocked"
