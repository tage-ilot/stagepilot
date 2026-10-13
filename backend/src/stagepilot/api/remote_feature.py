"""Authorized product-facing Remote controls; no infrastructure in responses."""

from __future__ import annotations

from typing import Any
from uuid import uuid4

from fastapi import APIRouter, HTTPException, Request

from stagepilot.api.remote_auth import LoginRequest, RemoteRoute, _admin
from stagepilot.api.remote_ingress import remote_context
from stagepilot.remote_feature import RemoteFeature
from stagepilot.remote_provider import InstallationPermanentlyRevokedError, ProviderError
from stagepilot.services.remote_auth import RemoteRole

router = APIRouter(prefix="/api/v1/remote-access", route_class=RemoteRoute)


def manager(request: Request) -> Any | None:
    return getattr(request.app.state, "remote_manager", None)


def feature(request: Request) -> RemoteFeature:
    value = getattr(request.app.state, "remote_feature", None)
    if not isinstance(value, RemoteFeature):
        raise HTTPException(503, "Remote Access is unavailable on this installation.")
    return value


def mutation(request: Request) -> None:
    if request.headers.get("x-stagepilot-remote") != "1":
        raise HTTPException(403, "Remote settings request required.")


@router.get("")
async def status(request: Request) -> dict[str, Any]:
    access = _admin(request)
    value: Any = manager(request) or getattr(request.app.state, "remote_feature", None)
    result = (
        await access.call(value.status)
        if value is not None and callable(getattr(value, "status", None))
        else {
            "available": False,
            "enabled": False,
            "state": "off",
            "url": None,
            "message": "Remote Access is unavailable on this installation.",
            "temporary_url": True,
            "permanently_revoked": False,
        }
    )
    result.setdefault("provisioned", True)
    result.setdefault("credential_available", True)
    result.setdefault("permanently_revoked", False)
    result["needs_operator"] = not any(
        u["enabled"] and u["role"] == RemoteRole.OPERATOR
        for u in await access.call(access.store.users)
    )
    return result


@router.post("/bootstrap", status_code=201)
async def bootstrap(body: LoginRequest, request: Request) -> dict[str, Any]:
    access = _admin(request)
    mutation(request)
    if remote_context(request.scope) is not None:
        raise HTTPException(403, "Create the first Operator from local StagePilot.")
    return await access.call(access.store.bootstrap, body.email, body.password.get_secret_value())


@router.post("/enable")
async def enable(request: Request) -> dict[str, Any]:
    access = _admin(request)
    mutation(request)
    managed = manager(request)
    value: Any = managed or feature(request)
    try:
        if managed is not None:
            await access.call(value.enable)
        else:
            await access.call(value.set_enabled, True)
    except InstallationPermanentlyRevokedError as exc:
        raise HTTPException(503, _enable_error_message(exc)) from exc
    except ProviderError as exc:
        raise HTTPException(503, _enable_error_message(exc)) from exc
    except ValueError as exc:
        raise HTTPException(409, "Remote Access is not provisioned or is already managed.") from exc
    return await status(request)


@router.post("/reset")
async def reset(request: Request) -> dict[str, Any]:
    """Explicit, visible recovery once `enable` has confirmed this
    installation's credential is permanently (not transiently) revoked --
    see `status().permanently_revoked` and `DesktopRemoteManager.reset_identity`.
    Discards the dead local identity so the next `enable` mints a fresh one;
    never runs silently/automatically."""

    access = _admin(request)
    mutation(request)
    value = manager(request)
    if value is None or not callable(getattr(value, "reset_identity", None)):
        raise HTTPException(
            503, "Resetting the installation identity is unavailable on this installation."
        )
    await access.call(value.reset_identity)
    return await status(request)


def _enable_error_message(exc: ProviderError) -> str:
    """Map a sanitized ProviderError to a short, specific, end-user-facing
    message. ProviderError text never contains secrets/tokens/stack traces
    (see remote_provider.py), but we still map to fixed strings here rather
    than passing the exception text straight through, so wording stays
    stable and end-user-appropriate regardless of the exact internal reason.
    """

    text = str(exc)
    if "enrollment limit" in text:
        return "This computer has reached its enrollment limit for now. Try again later."
    if "Could not reach" in text or ("unavailable" in text and "Internet" in text):
        return (
            "Could not reach the enrollment service. Check your Internet connection and try again."
        )
    if "credential is unavailable" in text or "credential was revoked" in text:
        return (
            "Your previous Remote Access setup could not be restored and needs to be "
            "reset. This is safe and won't affect local StagePilot."
        )
    if "Hostname ownership conflict" in text:
        return (
            "Your previous Remote setup is still registered. Try again in a minute; "
            "if it keeps happening, reset Remote Access."
        )
    if "not trusted" in text:
        return "Remote Access is unavailable on this installation."
    return (
        "Remote Access could not be enabled right now; local StagePilot is unaffected. "
        "Try again shortly."
    )


@router.post("/disable")
async def disable(request: Request) -> dict[str, Any]:
    access = _admin(request)
    mutation(request)
    value = manager(request)
    try:
        if value is not None:
            await access.call(value.disable)
        else:
            await access.call(feature(request).set_enabled, False)
    except ProviderError as exc:
        raise HTTPException(
            503,
            "Remote public access is closed locally; provider revocation will retry on restart.",
        ) from exc
    await access.call(access.store.installation_generation, str(uuid4()))
    return await status(request)


@router.post("/regenerate")
async def regenerate(request: Request) -> dict[str, Any]:
    access = _admin(request)
    mutation(request)
    value = manager(request)
    if value is None or not callable(getattr(value, "regenerate", None)):
        raise HTTPException(
            503, "Regenerating the Remote link is unavailable on this installation."
        )
    try:
        await access.call(value.regenerate)
    except ProviderError as exc:
        raise HTTPException(
            503,
            "Remote link regeneration is unavailable; local StagePilot is unaffected.",
        ) from exc
    return await status(request)
