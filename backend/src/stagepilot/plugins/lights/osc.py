"""Synchronous, typed boundary around python-osc's UDP client.

Mirrors the MIDI output contracts in ``client.py``: a backend opens an output, the output
sends and closes. OSC over UDP is one-shot (no note length, no handshake), so ``open_output``
only creates the socket; send errors are the only feedback.
"""

from __future__ import annotations

import socket
from collections.abc import Callable, Sequence
from typing import Protocol

from pythonosc.udp_client import SimpleUDPClient

from stagepilot.core.config import OscArgument

type OscValue = int | float | str | bool


class OscOutputContract(Protocol):
    @property
    def closed(self) -> bool: ...

    def send_message(self, address: str, args: Sequence[OscArgument]) -> None: ...

    def close(self) -> None: ...


class OscOutputBackendContract(Protocol):
    def open_output(self, host: str, port: int) -> OscOutputContract: ...


type OscOutputBackendFactory = Callable[[], OscOutputBackendContract]


class UdpOscOutput:
    def __init__(self, client: SimpleUDPClient, target: str) -> None:
        self._client: SimpleUDPClient | None = client
        self._target = target

    @property
    def closed(self) -> bool:
        return self._client is None

    def send_message(self, address: str, args: Sequence[OscArgument]) -> None:
        client = self._client
        if client is None:
            raise RuntimeError("The OSC output is closed.")
        values: list[OscValue] = [argument.value for argument in args]
        try:
            client.send_message(address, values)
        except OSError as exc:
            raise RuntimeError(
                f"OSC send to {self._target} failed: {exc.strerror or exc}."
            ) from exc

    def close(self) -> None:
        client, self._client = self._client, None
        if client is not None:
            sock = getattr(client, "_sock", None)
            if sock is not None:
                sock.close()


class UdpOscOutputBackend:
    """Create UDP OSC outputs. The host is resolved here so a bad host fails on save."""

    def open_output(self, host: str, port: int) -> OscOutputContract:
        target = f"{host}:{port}"
        try:
            client = SimpleUDPClient(host, port)
        except socket.gaierror as exc:
            raise ValueError(f'The OSC host "{host}" could not be found.') from exc
        except OSError as exc:
            raise RuntimeError(f"OSC output to {target} failed: {exc.strerror or exc}.") from exc
        return UdpOscOutput(client, target)
