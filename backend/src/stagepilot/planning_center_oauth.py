"""Planning Center OAuth 2.0 sign-in: PKCE, token exchange, storage, refresh.

The client_secret never reaches this machine: every call that needs it is
relayed through the StagePilot control-plane Worker
(`/v1/planning-center/oauth/flow|token|refresh`, added in phase 1), which
holds the secret server-side.

The transient-vs-permanent distinction here is deliberate and mirrors the
Remote Access credential work (t_7bdec765 / t_460cf8dd): a network blip,
timeout, 429 or 5xx is TRANSIENT and must never flip the connection into
"sign in again" -- only Planning Center itself rejecting the grant
(`invalid_grant`, i.e. the refresh token was revoked or expired) is
PERMANENT and sets the explicit `needs_reconnect` state.
"""

from __future__ import annotations

import asyncio
import base64
import hashlib
import json
import logging
import secrets
import time
from dataclasses import dataclass
from typing import Literal
from urllib.parse import urlencode

import httpx
from pydantic import BaseModel, ConfigDict, Field, ValidationError

from stagepilot.core.settings import CredentialStore, CredentialStoreError

logger = logging.getLogger(__name__)

AUTHORIZE_ENDPOINT = "https://api.planningcenteronline.com/oauth/authorize"
# StagePilot only ever calls Planning Center Services endpoints
# (`services/v2/...` in plugins/planning_center/client.py), so only the
# `services` scope is requested -- never a broader product scope.
OAUTH_SCOPE = "services"
DEFAULT_CONTROL_PLANE_ORIGIN = (
    "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev"
)
TRUSTED_CONTROL_PLANE_ORIGINS = frozenset({DEFAULT_CONTROL_PLANE_ORIGIN})

# Planning Center access tokens live for 2 hours; refresh 15 minutes early
# so a slow/flaky refresh still has room to retry before anything expires.
REFRESH_MARGIN_SECONDS = 15 * 60
REFRESH_POLL_SECONDS = 60.0
# A sign-in attempt that never produced a callback is abandoned after the
# desktop listener's own ~5 minute timeout, plus slack.
FLOW_TTL_SECONDS = 420.0


class PlanningCenterOAuthError(RuntimeError):
    """A sign-in or refresh attempt failed.

    `permanent` distinguishes "this credential is dead, the user must sign
    in again" from "this may well work on the next attempt".
    """

    def __init__(self, message: str, *, permanent: bool = False) -> None:
        super().__init__(message)
        self.permanent = permanent


class OAuthTokens(BaseModel):
    """The stored OAuth token blob (keyring-backed, never settings.json)."""

    model_config = ConfigDict(extra="ignore")

    access_token: str = Field(min_length=1)
    refresh_token: str = Field(min_length=1)
    expires_at: float
    scope: str = OAUTH_SCOPE
    needs_reconnect: bool = False

    def expires_in(self, *, now: float | None = None) -> float:
        return self.expires_at - (time.time() if now is None else now)

    def needs_refresh(self, *, now: float | None = None) -> bool:
        return self.expires_in(now=now) <= REFRESH_MARGIN_SECONDS


@dataclass(frozen=True, slots=True)
class PendingFlow:
    """One in-progress sign-in attempt, held in memory only."""

    state: str
    code_verifier: str
    flow_id: str
    ticket: str
    started_at: float

    def expired(self, *, now: float | None = None) -> bool:
        return (time.time() if now is None else now) - self.started_at > FLOW_TTL_SECONDS


class OAuthConnectionStatus(BaseModel):
    connection_method: Literal["oauth", "manual"]
    connected: bool
    needs_reconnect: bool
    expires_at: float | None = None
    scope: str | None = None


def generate_code_verifier() -> str:
    """RFC 7636 code verifier: 43-128 chars of unreserved characters."""

    return base64.urlsafe_b64encode(secrets.token_bytes(64)).decode("ascii").rstrip("=")


def code_challenge_for(verifier: str) -> str:
    """S256 challenge. Plain challenges are never used."""

    digest = hashlib.sha256(verifier.encode("ascii")).digest()
    return base64.urlsafe_b64encode(digest).decode("ascii").rstrip("=")


def build_state(flow_id: str) -> str:
    """Bind the control-plane flow id into the OAuth `state` value.

    The Worker mints `{flow_id, ticket}`; embedding `flow_id` in `state`
    means the callback itself proves which flow it belongs to, and the
    random suffix keeps `state` unguessable for CSRF purposes.
    """

    return f"{flow_id}.{secrets.token_urlsafe(24)}"


def flow_id_from_state(state: str) -> str | None:
    head, separator, tail = state.partition(".")
    if not separator or not head or not tail:
        return None
    return head


def build_authorize_url(client_id: str, state: str, code_challenge: str) -> str:
    """The authorize URL WITHOUT `redirect_uri`.

    The desktop side appends `redirect_uri` because only it knows which of
    the four pre-registered loopback ports was actually free.
    """

    query = urlencode(
        {
            "client_id": client_id,
            "response_type": "code",
            "scope": OAUTH_SCOPE,
            "state": state,
            "code_challenge": code_challenge,
            "code_challenge_method": "S256",
        }
    )
    return f"{AUTHORIZE_ENDPOINT}?{query}"


class OAuthTokenStore:
    """Persist the OAuth token blob through the existing CredentialStore.

    This is deliberately the same keyring abstraction the Planning Center
    PAT secret already uses (a second account, `planning-center-oauth`),
    not a new storage mechanism: the blob is JSON so the access token,
    refresh token, expiry and reconnect marker travel together atomically.
    """

    def __init__(self, credentials: CredentialStore) -> None:
        self._credentials = credentials

    def load(self) -> OAuthTokens | None:
        raw = self._credentials.get_secret()
        if not raw:
            return None
        try:
            return OAuthTokens.model_validate_json(raw)
        except ValidationError:
            # A corrupt blob is treated as "disconnected" rather than
            # crashing startup; the user can simply sign in again.
            logger.warning("planning_center_oauth_tokens_unreadable")
            return None

    def save(self, tokens: OAuthTokens) -> None:
        self._credentials.set_secret(tokens.model_dump_json())

    def clear(self) -> None:
        self._credentials.remove_secret()

    def mark_needs_reconnect(self) -> None:
        current = self.load()
        if current is None or current.needs_reconnect:
            return
        self.save(current.model_copy(update={"needs_reconnect": True}))


class ControlPlaneOAuthClient:
    """Call the phase-1 Worker routes that hold the Planning Center secret."""

    def __init__(
        self,
        *,
        origin: str = DEFAULT_CONTROL_PLANE_ORIGIN,
        transport: httpx.AsyncBaseTransport | None = None,
        timeout: float = 20.0,
    ) -> None:
        if origin not in TRUSTED_CONTROL_PLANE_ORIGINS:
            raise PlanningCenterOAuthError("The Planning Center sign-in service is not trusted.")
        self._origin = origin
        self._transport = transport
        self._timeout = timeout

    async def _post(self, path: str, payload: dict[str, str]) -> dict[str, object]:
        try:
            async with httpx.AsyncClient(
                base_url=self._origin,
                timeout=self._timeout,
                trust_env=False,
                follow_redirects=False,
                transport=self._transport,
            ) as client:
                response = await client.post(path, json=payload)
        except httpx.HTTPError as exc:
            # Transient by construction: we never reached the service.
            raise PlanningCenterOAuthError(
                "Could not reach the Planning Center sign-in service. Check your Internet "
                "connection and try again."
            ) from exc
        if response.status_code in {200, 201}:
            try:
                body = response.json()
            except ValueError as exc:
                raise PlanningCenterOAuthError(
                    "The Planning Center sign-in service returned an invalid response."
                ) from exc
            if not isinstance(body, dict):
                raise PlanningCenterOAuthError(
                    "The Planning Center sign-in service returned an invalid response."
                )
            return body
        raise self._failure(response)

    @staticmethod
    def _failure(response: httpx.Response) -> PlanningCenterOAuthError:
        error = ""
        try:
            body = response.json()
        except ValueError:
            body = None
        if isinstance(body, dict):
            value = body.get("error")
            if isinstance(value, str):
                error = value
        # Only Planning Center rejecting the grant itself is permanent. A
        # 429/5xx (rate limit, Worker hiccup, PCO outage) is transient and
        # must not strand a still-valid connection in "sign in again".
        if error in {"invalid_grant", "unauthorized_client", "invalid_client"}:
            return PlanningCenterOAuthError(
                "Your Planning Center connection has expired. Sign in again.",
                permanent=True,
            )
        if response.status_code == 429:
            return PlanningCenterOAuthError(
                "Planning Center sign-in is busy right now. Try again in a moment."
            )
        return PlanningCenterOAuthError(
            "Planning Center sign-in could not be completed. Try again."
        )

    async def start_flow(self) -> tuple[str, str]:
        body = await self._post("/v1/planning-center/oauth/flow", {})
        flow_id = body.get("flow_id")
        ticket = body.get("ticket")
        if not isinstance(flow_id, str) or not isinstance(ticket, str) or not flow_id or not ticket:
            raise PlanningCenterOAuthError(
                "The Planning Center sign-in service returned an invalid response."
            )
        return flow_id, ticket

    async def exchange_code(
        self,
        *,
        flow: PendingFlow,
        code: str,
        redirect_uri: str,
    ) -> OAuthTokens:
        body = await self._post(
            "/v1/planning-center/oauth/token",
            {
                "flow_id": flow.flow_id,
                "ticket": flow.ticket,
                "code": code,
                "code_verifier": flow.code_verifier,
                "redirect_uri": redirect_uri,
            },
        )
        return _tokens_from_response(body)

    async def refresh(self, *, flow_id: str, ticket: str, refresh_token: str) -> OAuthTokens:
        body = await self._post(
            "/v1/planning-center/oauth/refresh",
            {"flow_id": flow_id, "ticket": ticket, "refresh_token": refresh_token},
        )
        return _tokens_from_response(body)


def _tokens_from_response(body: dict[str, object]) -> OAuthTokens:
    access_token = body.get("access_token")
    refresh_token = body.get("refresh_token")
    expires_in = body.get("expires_in")
    scope = body.get("scope")
    if (
        not isinstance(access_token, str)
        or not access_token
        or not isinstance(refresh_token, str)
        or not refresh_token
        or not isinstance(expires_in, int | float)
    ):
        raise PlanningCenterOAuthError(
            "The Planning Center sign-in service returned an invalid response."
        )
    return OAuthTokens(
        access_token=access_token,
        refresh_token=refresh_token,
        expires_at=time.time() + float(expires_in),
        scope=scope if isinstance(scope, str) and scope else OAUTH_SCOPE,
    )


class PlanningCenterOAuthService:
    """Own the sign-in handshake, token storage and proactive refresh."""

    def __init__(
        self,
        *,
        client_id: str,
        tokens: OAuthTokenStore,
        control_plane: ControlPlaneOAuthClient | None = None,
    ) -> None:
        self._client_id = client_id
        self._tokens = tokens
        self._control_plane = control_plane or ControlPlaneOAuthClient()
        self._pending: PendingFlow | None = None
        self._lock = asyncio.Lock()

    @property
    def configured(self) -> bool:
        """Whether this build has a Planning Center OAuth client id at all."""

        return bool(self._client_id)

    async def start(self) -> tuple[str, str]:
        """Begin a sign-in: returns (authorize_url_without_redirect, state)."""

        if not self._client_id:
            raise PlanningCenterOAuthError(
                "This build of StagePilot cannot sign in to Planning Center."
            )
        flow_id, ticket = await self._control_plane.start_flow()
        verifier = generate_code_verifier()
        state = build_state(flow_id)
        self._pending = PendingFlow(
            state=state,
            code_verifier=verifier,
            flow_id=flow_id,
            ticket=ticket,
            started_at=time.time(),
        )
        return build_authorize_url(self._client_id, state, code_challenge_for(verifier)), state

    async def complete(self, *, state: str, code: str, redirect_uri: str) -> OAuthTokens:
        """Finish a sign-in with the code from the desktop loopback listener."""

        pending = self._pending
        if pending is None or pending.expired():
            self._pending = None
            raise PlanningCenterOAuthError(
                "This Planning Center sign-in expired. Start it again.", permanent=True
            )
        # Constant-time compare: `state` is the CSRF token for this flow.
        if not secrets.compare_digest(pending.state, state):
            raise PlanningCenterOAuthError(
                "Planning Center sign-in could not be verified. Try signing in again.",
                permanent=True,
            )
        tokens = await self._control_plane.exchange_code(
            flow=pending, code=code, redirect_uri=redirect_uri
        )
        self._pending = None
        self._tokens.save(tokens)
        return tokens

    def cancel(self) -> None:
        self._pending = None

    def stored(self) -> OAuthTokens | None:
        return self._tokens.load()

    def disconnect(self) -> None:
        self._pending = None
        self._tokens.clear()

    async def refresh_now(self, tokens: OAuthTokens | None = None) -> OAuthTokens:
        """Exchange the stored refresh token for a fresh token pair.

        Planning Center issues a NEW refresh token on every refresh and
        invalidates the old one, so the stored blob is overwritten in full
        every time -- never partially updated, never reusing the old
        refresh token.
        """

        async with self._lock:
            current = tokens or self._tokens.load()
            if current is None:
                raise PlanningCenterOAuthError(
                    "Planning Center is disconnected. Sign in again.", permanent=True
                )
            flow_id, ticket = await self._control_plane.start_flow()
            try:
                refreshed = await self._control_plane.refresh(
                    flow_id=flow_id,
                    ticket=ticket,
                    refresh_token=current.refresh_token,
                )
            except PlanningCenterOAuthError as exc:
                if exc.permanent:
                    # Explicit, actionable state -- not a silent failure and
                    # not a generic error: the UI shows "sign in again".
                    self._tokens.mark_needs_reconnect()
                raise
            self._tokens.save(refreshed)
            return refreshed

    async def valid_access_token(self) -> str:
        """Return a usable access token, refreshing first when it is due."""

        current = self._tokens.load()
        if current is None:
            raise PlanningCenterOAuthError(
                "Planning Center is disconnected. Sign in again.", permanent=True
            )
        if current.needs_reconnect:
            raise PlanningCenterOAuthError(
                "Your Planning Center connection has expired. Sign in again.", permanent=True
            )
        if current.needs_refresh():
            current = await self.refresh_now(current)
        return current.access_token

    def status(self, *, connection_method: Literal["oauth", "manual"]) -> OAuthConnectionStatus:
        try:
            tokens = self._tokens.load()
        except CredentialStoreError:
            tokens = None
        return OAuthConnectionStatus(
            connection_method=connection_method,
            connected=tokens is not None and not tokens.needs_reconnect,
            needs_reconnect=tokens is not None and tokens.needs_reconnect,
            expires_at=tokens.expires_at if tokens else None,
            scope=tokens.scope if tokens else None,
        )


async def proactive_refresh_loop(
    service: PlanningCenterOAuthService,
    stop: asyncio.Event,
    *,
    poll_seconds: float = REFRESH_POLL_SECONDS,
) -> None:
    """Refresh the access token ~15 minutes before it expires.

    Modeled on the existing `remote_runtime.reconcile` background task
    (a single lifespan-owned asyncio task polling a stop event), rather
    than introducing a new scheduler dependency. A transient failure just
    leaves the tokens alone and retries on the next tick; a permanent one
    has already set `needs_reconnect` inside `refresh_now`, so the loop
    stops trying until the user signs in again.
    """

    while not stop.is_set():
        try:
            tokens = await asyncio.to_thread(service.stored)
            if tokens is not None and not tokens.needs_reconnect and tokens.needs_refresh():
                await service.refresh_now(tokens)
                logger.info("planning_center_oauth_refreshed")
        except PlanningCenterOAuthError as exc:
            logger.warning("planning_center_oauth_refresh_failed permanent=%s", exc.permanent)
        except (CredentialStoreError, OSError):
            logger.warning("planning_center_oauth_refresh_unavailable")
        try:
            await asyncio.wait_for(stop.wait(), poll_seconds)
        except TimeoutError:
            continue


def tokens_from_json(raw: str) -> OAuthTokens | None:
    """Parse a stored blob, tolerating anything unreadable."""

    try:
        return OAuthTokens.model_validate(json.loads(raw))
    except (ValueError, ValidationError):
        return None
