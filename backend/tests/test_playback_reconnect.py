"""Fast reconnect: cheap 1 s re-probe, rare scan, exclusive manual host, true status."""

from __future__ import annotations

import asyncio
import ipaddress
from collections.abc import Callable

import pytest

from playback_fake import FakeClock, FakePlayback
from stagepilot.plugins.playback_api import client as client_mod
from stagepilot.plugins.playback_api import find as find_mod
from stagepilot.plugins.playback_api.client import (
    FULL_SCAN_INTERVAL,
    RECONNECT_PROBE_INTERVAL,
    ConnectionOptions,
    PlaybackClient,
    PlaybackStatus,
)
from stagepilot.plugins.playback_api.normalizer import Heartbeat


async def until(cond: Callable[[], bool], limit: float = 5.0) -> None:
    async def poll() -> None:
        while not cond():  # noqa: ASYNC110 - polling a thread-fed fake
            await asyncio.sleep(0.005)

    await asyncio.wait_for(poll(), limit)


def hb() -> Heartbeat:
    with FakePlayback() as fake:
        return fake.result().heartbeat


class Net:
    """Probe stand-in: records every probe, answers only for hosts in ``up``."""

    def __init__(self) -> None:
        self.up: set[str] = set()
        self.calls: list[str] = []
        self.beat = hb()

    def __call__(self, host: str, port: int, timeout: float) -> Heartbeat | None:
        self.calls.append(host)
        return self.beat if host in self.up else None


@pytest.fixture(autouse=True)
def one_host_lan(monkeypatch: pytest.MonkeyPatch) -> None:
    real = find_mod.find_playback
    lan = (ipaddress.IPv4Network("192.168.9.0/30"),)
    monkeypatch.setattr(
        client_mod,
        "find_playback",
        lambda **kw: real(networks=lambda: lan, **kw),
    )


def locate_client(net: Net, clock: FakeClock, **opts: object) -> PlaybackClient:
    return PlaybackClient(ConnectionOptions(**opts), probe_endpoint=net, clock=clock)  # type: ignore[arg-type]


def test_constants_are_sane() -> None:
    assert RECONNECT_PROBE_INTERVAL == 1.0
    assert FULL_SCAN_INTERVAL >= 30.0


def test_saved_address_probed_every_time_without_scanning() -> None:
    net, clock = Net(), FakeClock()
    c = locate_client(net, clock)
    net.up = {"192.168.9.2"}
    first = c._locate(c.options)
    assert first and first.host == "192.168.9.2" and first.source == "lan"
    c._last_good = first
    net.up = set()
    net.calls.clear()
    clock.advance(1)
    assert c._locate(c.options) is None  # loopback + a (not yet due) scan skipped
    assert net.calls[0] == "192.168.9.2"  # last good probed first, cheaply
    net.calls.clear()
    net.up = {"192.168.9.2"}
    again = c._locate(c.options)
    assert again and net.calls == ["192.168.9.2"]


def test_full_scan_rate_limited_and_only_with_auto_scan() -> None:
    net, clock = Net(), FakeClock()
    c = locate_client(net, clock)
    c._locate(c.options)
    scanned = [h for h in net.calls if h.startswith("192.168.9.")]
    assert scanned  # first attempt scans
    net.calls.clear()
    for _ in range(10):
        clock.advance(1)
        c._locate(c.options)
    assert not [h for h in net.calls if h.startswith("192.168.9.")]
    clock.advance(FULL_SCAN_INTERVAL)
    c._locate(c.options)
    assert [h for h in net.calls if h.startswith("192.168.9.")]

    net2 = Net()
    off = locate_client(net2, FakeClock(), auto_scan=False)
    for _ in range(3):
        off._locate(off.options)
    assert set(net2.calls) == {"127.0.0.1"}


def test_manual_host_never_scans_or_tries_loopback() -> None:
    net, clock = Net(), FakeClock()
    c = locate_client(net, clock, host_override="10.0.0.5", auto_scan=True)
    net.up = {"127.0.0.1", "192.168.9.2"}
    for _ in range(5):
        clock.advance(100)
        assert c._locate(c.options) is None
    assert set(net.calls) == {"10.0.0.5"}
    net.up.add("10.0.0.5")
    found = c._locate(c.options)
    assert found and found.source == "manual"


def test_cancel_stops_scan_early() -> None:
    net, clock = Net(), FakeClock()
    c = locate_client(net, clock)
    c._cancel.set()
    assert c._locate(c.options) is None
    assert net.calls == []


def manual(port: int, **kw: object) -> ConnectionOptions:
    return ConnectionOptions(host_override="127.0.0.1", port=port, **kw)  # type: ignore[arg-type]


def make(fake: FakePlayback, statuses: list[PlaybackStatus]) -> PlaybackClient:
    return PlaybackClient(
        manual(fake.port),
        observer=lambda s, _e: statuses.append(s),
        reconnect_delays=(0.02,),
        heartbeat_timeout=0.4,
    )


async def connected(fake: FakePlayback, client: PlaybackClient, count: int = 1) -> None:
    async def beats() -> None:
        while fake.connections < count or not client.status.connected:
            fake.beat()
            await asyncio.sleep(0.01)

    await asyncio.wait_for(beats(), 5)


async def test_drop_then_return_after_short_gap() -> None:
    statuses: list[PlaybackStatus] = []
    with FakePlayback() as fake:
        client = make(fake, statuses)
        await client.start()
        await connected(fake, client)
        fake.go_offline()
        await until(lambda: not client.status.connected)
        assert client.status.last_error  # true state: disconnected
        await asyncio.sleep(0.15)
        fake.come_back()
        await connected(fake, client, 2)
        assert client.status.connected and client.status.host == "127.0.0.1"
        await client.stop()


async def test_long_outage_does_not_back_off_or_grow_tasks() -> None:
    statuses: list[PlaybackStatus] = []
    with FakePlayback() as fake:
        client = make(fake, statuses)
        await client.start()
        await connected(fake, client)
        before = len(asyncio.all_tasks())
        fake.go_offline()
        await asyncio.sleep(1.0)  # ~50 probe cycles at 20 ms stand in for 60 s at 1 s
        assert len(asyncio.all_tasks()) <= before
        fake.come_back()
        await connected(fake, client, 2)  # no multi-second backoff after a long outage
        await client.stop()


async def test_stalled_socket_times_out_and_reconnects() -> None:
    statuses: list[PlaybackStatus] = []
    with FakePlayback() as fake:
        client = make(fake, statuses)
        await client.start()
        await connected(fake, client)
        fake.stall()
        await until(lambda: not client.status.connected)
        fake.stall(False)
        await connected(fake, client, 2)
        await client.stop()


async def test_host_change_picked_up_promptly_and_saved_state_untouched() -> None:
    statuses: list[PlaybackStatus] = []
    with FakePlayback(song=101) as a, FakePlayback(song=202) as b:
        client = make(a, statuses)
        await client.start()
        await connected(a, client)
        opts = client.options
        await client.reconfigure(manual(b.port))
        await connected(b, client)
        assert client.status.port == b.port
        assert opts.host_override == "127.0.0.1"
        await client.stop()


async def test_remote_connections_toggle_reconnects() -> None:
    statuses: list[PlaybackStatus] = []
    with FakePlayback() as fake:
        client = make(fake, statuses)
        await client.start()
        await connected(fake, client)
        fake.go_offline()  # Remote Connections off
        await until(lambda: not client.status.connected)
        fake.come_back()  # and on again
        await connected(fake, client, 2)
        await client.stop()


async def test_stop_cancels_promptly_while_looking() -> None:
    client = PlaybackClient(manual(1), reconnect_delays=(30.0,))  # port 1: nothing there
    await client.start()
    await asyncio.sleep(0.1)
    await asyncio.wait_for(client.stop(), 3)
    assert client._task is None


def test_default_delays_have_no_backoff() -> None:
    assert PlaybackClient()._delays == (RECONNECT_PROBE_INTERVAL,)
    assert client_mod.RECONNECT_PROBE_TIMEOUT <= 2.0
