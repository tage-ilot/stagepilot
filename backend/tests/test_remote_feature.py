"""Product Remote API, durable retries and independently managed intent."""

from __future__ import annotations

import os
import time
from pathlib import Path
from unittest.mock import patch
from uuid import uuid4

import pytest
from fastapi.testclient import TestClient

from stagepilot.api.remote_ingress import COOKIE_NAME, RemoteIngress
from stagepilot.core.config import Settings
from stagepilot.main import create_app
from stagepilot.remote_feature import RemoteFeature
from stagepilot.remote_files import DesiredRemote, atomic_write, read_desired, safe_status
from stagepilot.services.remote_auth import RemoteAuthError, RemoteRole, RemoteStore, csrf_token

PASSWORD = "a-test-password-only"
ORIGIN = "https://remote.stagepilot.test"


def test_atomic_write_skips_unsupported_directory_fsync_on_windows(
    tmp_path: Path,
) -> None:
    path = tmp_path / "remote.json"

    with (
        patch("stagepilot.remote_files.sys.platform", "win32"),
        patch("stagepilot.remote_files.os.open", wraps=os.open) as open_mock,
    ):
        atomic_write(path, "durable file contents")

    assert path.read_text() == "durable file contents"
    assert not any(call.args[:2] == (tmp_path, os.O_RDONLY) for call in open_mock.call_args_list)


def test_enable_succeeds_with_zero_operators_on_fresh_install(tmp_path: Path) -> None:
    """Regression for the beta.10 gap: a genuinely fresh installation (zero
    Operators configured) must be able to call /enable directly and start
    the tunnel, matching the frontend's already-shipped expectation that
    Operator creation is not a precondition for starting Remote Access.
    Real Viewer/Operator auth still gates every actual Remote request
    regardless of Operator count, so this is safe."""
    store = RemoteStore(tmp_path / "identity.db")
    app = create_app(Settings(), remote_store=store, dashboard_auth_enforced=True)
    feature = RemoteFeature(tmp_path / "export/remote.json")
    app.state.remote_feature = feature
    with TestClient(app) as client:
        assert client.post("/api/v1/dashboard-auth/login", json={"pin": "1234"}).status_code == 200
        assert client.get("/api/v1/remote-access").json()["needs_operator"] is True
        client.headers["X-StagePilot-Remote"] = "1"
        response = client.post("/api/v1/remote-access/enable")
        assert response.status_code == 200
        status = client.get("/api/v1/remote-access").json()
        assert status["needs_operator"] is True


def test_last_enabled_operator_removal_guard_is_unaffected(tmp_path: Path) -> None:
    """The separate, still-valid rule that the last enabled Operator on an
    already-enabled installation cannot be removed/disabled/demoted must
    keep working -- it is unrelated to the enable-time zero-operator gate
    that was removed above."""
    store = RemoteStore(tmp_path / "identity.db")
    user = store.bootstrap("operator@example.test", PASSWORD)
    with pytest.raises(RemoteAuthError, match="last enabled Operator"):
        store.update_user(user["id"], delete=True)
    with pytest.raises(RemoteAuthError, match="last enabled Operator"):
        store.update_user(user["id"], enabled=False)
    with pytest.raises(RemoteAuthError, match="last enabled Operator"):
        store.update_user(user["id"], role=RemoteRole.VIEWER)


def test_product_bootstrap_and_intent(tmp_path: Path) -> None:
    store = RemoteStore(tmp_path / "identity.db")
    app = create_app(Settings(), remote_store=store, dashboard_auth_enforced=True)
    feature = RemoteFeature(tmp_path / "export/remote.json")
    app.state.remote_feature = feature
    with TestClient(app) as client:
        assert (
            client.post(
                "/api/v1/remote-access/bootstrap",
                json={
                    "email": "operator@example.test",
                    "password": PASSWORD,
                },
            ).status_code
            == 401
        )
        assert client.post("/api/v1/dashboard-auth/login", json={"pin": "1234"}).status_code == 200
        assert client.get("/api/v1/remote-access").json()["needs_operator"] is True
        assert client.post("/api/v1/remote-access/enable").status_code == 403
        client.headers["X-StagePilot-Remote"] = "1"
        assert client.post("/api/v1/remote-access/enable").status_code == 200
        body = {"email": "operator@example.test", "password": PASSWORD}
        assert client.post("/api/v1/remote-access/bootstrap", json=body).status_code == 201
        assert client.post("/api/v1/remote-access/bootstrap", json=body).status_code == 409
        assert client.post("/api/v1/remote-access/enable").status_code == 200
        generation = feature.intent().generation
        assert client.post("/api/v1/remote-access/enable").status_code == 200
        assert feature.intent().generation == generation
        safe_status(
            feature.status_path,
            available=True,
            state="connected",
            url=ORIGIN,
            generation=generation,
            checked_at=time.time(),
        )
        status = client.get("/api/v1/remote-access").json()
        assert status["state"] == "connected" and status["url"] == ORIGIN
        assert str(tmp_path) not in str(status)
        assert client.post("/api/v1/remote-access/disable").json()["state"] == "off"
        assert not DesiredRemote.model_validate_json(feature.control.read_text()).enabled
        assert client.get("/api/v1/health/live").status_code == 200


def test_remote_retry_and_viewer_policy(tmp_path: Path) -> None:
    store = RemoteStore(tmp_path / "identity.db")
    store.bootstrap("operator@example.test", PASSWORD)
    store.create_user("viewer@example.test", PASSWORD, RemoteRole.VIEWER)
    app = create_app(Settings(), remote_store=store)
    app.state.remote_feature = RemoteFeature(tmp_path / "remote.json")
    with (
        TestClient(app),
        TestClient(RemoteIngress(app, public_origin=ORIGIN), base_url=ORIGIN) as client,
    ):
        client.headers.update({"Origin": ORIGIN, "X-StagePilot-Remote": "1"})
        token = store.login("viewer@example.test", PASSWORD)
        client.cookies.set(COOKIE_NAME, token)
        client.headers["X-CSRF-Token"] = csrf_token(token)
        assert client.get("/api/v1/remote-access").status_code == 403
        assert (
            client.post(
                "/api/v1/remote-access/bootstrap",
                json={
                    "email": "other@example.test",
                    "password": PASSWORD,
                },
            ).status_code
            == 403
        )
        token = store.login("operator@example.test", PASSWORD)
        client.cookies.set(COOKIE_NAME, token)
        client.headers["X-CSRF-Token"] = csrf_token(token)
        assert (
            client.post(
                "/api/v1/remote-access/bootstrap",
                json={
                    "email": "other@example.test",
                    "password": PASSWORD,
                },
            ).status_code
            == 403
        )
        client.headers["Idempotency-Key"] = str(uuid4())
        first = client.post("/api/v1/actions/start_next")
        second = client.post("/api/v1/actions/start_next")
        assert first.status_code == second.status_code == 200
        assert first.content == second.content
        assert client.post("/api/v1/actions/stop_timer").status_code == 409
        store.revoke(token)
        assert client.post("/api/v1/actions/start_next").status_code == 401


def test_journal_restart_uncertainty_and_conflict(tmp_path: Path) -> None:
    path = tmp_path / "identity.db"
    store = RemoteStore(path)
    assert store.claim_action("key", "fingerprint", time.time() + 60) is None
    restarted = RemoteStore(path)
    with pytest.raises(RemoteAuthError, match="pending or uncertain"):
        restarted.claim_action("key", "fingerprint", time.time() + 60)
    with pytest.raises(RemoteAuthError, match="another action"):
        restarted.claim_action("key", "other", time.time() + 60)
    restarted.finish_action("key", 200, b"{}")
    assert store.claim_action("key", "fingerprint", time.time() + 60) == (200, b"{}")


def test_stale_status_and_foreign_route(tmp_path: Path) -> None:
    feature = RemoteFeature(tmp_path / "remote.json")
    feature.set_enabled(True)
    safe_status(
        feature.status_path,
        available=True,
        state="connected",
        url=ORIGIN,
        generation=feature.intent().generation,
        checked_at=time.time() - 100,
    )
    assert feature.status()["url"] is None
    feature.set_enabled(False)
    atomic_write(
        feature.control,
        DesiredRemote(
            enabled=True, generation=str(uuid4()), public_origin=ORIGIN
        ).model_dump_json(),
    )
    with pytest.raises(ValueError):
        feature.set_enabled(True)


def test_named_marker_survives_ui_disable_and_reports_stable_url(tmp_path: Path) -> None:
    feature = RemoteFeature(tmp_path / "remote.json")
    marker = DesiredRemote(public_origin=ORIGIN, port=18766)
    atomic_write(feature.control, marker.model_dump_json())
    feature.set_enabled(True)
    generation = feature.intent().generation
    safe_status(
        feature.status_path,
        available=True,
        state="connected",
        url=ORIGIN,
        generation=generation,
        temporary_url=False,
        checked_at=time.time(),
    )
    assert feature.status()["url"] == ORIGIN
    assert feature.status()["temporary_url"] is False
    feature.set_enabled(False)
    disabled = read_desired(feature.control)
    assert not disabled.enabled
    assert disabled.public_origin == ORIGIN
    assert disabled.port == 18766


def test_hostname_conflict_maps_to_recovery_text_without_infrastructure_names() -> None:
    import httpx

    from stagepilot.api.remote_feature import _enable_error_message
    from stagepilot.remote_beta_control import _worker_diagnostic
    from stagepilot.remote_provider import ProviderError

    response = httpx.Response(
        503,
        json={
            "error": "operation incomplete; retry reconciliation",
            "diagnostic": "hostname ownership conflict",
        },
    )
    assert _worker_diagnostic(response) == "hostname ownership conflict"
    assert _worker_diagnostic(httpx.Response(503, text="<html>")) is None
    assert _worker_diagnostic(httpx.Response(503, json={"diagnostic": 5})) is None

    message = _enable_error_message(
        ProviderError("Hostname ownership conflict; reset may be required")
    )
    assert message == (
        "Your previous Remote setup is still registered. Try again in a minute; "
        "if it keeps happening, reset Remote Access."
    )
    for word in ("tunnel", "cloudflare", "hostname", "dns"):
        assert word not in message.lower()
    assert "could not be enabled" in _enable_error_message(ProviderError("other"))
