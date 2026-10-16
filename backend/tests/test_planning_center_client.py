from __future__ import annotations

import asyncio
import base64
import traceback
from collections.abc import Callable
from datetime import UTC, date, datetime
from zoneinfo import ZoneInfo

import httpx
import pytest

from stagepilot.core.config import PlanningCenterSettings
from stagepilot.plugins.planning_center.client import PlanningCenterClient
from stagepilot.plugins.planning_center.errors import (
    PlanningCenterApiError,
    PlanningCenterAuthenticationError,
    PlanningCenterConfigurationError,
    PlanningCenterPermissionError,
    PlanningCenterRateLimitError,
    PlanningCenterResponseError,
    PlanningCenterTimeoutError,
    PlanningCenterTransportError,
)
from stagepilot.plugins.planning_center.models import PlanTimeAttributes, PlanTimeResource

JsonObject = dict[str, object]
Handler = Callable[[httpx.Request], httpx.Response]


def client_settings() -> PlanningCenterSettings:
    return PlanningCenterSettings(
        app_id="test-app-id",
        secret="test-secret",
        request_timeout_seconds=3,
        user_agent="StagePilot tests (https://github.com/tage-ilot/stagepilot)",
    )


def service_type_resource(
    identifier: str,
    name: str,
    sequence: int,
    **extra_attributes: object,
) -> JsonObject:
    attributes: JsonObject = {"name": name, "sequence": sequence}
    attributes.update(extra_attributes)
    return {
        "type": "ServiceType",
        "id": identifier,
        "attributes": attributes,
        "future_top_level_field": "ignored",
    }


def mock_transport(handler: Handler) -> httpx.MockTransport:
    return httpx.MockTransport(handler)


@pytest.mark.asyncio
async def test_list_service_types_uses_required_contract_and_maps_resources() -> None:
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(
            200,
            request=request,
            json={
                "data": [
                    service_type_resource("10", "Weekend", 1, unknown="ignored"),
                    service_type_resource(
                        "20",
                        "Legacy",
                        2,
                        archived_at="2026-01-01T00:00:00Z",
                    ),
                ],
                "meta": {"total_count": 2, "count": 2, "next": None},
            },
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        service_types = await client.list_service_types()

    assert [(item.id, item.name, item.sequence) for item in service_types] == [
        ("10", "Weekend", 1),
        ("20", "Legacy", 2),
    ]
    assert service_types[0].archived is False
    assert service_types[1].archived is True
    assert len(requests) == 1
    request = requests[0]
    assert request.url.path == "/services/v2/service_types"
    assert request.url.params["order"] == "sequence"
    assert request.url.params["per_page"] == "100"
    assert request.url.params["offset"] == "0"
    assert request.headers["Accept"] == "application/json"
    assert request.headers["User-Agent"].startswith("StagePilot tests")
    assert request.headers["X-PCO-API-Version"] == "2018-11-01"
    expected_auth = base64.b64encode(b"test-app-id:test-secret").decode("ascii")
    assert request.headers["Authorization"] == f"Basic {expected_auth}"
    assert "test-secret" not in str(request.url)


@pytest.mark.asyncio
async def test_list_service_types_follows_safe_next_link() -> None:
    offsets: list[str] = []

    def handler(request: httpx.Request) -> httpx.Response:
        offset = request.url.params.get("offset", "0")
        offsets.append(offset)
        if offset == "0":
            return httpx.Response(
                200,
                request=request,
                json={
                    "data": [service_type_resource("1", "First", 1)],
                    "meta": {"total_count": 2, "count": 1, "next": {"offset": 1}},
                    "links": {
                        "next": (
                            "https://api.planningcenteronline.com/services/v2/"
                            "service_types?order=sequence&per_page=100&offset=1"
                        )
                    },
                },
            )
        return httpx.Response(
            200,
            request=request,
            json={
                "data": [service_type_resource("2", "Second", 2)],
                "meta": {"total_count": 2, "count": 1, "next": None},
                "links": {"next": None},
            },
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        result = await client.list_service_types()

    assert [item.id for item in result] == ["1", "2"]
    assert offsets == ["0", "1"]


@pytest.mark.asyncio
async def test_list_service_types_rejects_unsafe_next_link() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            request=request,
            json={
                "data": [service_type_resource("1", "First", 1)],
                "links": {"next": "https://example.com/steal-credentials"},
            },
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterResponseError, match="unsafe pagination URL"):
            await client.list_service_types()


@pytest.mark.asyncio
async def test_list_service_types_rejects_pagination_loop() -> None:
    calls = 0

    def handler(request: httpx.Request) -> httpx.Response:
        nonlocal calls
        calls += 1
        return httpx.Response(
            200,
            request=request,
            json={
                "data": [service_type_resource("1", "First", 1)],
                "links": {"next": str(request.url)},
            },
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterResponseError, match="repeated pagination request"):
            await client.list_service_types()

    assert calls == 1


@pytest.mark.asyncio
async def test_pagination_uses_meta_offset_when_links_are_absent() -> None:
    offsets: list[str] = []

    def handler(request: httpx.Request) -> httpx.Response:
        offset = request.url.params.get("offset", "0")
        offsets.append(offset)
        if offset == "0":
            payload: JsonObject = {
                "data": [service_type_resource("1", "First", 1)],
                "meta": {"total_count": 2, "count": 1, "next": {"offset": 1}},
            }
        else:
            payload = {
                "data": [service_type_resource("2", "Second", 2)],
                "meta": {"total_count": 2, "count": 1, "next": None},
            }
        return httpx.Response(200, request=request, json=payload)

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        result = await client.list_service_types()

    assert [item.id for item in result] == ["1", "2"]
    assert offsets == ["0", "1"]


@pytest.mark.asyncio
async def test_pagination_rejects_empty_page_before_advertised_total() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            request=request,
            json={"data": [], "meta": {"total_count": 1, "count": 0}},
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterResponseError, match="non-advancing"):
            await client.list_service_types()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("status_code", "error_type"),
    [
        (401, PlanningCenterAuthenticationError),
        (403, PlanningCenterPermissionError),
    ],
)
async def test_authentication_and_permission_errors_are_typed(
    status_code: int,
    error_type: type[Exception],
) -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            status_code,
            request=request,
            json={"errors": [{"detail": "test-secret must never be echoed"}]},
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(error_type) as captured:
            await client.list_service_types()

    assert "test-secret" not in str(captured.value)
    assert "test-app-id" not in str(captured.value)


@pytest.mark.asyncio
async def test_rate_limit_preserves_only_safe_retry_delay() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            429,
            request=request,
            headers={"Retry-After": "7"},
            json={"errors": [{"detail": "test-secret"}]},
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterRateLimitError) as captured:
            await client.list_service_types()

    assert captured.value.retry_after_seconds == 7
    assert "test-secret" not in str(captured.value)


@pytest.mark.asyncio
@pytest.mark.parametrize("status_code", [400, 404, 500, 503])
async def test_other_api_errors_are_sanitized(status_code: int) -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            status_code,
            request=request,
            json={"errors": [{"detail": "test-secret"}]},
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterApiError) as captured:
            await client.list_service_types()

    assert captured.value.status_code == status_code
    assert "test-secret" not in str(captured.value)


@pytest.mark.asyncio
async def test_timeout_is_translated_without_secret_leakage() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("test-secret", request=request)

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterTimeoutError) as captured:
            await client.list_service_types()

    assert "test-secret" not in str(captured.value)
    assert "test-secret" not in "".join(traceback.format_exception(captured.value))


@pytest.mark.asyncio
async def test_transport_error_is_translated() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError("test-secret", request=request)

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterTransportError, match="Could not connect"):
            await client.list_service_types()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "payload",
    [
        {},
        {"data": {}},
        {"data": [{"type": "WrongType", "id": "1", "attributes": {"name": "X"}}]},
        {
            "data": [
                {
                    "type": "ServiceType",
                    "id": "../unsafe",
                    "attributes": {"name": "X"},
                }
            ]
        },
        {"data": [{"type": "ServiceType", "id": "1", "attributes": {}}]},
        {"data": [], "meta": {"total_count": "not-a-number"}},
    ],
)
async def test_malformed_service_type_responses_are_rejected(payload: object) -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, request=request, json=payload)

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterResponseError, match="invalid service type response"):
            await client.list_service_types()


@pytest.mark.asyncio
async def test_invalid_json_is_rejected() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            request=request,
            content=b"<html>not JSON</html>",
            headers={"Content-Type": "text/html"},
        )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(PlanningCenterResponseError, match="invalid service type response"):
            await client.list_service_types()


@pytest.mark.asyncio
async def test_cancellation_is_not_swallowed() -> None:
    def handler(_request: httpx.Request) -> httpx.Response:
        raise asyncio.CancelledError

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        with pytest.raises(asyncio.CancelledError):
            await client.list_service_types()


def test_client_requires_credentials() -> None:
    with pytest.raises(PlanningCenterConfigurationError, match="not configured"):
        PlanningCenterClient(PlanningCenterSettings())


def oauth_settings() -> PlanningCenterSettings:
    return PlanningCenterSettings(
        connection_method="oauth",
        access_token="oauth-access-token",
        request_timeout_seconds=3,
        user_agent="StagePilot tests (https://github.com/tage-ilot/stagepilot)",
    )


@pytest.mark.asyncio
async def test_oauth_connection_method_uses_bearer_authentication() -> None:
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(
            200,
            request=request,
            json={"data": [], "meta": {"total_count": 0, "count": 0, "next": None}},
        )

    async with PlanningCenterClient(oauth_settings(), transport=mock_transport(handler)) as client:
        assert await client.list_service_types() == []

    request = requests[0]
    assert request.headers["Authorization"] == "Bearer oauth-access-token"
    assert request.headers["X-PCO-API-Version"] == "2018-11-01"
    assert "oauth-access-token" not in str(request.url)


@pytest.mark.asyncio
async def test_manual_connection_method_still_uses_basic_authentication() -> None:
    """The existing PAT path must keep working exactly as before."""

    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(
            200,
            request=request,
            json={"data": [], "meta": {"total_count": 0, "count": 0, "next": None}},
        )

    settings = client_settings()
    assert settings.connection_method == "manual"
    async with PlanningCenterClient(settings, transport=mock_transport(handler)) as client:
        assert await client.list_service_types() == []

    expected = base64.b64encode(b"test-app-id:test-secret").decode("ascii")
    assert requests[0].headers["Authorization"] == f"Basic {expected}"


def test_matching_service_times_uses_dst_aware_zone_for_denver() -> None:
    """Regression for Greptile P1: a plain UTC-7 offset must not resolve to a
    fixed-offset zone (e.g. America/Phoenix) when the actual region observes DST.

    A service at 06:30 UTC on 2026-07-12 is Saturday 23:30 in Phoenix (no DST,
    fixed UTC-7) but Sunday 00:30 in Denver (DST-aware, UTC-6 in July). Using the
    wrong IANA zone silently excludes the service from Sunday's plan.
    """

    starts_at = datetime(2026, 7, 12, 6, 30, tzinfo=UTC)
    plan_time = PlanTimeResource(
        type="PlanTime",
        id="pt1",
        attributes=PlanTimeAttributes(
            starts_at=starts_at,
            time_type="service",
        ),
    )
    target_sunday = date(2026, 7, 12)

    denver_matches = PlanningCenterClient._matching_service_times(
        [plan_time],
        target_sunday,
        target_sunday,
        ZoneInfo("America/Denver"),
    )
    phoenix_matches = PlanningCenterClient._matching_service_times(
        [plan_time],
        target_sunday,
        target_sunday,
        ZoneInfo("America/Phoenix"),
    )

    assert len(denver_matches) == 1
    assert len(phoenix_matches) == 0


def test_oauth_without_an_access_token_is_a_configuration_error() -> None:
    with pytest.raises(PlanningCenterConfigurationError, match="not configured"):
        PlanningCenterClient(PlanningCenterSettings(connection_method="oauth"))
