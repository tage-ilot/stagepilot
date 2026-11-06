"""The rig-in-a-box: fakes + backend, and the helpers every scenario uses."""
from __future__ import annotations

import asyncio
import json
import time
from datetime import datetime
from pathlib import Path

from .backend import Backend
from .fakes import FakeLightkey, FakePlayback, FakeRelay

REPO = Path(__file__).resolve().parents[3]
TAKES_DEFAULT = Path.home() / "AgentHub-data/projects/stagepilot-lightkey-smooth/data/takes-fixtures.json"
LEAD = 0.045  # LOOK_LEAD_SECONDS: look rows and interpolated values leave this early

# demo plan songs (stagepilot-server/src/demo.rs): key -> (title, seconds)
DEMO = {
    "demo-battle-belongs": ("Battle Belongs", 281),
    "demo-holy-forever": ("Holy Forever", 336),
    "demo-gratitude": ("Gratitude", 352),
    "demo-firm-foundation": ("Firm Foundation", 377),
}
# the Playback song id each demo song is bound to (learned while it plays)
PB_ID = {"demo-battle-belongs": 101, "demo-holy-forever": 102, "demo-gratitude": 103, "demo-firm-foundation": 104}
# which real take is fed to which demo song (a take must fit inside its song)
TAKE_FOR = {"Unstoppable God": "demo-battle-belongs", "Able": "demo-holy-forever"}


def iso(s: str) -> float:
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def load_takes(path=None):
    doc = json.loads(Path(path or TAKES_DEFAULT).read_text())
    out = {}
    for t in doc["takes"]:
        f = t["fixtures"]
        out[t["song_title"]] = {
            "title": t["song_title"], "duration": float(t["duration_seconds"]),
            "initial": [(c["a"], c["v"]) for c in f["initial"]],
            "events": [(e["at_seconds"], [(c["a"], c["v"]) for c in e["osc"]]) for e in f["events"]],
        }
    return out


class Rig:
    def __init__(self, workdir: Path, binary: Path | None = None, backend_port=18900, skew=3.25,
                 echo=True, echo_delay=0.02, relay_latency=0.005):
        self.workdir = Path(workdir)
        self.binary = Path(binary or REPO / "target/release/stagepilot-server")
        self.backend_port = backend_port
        self.relay = FakeRelay(skew=skew)
        self.lk = FakeLightkey()
        self.pb = FakePlayback(18981)
        self.backend: Backend | None = None
        self.echo, self.echo_delay, self.relay_latency = echo, echo_delay, relay_latency
        self.echoed = 0
        self.pbmap = dict(PB_ID)

    # ---------------------------------------------------------------- lifecycle
    def _settings(self):
        return {
            "schema_version": 2,
            "integration_modes": {"service_source": "demo", "midi_source": "playback_api", "timer_output": "simulated"},
            "web_dashboard_pin_enabled": False,
            "playback_api": {"enabled": True, "host": "127.0.0.1", "port": self.pb.port, "auto_scan": False},
        }

    async def up(self, configure=True):
        await self.relay.start()
        await self.lk.start()
        await self.pb.start()
        self.lk.on_packet = self._echo
        self.backend = Backend(self.binary, self.workdir / "backend", self.backend_port, self._settings())
        self.backend.start()
        if configure:
            self.configure()
        log("rig up: backend", self.backend.base, "pid", self.backend.proc.pid)

    def restart_backend(self, kill=False):
        self.backend.stop(kill=kill)
        self.backend.start()

    def pb_id(self, key):
        return self.pbmap[key]

    async def restart_stopped(self):
        """Start the (stopped) backend again with the Playback stopped. The Playback song ->
        plan song pairing lives in memory and is learned while songs play (it needs a whole song to
        advance), so after a restart the first Playback song seen resolves to the plan's Up next."""
        self.pb.stop()
        self.backend.start()
        await self.wait_link()
        await asyncio.sleep(1)

    async def down(self):
        if self.backend:
            self.backend.stop()
        await self.relay.stop()
        await self.lk.stop()
        await self.pb.shutdown()

    def configure(self):
        """Lighting setup through the HTTP API only: OSC to the fake Lightkey, the API at 127.0.0.1."""
        b = self.backend
        r = b.post("/api/v1/lights/settings", {"enabled": True, "method": "osc", "osc_host": "127.0.0.1",
                                               "osc_port": self.lk.osc_port})
        r2 = b.put("/api/v1/lights/lightkey/settings",
                   {"api_url": f"http://127.0.0.1:{self.lk.api_port}", "replay": True, "mode": "auto"})
        return r, r2

    def _echo(self, addr, args):
        """Lightkey puts every override on its DMX output; the relay reports it back a moment
        later, quantised to DMX like the real decode (8 bit for levels)."""
        if not self.echo or addr.endswith("/clear"):
            return
        out = [self._dmx(addr, v) for v in args]

        def go():
            self.relay.emit([(addr, out)])
            self.echoed += 1
        asyncio.get_running_loop().call_later(self.echo_delay, go)

    @staticmethod
    def _dmx(addr, v):
        if isinstance(v, float) and 0.0 <= v <= 1.0 and "ngle" not in addr:
            return float(int(v * 255.0)) / 255.0
        return v

    # ---------------------------------------------------------------- status
    def lk_status(self):
        return self.backend.get("/api/v1/lights/lightkey")

    def state(self):
        return self.backend.get("/api/v1/state")

    def settings(self):
        return self.backend.get("/api/v1/settings")["settings"]

    def cue_map(self, key):
        return (self.settings()["lights"].get("cue_maps") or {}).get(key)

    def set_mode(self, mode):
        """auto | replay | record. Record is the recording switch; the others the settings route."""
        if mode == "record":
            return self.backend.post("/api/v1/lights/lightkey/recording", {"recording": True})
        if self.lk_status()["recording"]:
            self.backend.post("/api/v1/lights/lightkey/recording", {"recording": False})
        return self.backend.put("/api/v1/lights/lightkey/settings", {"mode": mode})

    async def wait(self, pred, timeout=30.0, every=0.1, what="condition"):
        t0 = time.time()
        while time.time() - t0 < timeout:
            v = await asyncio.to_thread(pred)
            if v:
                return v
            await asyncio.sleep(every)
        raise TimeoutError(f"timed out waiting for {what}")

    async def wait_link(self, timeout=30):
        await self.wait(lambda: (lambda s: s["link"] == "connected" and ((s.get("fixtures") or {}).get("link") in ("live", None)))(self.lk_status()),
                        timeout, what="link connected (+ relay live when reported)")

    # ---------------------------------------------------------------- the relay's state
    def set_relay_state(self, commands):
        """Bring the relay's fixture state to `commands` (only the differences are sent)."""
        diff = [(a, v) for a, v in commands if self.relay.state.get(a) != list(v)]
        if diff and self.relay.subs:
            self.relay.emit(diff)
        else:
            self.relay.load_initial(diff)

    # ---------------------------------------------------------------- the song
    async def play(self, song_key, position=0.0):
        """Start (or move to) a song on the fake Playback; returns the song's wall-clock origin as
        StagePilot's own timer sees it."""
        self.pb.play(self.pb_id(song_key), position)
        st = await self.wait(lambda: (lambda s: s if s["timer"]["status"] == "running" and s["timer"]["started_at"] else None)(self.state()),
                             15, 0.05, "timer running")
        return iso(st["timer"]["started_at"])

    def song_clock(self):
        t = self.state()["timer"]
        return iso(t["started_at"]) if t.get("started_at") else None

    async def feed(self, take, origin, start=0.0, end=None, until_event=None):
        """Real-time relay: every take event goes out at `origin + at + relay_latency`, its time tag
        = the DMX change moment on the relay's clock. Returns the number of events sent."""
        n = 0
        end = take["duration"] + 1 if end is None else end
        for at, cmds in take["events"]:
            if at < start:
                continue
            if at > end:
                break
            delay = origin + at + self.relay_latency - time.time()
            if delay > 0:
                await asyncio.sleep(delay)
            self.relay.emit(cmds, tag=origin + at + self.relay.skew)
            n += 1
        return n

    async def sleep_until_song(self, origin, position):
        d = origin + position - time.time()
        if d > 0:
            await asyncio.sleep(d)
