"""Python-client side of the benchmark (same scenario as examples/bench.rs). Usage: bench_py.py <port>"""

from __future__ import annotations

import asyncio
import json
import sys
import time

from stagepilot.plugins.playback_api.client import ConnectionOptions, PlaybackClient
from stagepilot.plugins.playback_api.find import DiscoveryResult
from stagepilot.plugins.playback_api.normalizer import Heartbeat


def rss(field: str) -> int:
    for line in open("/proc/self/status"):
        if line.startswith(field):
            return int(line.split()[1])
    return 0


async def main(port: int) -> None:
    hb = Heartbeat(101, 0.0, False, False, 1)
    client = PlaybackClient(
        ConnectionOptions(host_override="127.0.0.1", port=port),
        finder=lambda _o: DiscoveryResult("127.0.0.1", port, "manual", hb),
    )
    await client.start()
    while not client.status.connected:
        await asyncio.sleep(0.0005)
    connected_at = time.time() * 1000
    await asyncio.sleep(1.5)
    steady = rss("VmRSS:")
    lat = []
    for i in range(50):
        before = client.status.heartbeat.position
        t0 = time.perf_counter()
        await client._discovery_command("seek_end" if i % 2 == 0 else "return_to_start")
        while client.status.heartbeat.position == before:
            if time.perf_counter() - t0 > 3:
                raise SystemExit("no state change")
            await asyncio.sleep(0.0002)
        lat.append((time.perf_counter() - t0) * 1000)
    lat.sort()
    print(json.dumps({"connected_at_ms": connected_at, "rss_kb": steady,
                      "peak_rss_kb": rss("VmHWM:"), "lat_ms": lat}))
    await client.stop()


asyncio.run(main(int(sys.argv[1])))
