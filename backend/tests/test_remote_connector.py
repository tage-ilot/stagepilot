from __future__ import annotations

import json
import socket
import sys
import time
from pathlib import Path
from uuid import uuid4

import pytest

from stagepilot.remote_connector import Connector
from stagepilot.remote_files import DesiredRemote, atomic_write
from test_remote_server import unused_port

# These tests spawn a substitute connector written as a `#!/usr/bin/python3`
# script and made executable with `chmod(0o700)`. Windows has no shebang
# support and no POSIX execute bit, so the child can never start there and the
# test would assert on the harness rather than on StagePilot. The product code
# under test is platform-independent and is covered on Windows by the packaged
# Remote lifecycle job in ci.yml.
posix_child_process = pytest.mark.skipif(
    sys.platform == "win32",
    reason="substitute connector relies on a POSIX shebang and execute bit",
)


def connector_fixture(tmp_path: Path) -> tuple[Connector, DesiredRemote]:
    binary = tmp_path / "fake-connector"
    # A real child process/health socket, explicitly NOT a Cloudflare connection.
    binary.write_text(
        "#!/usr/bin/python3\n"
        "import sys\n"
        "from http.server import BaseHTTPRequestHandler, HTTPServer\n"
        "from pathlib import Path\n"
        "if '--token' in sys.argv:\n"
        " assert sys.argv[sys.argv.index('--token')+1] == 'test-run-token'\n"
        "else:\n"
        " assert Path(sys.argv[sys.argv.index('--token-file')+1]).read_text() == 'test-run-token'\n"
        "class Handler(BaseHTTPRequestHandler):\n"
        " def do_GET(self):\n"
        "  self.send_response(200); self.end_headers()\n"
        " def log_message(self, *args): pass\n"
        "port=int(sys.argv[sys.argv.index('--metrics')+1].split(':')[-1])\n"
        "HTTPServer(('127.0.0.1',port), Handler).serve_forever()\n"
    )
    binary.chmod(0o700)
    desired = DesiredRemote(
        enabled=True,
        generation=str(uuid4()),
        public_origin="https://remote.test",
        port=unused_port(),
    )
    path = tmp_path / "remote.json"
    atomic_write(path, desired.model_dump_json())
    atomic_write(tmp_path / "connector.token", "test-run-token")
    return Connector(binary, path, tmp_path / "status.json", unused_port()), desired


def wait_connected(connector: Connector) -> None:
    # A real clock deadline, not a loop count: after a deliberate crash the connector waits out its
    # 1-2 second restart backoff, and a loaded CI runner (macOS Intel) needs more than 5 seconds.
    deadline = time.monotonic() + 30
    while True:
        connector.step()
        if json.loads(connector.status.read_text())["state"] == "connected":
            return
        if time.monotonic() > deadline:
            raise AssertionError("Local substitute connector did not recover")
        time.sleep(0.05)


@posix_child_process
def test_child_crash_restart_disable_reenable_and_supervisor_restart(tmp_path: Path) -> None:
    connector, desired = connector_fixture(tmp_path)
    try:
        wait_connected(connector)
        assert connector.process is not None
        first = connector.process.pid
        connector.process.kill()
        connector.process.wait(timeout=5)
        wait_connected(connector)
        assert connector.process is not None and connector.process.pid != first
        atomic_write(connector.control, DesiredRemote().model_dump_json())
        connector.step()
        assert connector.process is None
        assert json.loads(connector.status.read_text())["state"] == "disabled"
        desired.generation = str(uuid4())
        atomic_write(connector.control, desired.model_dump_json())
        wait_connected(connector)
        connector.stop()
        restarted = Connector(
            connector.binary, connector.control, connector.status, connector.metrics_port
        )
        try:
            wait_connected(restarted)
            atomic_write(connector.control, "corrupt")
            restarted.step()
            assert restarted.process is None
        finally:
            restarted.stop()
        assert "test-run-token" not in connector.status.read_text()
    finally:
        connector.stop()


@posix_child_process
def test_missing_insecure_token_and_metrics_conflict_fail_closed(tmp_path: Path) -> None:
    connector, _ = connector_fixture(tmp_path)
    token = connector.control.with_name("connector.token")
    try:
        token.chmod(0o644)
        connector.step()
        assert connector.process is None
        token.chmod(0o600)
        connector.retry_at = 0
        with socket.create_server(("127.0.0.1", connector.metrics_port)):
            connector.step()
            assert connector.process is None
            assert json.loads(connector.status.read_text())["state"] == "reconnecting"
        connector.retry_at = 0
        wait_connected(connector)
    finally:
        connector.stop()


@posix_child_process
def test_desktop_token_is_passed_directly_without_creating_a_file(tmp_path: Path) -> None:
    connector, _ = connector_fixture(tmp_path)
    token = connector.control.with_name("connector.token")
    token.unlink()
    connector.token_provider = lambda: "test-run-token"
    try:
        wait_connected(connector)
        assert not token.exists()
        assert connector.process is not None
        arguments = connector.process.args
        assert isinstance(arguments, list)
        assert "--token" in arguments
        assert "--token-file" not in arguments
        connector.process.kill()
        connector.process.wait(timeout=5)
        wait_connected(connector)
        assert not token.exists()
    finally:
        connector.stop()
    assert not token.exists()
