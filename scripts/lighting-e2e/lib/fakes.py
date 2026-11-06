"""The rig-in-a-box fakes: the lightkey-api relay (UDP 21700) and status API (HTTP), the Lightkey
OSC sink (UDP 21600) and a Playback WebSocket. Nothing here talks to a real computer; every socket
is bound to 127.0.0.1."""
from __future__ import annotations

import asyncio
import json
import random
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

from . import osc

RELAY_VERSION = 1
MAX_DATAGRAM = 1300  # bytes of OSC per relay packet before a command set is split in parts


class FakeRelay(asyncio.DatagramProtocol):
    """What `lightkey-api`'s relay sends to a subscriber (docs/relay.md of that project, as read
    from `lightkey_fixtures::parse_packet`): OSC bundles whose time tag is the DMX-change time on
    the relay computer's clock, headed by `/relay/seq seq kind part parts`; `hello` replies and
    repeats every 2 s, `keyframe` = the whole state, `event` = changed commands; `/relay/ping`
    is answered with `/relay/pong t1 t2 t3`."""

    def __init__(self, host="127.0.0.1", port=21700, skew=0.0, hello_every=2.0, subscriber_ttl=15.0, keyframe_every=10.0):
        self.host, self.port, self.skew = host, port, skew
        self.hello_every, self.ttl, self.keyframe_every = hello_every, subscriber_ttl, keyframe_every
        self.state: dict[str, list] = {}  # address -> args, in first-seen order
        self.stream = random.randint(1, 2**30)
        self.subs: dict[tuple, dict] = {}
        self.transport = None
        self.muted = False
        self.sent_packets = 0
        self.rx_log: list[tuple[float, str]] = []  # (mono, address) of what StagePilot sent us
        self._task = None

    # ---- clock: the relay computer's clock is deliberately NOT ours
    def now(self) -> float:
        return time.time() + self.skew

    async def start(self):
        loop = asyncio.get_running_loop()
        self.transport, _ = await loop.create_datagram_endpoint(lambda: self, local_addr=(self.host, self.port))
        self._task = asyncio.create_task(self._housekeeping())

    async def stop(self):
        if self._task:
            self._task.cancel()
        if self.transport:
            self.transport.close()

    # ---- wire
    def _send(self, sub, kind, messages, tag=None, parts=(1, 1), count=True):
        seq = 0
        if count:
            sub["seq"] = (sub["seq"] + 1) % (1 << 31)
            seq = sub["seq"]
        if self.muted:
            return
        head = osc.message("/relay/seq", [seq, kind, parts[0], parts[1]])
        data = osc.bundle(tag if tag is not None else self.now(), [head] + messages)
        self.transport.sendto(data, sub["addr"])
        self.sent_packets += 1

    def _hello(self):
        return osc.message("/relay/hello", [RELAY_VERSION, self.stream, "FakeProject", len(self.fixtures()), 1])

    def fixtures(self):
        return {a.split("/")[2] for a in self.state if a.count("/") >= 3}

    def _cmds(self, commands):
        return [osc.message(a, v) for a, v in commands]

    def _send_set(self, sub, kind, commands, tag=None):
        msgs = self._cmds(commands)
        chunks, cur, size = [], [], 80
        for m in msgs:
            if cur and size + len(m) + 4 > MAX_DATAGRAM:
                chunks.append(cur); cur, size = [], 80
            cur.append(m); size += len(m) + 4
        chunks.append(cur)
        for i, chunk in enumerate(chunks, 1):
            self._send(sub, kind, chunk, tag=tag, parts=(i, len(chunks)))

    def send_keyframe(self, sub):
        self._send_set(sub, "keyframe", list(self.state.items()))

    def emit(self, commands, tag=None):
        """A DMX change on the relay computer. commands: [(address, [args])]. `tag` is the DMX
        change time on the relay's clock (default: now)."""
        for a, v in commands:
            self.state[a] = list(v)
        tag = self.now() if tag is None else tag
        for sub in self.subs.values():
            self._send_set(sub, "event", commands, tag=tag)

    def load_initial(self, commands):
        for a, v in commands:
            self.state[a] = list(v)

    # ---- asyncio
    def datagram_received(self, data, addr):
        t2 = self.now()
        try:
            a, args = osc.decode_message(data)
        except Exception:
            return
        self.rx_log.append((time.monotonic(), a))
        if a == "/relay/subscribe":
            # docs/relay.md: "Replies with a hello, then a keyframe" - on every subscribe, renewals included
            sub = self.subs.get(addr)
            if sub is None:
                sub = self.subs[addr] = {"addr": addr, "seq": 0, "last": time.monotonic(),
                                         "next_hello": time.monotonic() + self.hello_every,
                                         "next_keyframe": time.monotonic() + self.keyframe_every}
            sub["last"] = time.monotonic()
            self._send(sub, "hello", [self._hello()], count=False)
            self.send_keyframe(sub)
        elif a == "/relay/unsubscribe":
            self.subs.pop(addr, None)
        elif a == "/relay/keyframe":
            sub = self.subs.get(addr)
            if sub:
                self.send_keyframe(sub)
        elif a == "/relay/ping" and args and not self.muted:
            t1 = float(args[0])
            self.transport.sendto(osc.message("/relay/pong", [("d", t1), ("d", t2), ("d", self.now())]), addr)

    async def _housekeeping(self):
        while True:
            await asyncio.sleep(0.25)
            mono = time.monotonic()
            for addr in [a for a, s in self.subs.items() if mono - s["last"] > self.ttl]:
                del self.subs[addr]
            for sub in self.subs.values():
                if mono >= sub["next_hello"]:
                    sub["next_hello"] = mono + self.hello_every
                    self._send(sub, "hello", [self._hello()])
                if mono >= sub["next_keyframe"]:  # relay_keyframe_s (10 s)
                    sub["next_keyframe"] = mono + self.keyframe_every
                    self.send_keyframe(sub)


class FakeLightkey:
    """Lightkey's two faces StagePilot talks to: the OSC override sink (UDP) and the lightkey-api
    status endpoints (HTTP `/time`, `/log`). Every OSC datagram is timestamped on arrival."""

    class _Sink(asyncio.DatagramProtocol):
        def __init__(self, owner):
            self.owner = owner

        def datagram_received(self, data, addr):
            mono, wall = time.monotonic(), time.time()
            try:
                a, args = osc.decode_message(data)
            except Exception:
                self.owner.bad += 1
                return
            self.owner.packets.append((mono, wall, a, args))
            if self.owner.on_packet:
                self.owner.on_packet(a, args)

    def __init__(self, osc_port=21600, api_port=8770, skew=0.0):
        self.osc_port, self.api_port, self.skew = osc_port, api_port, skew
        self.packets: list[tuple[float, float, str, list]] = []
        self.bad = 0
        self.on_packet = None
        self.api_down = False
        self.online = True
        self._transport = None
        self._http = None

    async def start(self):
        loop = asyncio.get_running_loop()
        self._transport, _ = await loop.create_datagram_endpoint(
            lambda: FakeLightkey._Sink(self), local_addr=("127.0.0.1", self.osc_port)
        )
        owner = self

        class H(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *a):
                return

            def do_GET(self):
                url = urlparse(self.path)
                if owner.api_down:
                    body, code = b'{"error":"down"}', 503
                elif url.path == "/time":
                    body, code = json.dumps({"t": (time.time() + owner.skew) * 1000.0}).encode(), 200
                elif url.path == "/log":
                    parse_qs(url.query)
                    body = json.dumps({"events": [], "state": {
                        "online": owner.online, "active": [], "seq": 1,
                        "server_time": (time.time() + owner.skew) * 1000.0}}).encode()
                    code = 200
                else:
                    body, code = b"{}", 404
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self._http = ThreadingHTTPServer(("127.0.0.1", self.api_port), H)
        self._http.daemon_threads = True
        threading.Thread(target=self._http.serve_forever, daemon=True).start()

    async def stop(self):
        if self._transport:
            self._transport.close()
        if self._http:
            self._http.shutdown()
            self._http.server_close()

    def mark(self) -> int:
        return len(self.packets)


class FakePlayback:
    """A Playback WebSocket (`pr-protocol`): a heartbeat ten times a second with the song id and
    position, which is all StagePilot needs to start, pause, resume, seek and stop a song."""

    def __init__(self, port=18981):
        self.port = port
        self.song, self.base, self.since, self.playing = 101, 0.0, 0.0, False
        self.server = None
        self.clients = 0
        self.received: list[str] = []

    def position(self) -> float:
        return self.base + (time.monotonic() - self.since if self.playing else 0.0)

    def play(self, song=None, position=None):
        if song is not None:
            self.song = song
        self.base = self.position() if position is None and self.playing else (position or 0.0)
        self.since, self.playing = time.monotonic(), True

    def pause(self):
        self.base, self.playing = self.position(), False

    def seek(self, position):
        self.base, self.since = position, time.monotonic()

    def stop(self):
        self.base, self.playing = 0.0, False

    def end(self, length):
        """The song reached its end: stopped with the position at the song's length."""
        self.base, self.playing = length, False

    def _heartbeat(self) -> str:
        return json.dumps({"heartbeat": {"stateData": {
            "setlistSongID": self.song, "sequenceTime": round(self.position(), 3),
            "sequencerPlayState": {"playing" if self.playing else "stopped": {}},
            "padPlayerPlayState": {"stopped": {}}, "setlistCloudVersion": 1}}})

    async def start(self):
        import websockets

        async def handler(ws):
            self.clients += 1
            async def reader():
                async for m in ws:
                    self.received.append(m if isinstance(m, str) else "<binary>")
            r = asyncio.create_task(reader())
            try:
                while True:
                    await ws.send(self._heartbeat())
                    await asyncio.sleep(0.1)
            except Exception:
                pass
            finally:
                r.cancel()
                self.clients -= 1

        self.server = await websockets.serve(handler, "127.0.0.1", self.port, subprotocols=["pr-protocol"])

    async def shutdown(self):
        if self.server:
            self.server.close()
