"""Installation-side client for the authenticated private-beta control plane."""

from __future__ import annotations

import argparse
import json
import random
import time
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Literal
from uuid import uuid4

import httpx
from pydantic import BaseModel, Field

from stagepilot.file_lock import exclusive_lock
from stagepilot.remote_bootstrap import sanitize_device_name
from stagepilot.remote_files import (
    BetaControlConfig,
    DesiredRemote,
    atomic_write,
    is_group_or_world_readable,
)
from stagepilot.remote_provider import ProviderError


def _worker_diagnostic(response: httpx.Response) -> str | None:
    """Return the Worker's fixed `diagnostic` string, if present (never raw bodies)."""

    try:
        body = response.json()
    except ValueError:
        return None
    value = body.get("diagnostic") if isinstance(body, dict) else None
    return value if isinstance(value, str) else None


class InstallationRevokedError(ProviderError):
    """The installation capability is no longer accepted by the control plane."""


class BetaProvisionState(BaseModel):
    phase: Literal["disabled", "enabling", "enabled", "revoking"] = "disabled"
    generation: str = ""
    binding: dict[str, str | int] = Field(default_factory=dict)
    retire: bool = False


@contextmanager
def _exclusive_lock(path: Path) -> Iterator[None]:
    """Hold a one-byte process lock on Unix and Windows."""

    with exclusive_lock(path):
        yield


class BetaRemoteControl:
    def __init__(
        self,
        config: BetaControlConfig,
        client: httpx.Client,
        *,
        credential_provider: Callable[[], str] | None = None,
        sleep: Callable[[float], None] = time.sleep,
        random_value: Callable[[], float] = random.random,
        action_handlers: dict[str, Callable[[], None]] | None = None,
        device_name: str | None = None,
    ) -> None:
        self.config = config
        self.client = client
        self.credential_provider = credential_provider
        self.sleep = sleep
        self.random_value = random_value
        # Handlers for operator-queued pending actions (see apply_pending_actions).
        self.action_handlers = action_handlers or {}
        self.connector_token: str | None = None
        self.state_path = config.state_dir / "state.json"
        self.desired_path = config.installation_dir / "remote.json"
        # Cleanup-only compatibility path for tokens written by older beta builds.
        self.token_path = config.installation_dir / "connector.token"
        # Self-reported, untrusted, display-only device hostname (see
        # remote_bootstrap.sanitize_device_name); included on status/
        # provision/reconcile request bodies below.
        self.device_name = sanitize_device_name(device_name)

    def apply(self, action: str) -> dict[str, object]:
        self.config.state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        with _exclusive_lock(self.config.state_dir / "control.lock"):
            return self._apply(action)

    def _apply(self, action: str) -> dict[str, object]:
        if action != "status":
            self.connector_token = None
        try:
            state = (
                BetaProvisionState.model_validate_json(self.state_path.read_text())
                if self.state_path.exists()
                else BetaProvisionState()
            )
        except (OSError, ValueError):
            if action == "disable":
                self._write_disabled_marker()
                self.token_path.unlink(missing_ok=True)
            raise
        binding: dict[str, str | int] = {
            "control_plane": self.config.control_plane_url.rstrip("/"),
            "installation": self.config.installation_id,
            "hostname": self.config.hostname,
            "port": self.config.remote_port,
            "export": str(self.config.installation_dir),
        }
        if state.binding and state.binding != binding:
            raise ProviderError("Configuration changed; preserve the original beta control state")
        state.binding = binding
        if action == "status":
            return self._status(state)
        if action not in {"enable", "disable", "revoke", "reconcile"}:
            raise ValueError("Unknown Remote operation")

        if action == "enable":
            if state.phase == "revoking":
                raise ProviderError("Finish retrying disable before re-enabling")
            if state.phase == "disabled":
                state.generation = str(uuid4())
                state.phase = "enabling"
                state.retire = False
                self._write_disabled_marker()
                atomic_write(self.state_path, state.model_dump_json())
            return self._provision(state)

        if action in {"disable", "revoke"} or state.phase == "revoking":
            return self._disable(state, retire=action == "revoke" or state.retire)
        if state.phase == "enabling":
            return self._provision(state)
        if action == "reconcile" and state.phase == "enabled":
            return self._provision(state, operation="reconcile")
        return self._status(state)

    def _provision(
        self, state: BetaProvisionState, *, operation: str = "provision"
    ) -> dict[str, object]:
        if not state.generation:
            raise ProviderError("Missing installation generation")
        response = self._request(
            "POST",
            f"/v1/installations/{self.config.installation_id}/{operation}",
            {"generation": state.generation},
        )
        if (
            response.get("installationId") != self.config.installation_id
            or response.get("hostname") != self.config.hostname
            or response.get("generation") != state.generation
            or response.get("phase") != "provisioned"
        ):
            raise ProviderError("Control-plane identity response did not match")
        token = response.get("tunnelToken")
        if (
            not isinstance(token, str)
            or len(token) < 20
            or any(character.isspace() for character in token)
        ):
            raise ProviderError("Installation tunnel credential unavailable")
        atomic_write(
            self.desired_path,
            DesiredRemote(
                enabled=True,
                generation=state.generation,
                public_origin=f"https://{self.config.hostname}",
                port=self.config.remote_port,
            ).model_dump_json(),
        )
        state.phase = "enabled"
        atomic_write(self.state_path, state.model_dump_json())
        self.connector_token = token
        return self._status(state)

    def _disable(self, state: BetaProvisionState, *, retire: bool) -> dict[str, object]:
        if state.phase != "disabled" or retire:
            state.phase = "revoking"
        state.retire = retire
        atomic_write(self.state_path, state.model_dump_json())
        self._write_disabled_marker()
        self.token_path.unlink(missing_ok=True)
        if state.generation or retire:
            operation = "revoke" if retire else "disable"
            response = self._request(
                "POST",
                f"/v1/installations/{self.config.installation_id}/{operation}",
                {},
            )
            if (
                response.get("installationId") != self.config.installation_id
                or response.get("hostname") != self.config.hostname
                or response.get("phase") != "disabled"
                or (retire and response.get("revoked") is not True)
            ):
                raise ProviderError("Control-plane revocation response did not match")
        state.phase = "disabled"
        state.generation = ""
        atomic_write(self.state_path, state.model_dump_json())
        return self._status(state)

    def apply_pending_actions(self) -> list[str]:
        """Apply operator-queued actions surfaced by the control plane's status route.

        The control plane can never dial this installation, so an admin action
        (e.g. resetting this installation's local login rate limit) is queued
        Worker-side and delivered on the installation's own authenticated
        status check -- the same poll/desired-state shape already used for
        enable/disable. Each action is applied locally and only then
        acknowledged, so an action survives a crash mid-apply and is retried;
        handlers must therefore be idempotent.
        """

        response = self._request(
            "GET", f"/v1/installations/{self.config.installation_id}/status", {}
        )
        actions = response.get("pendingActions")
        if not isinstance(actions, list):
            return []
        applied: list[str] = []
        for action in actions:
            if not isinstance(action, dict):
                continue
            kind = action.get("kind")
            action_id = action.get("actionId")
            if (
                not isinstance(kind, str)
                or not isinstance(action_id, str)
                # Never act on an action attributed to a different installation.
                or action.get("installationId") != self.config.installation_id
            ):
                continue
            handler = self.action_handlers.get(kind)
            if handler is None:
                continue
            handler()
            self._request(
                "POST",
                f"/v1/installations/{self.config.installation_id}/pending-actions/{action_id}/ack",
                {},
            )
            applied.append(kind)
        return applied

    def lightweight_status(self) -> dict[str, object]:
        """Minimal existence/deviceName check-in, no provisioning side effects.

        Deliberately calls only the existing authenticated GET status route
        (never provision/reconcile) so it can safely run unconditionally for
        every installation regardless of the Remote Access toggle.
        """

        return self._request("GET", f"/v1/installations/{self.config.installation_id}/status", {})

    def request_approval(self, reason: str) -> dict[str, object]:
        """Ask the operator to sign off on something; returns the pending request."""

        return self._request(
            "POST",
            f"/v1/installations/{self.config.installation_id}/approval-requests",
            {"reason": reason},
        )

    def approval_requests(self) -> list[dict[str, object]]:
        """Poll this installation's own approval requests for operator decisions."""

        response = self._request(
            "GET", f"/v1/installations/{self.config.installation_id}/approval-requests", {}
        )
        requests = response.get("requests")
        if not isinstance(requests, list):
            return []
        return [item for item in requests if isinstance(item, dict)]

    def _write_disabled_marker(self) -> None:
        atomic_write(
            self.desired_path,
            DesiredRemote(
                public_origin=f"https://{self.config.hostname}",
                port=self.config.remote_port,
            ).model_dump_json(),
        )

    def _credential(self) -> str:
        if self.credential_provider is not None:
            token = self.credential_provider().strip()
            expected_prefix = f"spi_{self.config.installation_id}."
            if not token.startswith(expected_prefix) or any(
                character.isspace() for character in token
            ):
                raise ProviderError("Installation credential is invalid")
            return token
        if self.config.credential_file is None:
            raise ProviderError("Installation credential unavailable")
        try:
            if self.config.credential_file.is_symlink():
                raise ProviderError("Installation credential file must not be a symlink")
            if is_group_or_world_readable(self.config.credential_file):
                raise ProviderError("Installation credential file must be private (0600)")
            token = self.config.credential_file.read_text().strip()
        except OSError as exc:
            raise ProviderError("Installation credential unavailable") from exc
        expected_prefix = f"spi_{self.config.installation_id}."
        if not token.startswith(expected_prefix) or any(character.isspace() for character in token):
            raise ProviderError("Installation credential is invalid")
        return token

    def _request(self, method: str, path: str, payload: dict[str, object]) -> dict[str, object]:
        params: dict[str, str] = {}
        if self.device_name is not None and path.endswith(("/status", "/provision", "/reconcile")):
            if method == "GET":
                # GET requests on the control plane carry deviceName as a
                # query parameter, not a JSON body: the Worker side treats
                # GET /status as a normal body-less status poll for the
                # common case, matching the standard Fetch API's
                # restriction that GET requests cannot have a body.
                params["deviceName"] = self.device_name
            else:
                payload = {**payload, "deviceName": self.device_name}
        try:
            response: httpx.Response | None = None
            for attempt in range(3):
                response = self.client.request(
                    method,
                    path,
                    headers={"Authorization": f"Bearer {self._credential()}"},
                    params=params or None,
                    json=payload,
                )
                if response.status_code not in {429, 503} or attempt == 2:
                    break
                try:
                    retry_after = max(0, min(300, int(response.headers.get("Retry-After", "0"))))
                except ValueError:
                    retry_after = 0
                self.sleep(
                    max(
                        float(retry_after),
                        min(0.5 * (2**attempt) + self.random_value() * 0.25, 5.0),
                    )
                )
            assert response is not None
            if response.status_code in {401, 403}:
                raise InstallationRevokedError("Installation enrollment is expired or revoked")
            if not response.is_success:
                if _worker_diagnostic(response) == "hostname ownership conflict":
                    raise ProviderError("Hostname ownership conflict; reset may be required")
                raise ProviderError(f"Beta control request failed (HTTP {response.status_code})")
            value = response.json()
            if not isinstance(value, dict):
                raise ProviderError("Beta control response was invalid")
            return value
        except (httpx.HTTPError, ValueError) as exc:
            raise ProviderError("Beta control plane unavailable; retry reconciliation") from exc

    def _status(self, state: BetaProvisionState) -> dict[str, object]:
        return {
            "phase": state.phase,
            "installation": self.config.installation_id,
            "hostname": self.config.hostname,
            "generation": state.generation,
        }


def run() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["enable", "disable", "revoke", "reconcile", "status"])
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args()
    try:
        config = BetaControlConfig.model_validate_json(args.config.read_text())
        with httpx.Client(
            base_url=config.control_plane_url,
            timeout=20,
            trust_env=False,
            follow_redirects=False,
        ) as client:
            result = BetaRemoteControl(config, client).apply(args.action)
        print(json.dumps(result))
    except (OSError, ValueError, ProviderError):
        parser.exit(1, "Remote operation incomplete; check private configuration and retry.\n")


if __name__ == "__main__":
    run()
