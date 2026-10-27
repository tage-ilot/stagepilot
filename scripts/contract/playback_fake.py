"""Scriptable fake Playback server for the contract replay (standard library only).

A real loopback websocket listener that speaks just enough of Playback's wire protocol for
StagePilot to connect to it, receive heartbeats and be driven by the scripted `fake` steps in
`scenarios.py` (`start`, `play`, `pause`, `seek`, `go_offline`, ...).

Time is simulated: ``tick(n)`` emits ``n`` one-second heartbeats instantly and advances the
position while playing, so a scenario never sleeps. Moved here from the retired Python backend's
test suite; it no longer imports anything from it.
"""

from __future__ import annotations

import base64
import hashlib
import json
import socket
import threading
from contextlib import suppress
from dataclasses import dataclass, field

_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def heartbeat(song: int = 101, position: float = 0, playing: bool = False, version: int = 1) -> str:
    return json.dumps(
        {
            "heartbeat": {
                "stateData": {
                    "setlistSongID": song,
                    "sequenceTime": position,
                    "sequencerPlayState": {"playing" if playing else "stopped": {}},
                    "padPlayerPlayState": {"stopped": {}},
                    "setlistCloudVersion": version,
                }
            }
        }
    )


def frame(payload: bytes, opcode: int = 1, final: bool = True) -> bytes:
    first = opcode | (128 if final else 0)
    if len(payload) < 126:
        return bytes((first, len(payload))) + payload
    return bytes((first, 126)) + len(payload).to_bytes(2, "big") + payload


@dataclass
class FakePlayback:
    """One fake Playback instance. Use as a context manager (``with FakePlayback() as pb``)."""

    song: int = 101
    position: float = 0.0
    playing: bool = False
    version: int = 1
    stalled: bool = False
    online: bool = True
    port: int = 0
    connections: int = 0
    received: list[tuple[int, bytes]] = field(default_factory=list)
    sent: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        self._lock = threading.Lock()
        self._conn: socket.socket | None = None
        self._listener: socket.socket | None = None
        self._stop = threading.Event()
        self._connected = threading.Condition()
        self._thread: threading.Thread | None = None

    # -- lifecycle ---------------------------------------------------------------------
    def __enter__(self) -> FakePlayback:
        self.start()
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def start(self, port: int = 0) -> None:
        """Listen on loopback. ``port`` 0 picks a free one; pass the old port to return."""
        listener = socket.socket()
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", port))
        listener.listen()
        listener.settimeout(0.05)
        self._listener = listener
        self.port = listener.getsockname()[1]
        self.online = True
        self._stop.clear()
        self._thread = threading.Thread(target=self._accept_loop, daemon=True)
        self._thread.start()

    def close(self) -> None:
        self._stop.set()
        self.drop()
        if self._thread:
            self._thread.join(2)
            self._thread = None
        if self._listener:
            self._listener.close()
            self._listener = None
        self.online = False

    def wait_connected(self, count: int = 1, timeout: float = 3.0) -> bool:
        """Blocking (thread) wait for ``count`` handshakes; in async tests poll ``connections``."""
        with self._connected:
            return self._connected.wait_for(lambda: self.connections >= count, timeout)

    # -- scripting: connection ----------------------------------------------------------
    def drop(self) -> None:
        """Close the current client connection (listener stays up)."""
        with self._lock:
            conn, self._conn = self._conn, None
        if conn is not None:
            with suppress(OSError):
                conn.sendall(frame(b"", 8))
            with suppress(OSError):
                conn.shutdown(socket.SHUT_RDWR)
            conn.close()

    def go_offline(self) -> None:
        """Drop the connection AND refuse new ones (Playback quit / Remote Connections off)."""
        port = self.port
        self.drop()
        self._stop.set()
        if self._thread:
            self._thread.join(2)
            self._thread = None
        if self._listener:
            self._listener.close()
            self._listener = None
        self.online = False
        self.port = port

    def come_back(self) -> None:
        """Listen again on the same port after ``go_offline`` (the network returned)."""
        self.start(self.port)

    def stall(self, stalled: bool = True) -> None:
        """Keep the socket open but stop sending heartbeats (``tick`` becomes silent)."""
        self.stalled = stalled

    # -- scripting: playback state ------------------------------------------------------
    def beat(self) -> None:
        """Send one heartbeat with the current song/position/playing (no-op while stalled)."""
        if not self.stalled:
            self._send(heartbeat(self.song, self.position, self.playing, self.version))

    def tick(self, seconds: int = 1) -> None:
        """Simulate ``seconds`` seconds: one heartbeat per second, position advancing if playing."""
        for _ in range(seconds):
            if self.playing:
                self.position += 1.0
            self.beat()

    def play(self) -> None:
        self.playing = True
        self.beat()

    def pause(self) -> None:
        """Playback pause: transport stops, position is kept."""
        self.playing = False
        self.beat()

    def resume(self) -> None:
        self.play()

    def seek(self, position: float) -> None:
        """waveformSeek (works while playing or paused), then the next heartbeat."""
        self.position = position
        self._send(json.dumps({"waveformSeek": {"sequenceTime": position}}))
        self.beat()

    def seek_to_start_and_play(self) -> None:
        self.seek(0.0)
        self.play()

    def select_song(self, song: int, position: float = 0.0) -> None:
        self.song, self.position, self.playing = song, position, False
        self._send(json.dumps({"setlistSelectSong": {"setlistSongID": song}}))
        self.beat()

    def load_setlist(self, setlist_id: int | str) -> None:
        """Announce a setlist id. Without this call the id stays absent (loading setlist)."""
        self._send(json.dumps({"contentLoadSetlist": {"setlistData": {"setlistID": setlist_id}}}))

    def send_raw(self, text: str) -> None:
        self._send(text)

    # -- internals ----------------------------------------------------------------------
    def _send(self, text: str) -> None:
        self.sent.append(text)
        with self._lock:
            conn = self._conn
        if conn is None:
            return
        with suppress(OSError):
            conn.sendall(frame(text.encode()))

    def _accept_loop(self) -> None:
        listener = self._listener
        assert listener is not None
        while not self._stop.is_set():
            try:
                conn, _ = listener.accept()
            except (TimeoutError, OSError):
                continue
            try:
                self._handshake(conn)
            except (OSError, KeyError):
                conn.close()
                continue
            with self._lock:
                old, self._conn = self._conn, conn
            if old is not None:
                old.close()
            with self._connected:
                self.connections += 1
                self._connected.notify_all()
            # Real Playback sends a heartbeat right away; so does the fake (unless stalled).
            self.beat()
            threading.Thread(target=self._drain, args=(conn,), daemon=True).start()

    def _handshake(self, conn: socket.socket) -> None:
        conn.settimeout(3)
        request = b""
        while b"\r\n\r\n" not in request:
            chunk = conn.recv(4096)
            if not chunk:
                raise OSError("closed")
            request += chunk
        headers = dict(
            line.split(": ", 1) for line in request.decode().split("\r\n")[1:] if ": " in line
        )
        accepted = base64.b64encode(
            hashlib.sha1(
                (headers["Sec-WebSocket-Key"] + _GUID).encode(), usedforsecurity=False
            ).digest()
        ).decode()
        conn.sendall(
            (
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                f"Connection: Upgrade\r\nSec-WebSocket-Accept: {accepted}\r\n"
                "Sec-WebSocket-Protocol: pr-protocol\r\n\r\n"
            ).encode()
        )
        conn.settimeout(None)

    def _drain(self, conn: socket.socket) -> None:
        """Record client frames (masked) until the connection ends."""
        try:
            while True:
                header = _read(conn, 2)
                size = header[1] & 127
                mask = _read(conn, 4)
                data = _read(conn, size)
                self.received.append(
                    (header[0] & 15, bytes(v ^ mask[i % 4] for i, v in enumerate(data)))
                )
        except OSError:
            pass


def _read(conn: socket.socket, size: int) -> bytes:
    data = b""
    while len(data) < size:
        chunk = conn.recv(size - len(data))
        if not chunk:
            raise OSError("closed")
        data += chunk
    return data
