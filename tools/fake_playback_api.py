#!/usr/bin/env python3
"""A stand-in for Playback's own connection (the Playback API), for looking at StagePilot's Playback Live Link in a browser.

It is the scriptable fake the contract replay uses (`scripts/contract/playback_fake.py`), kept alive: it listens like
Playback's remote connection and sends a heartbeat every second, so StagePilot says "Connected to Playback at 127.0.0.1".
Standard library only; it binds to 127.0.0.1 and never writes anything.

    python3 tools/fake_playback_api.py              # port 8080, where StagePilot looks by default
    python3 tools/fake_playback_api.py --port 18080 # then type 127.0.0.1 and set the port in the Playback API settings
"""

from __future__ import annotations

import argparse
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "scripts" / "contract"))
from playback_fake import FakePlayback  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser(description="A stand-in for Playback's remote connection (see the top of this file).")
    parser.add_argument("--port", type=int, default=8080)
    args = parser.parse_args()
    fake = FakePlayback()
    fake.start(args.port)
    print(f"fake Playback API on 127.0.0.1:{fake.port} (Ctrl-C to stop)", flush=True)
    try:
        while True:
            fake.tick(1)
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        fake.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
