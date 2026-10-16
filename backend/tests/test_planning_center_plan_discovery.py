from __future__ import annotations

from collections.abc import Callable, Mapping
from datetime import date, datetime, timedelta
from zoneinfo import ZoneInfo

import httpx
import pytest
from structlog.testing import capture_logs

from stagepilot.core.config import PlanningCenterSettings
from stagepilot.plugins.planning_center.client import PlanningCenterClient
from stagepilot.plugins.planning_center.errors import (
    PlanningCenterPlanSelectionError,
    PlanningCenterResponseError,
)
from stagepilot.plugins.planning_center.models import (
    PlanAmbiguousResult,
    PlanLoadedResult,
    PlanningCenterServiceType,
    PlanNotFoundResult,
    SkippedItemReason,
)

JsonObject = dict[str, object]
Handler = Callable[[httpx.Request], httpx.Response]
TARGET_DATE = date(2026, 7, 12)
NEXT_DATE = date(2026, 7, 13)
LATER_DATE = date(2026, 7, 14)
TIMEZONE_NAME = "America/Los_Angeles"


def client_settings() -> PlanningCenterSettings:
    return PlanningCenterSettings(
        app_id="test-app-id",
        secret="test-secret",
        request_timeout_seconds=3,
        user_agent="StagePilot tests (https://github.com/tage-ilot/stagepilot)",
    )


def weekend_service_type() -> PlanningCenterServiceType:
    return PlanningCenterServiceType(
        id="42",
        name="Weekend Services",
        sequence=1,
    )


def plan_resource(
    identifier: str,
    title: str,
    *,
    sort_date: str = "2026-07-12T16:00:00Z",
) -> JsonObject:
    return {
        "type": "Plan",
        "id": identifier,
        "attributes": {
            "title": title,
            "dates": "July 12, 2026",
            "sort_date": sort_date,
            "future_plan_field": "ignored",
        },
        "future_top_level_field": "ignored",
    }


def plan_time_resource(
    identifier: str,
    starts_at: str,
    *,
    time_type: str = "service",
) -> JsonObject:
    return {
        "type": "PlanTime",
        "id": identifier,
        "attributes": {
            "starts_at": starts_at,
            "time_type": time_type,
            "name": "Service",
            "future_time_field": "ignored",
        },
    }


def item_resource(
    identifier: str,
    title: str,
    item_type: str,
    sequence: int,
    *,
    length: int | None = 240,
    linked_song_id: str | None = None,
    description: str = "",
) -> JsonObject:
    song_data: JsonObject | None = None
    if linked_song_id is not None:
        song_data = {"type": "Song", "id": linked_song_id}
    return {
        "type": "Item",
        "id": identifier,
        "attributes": {
            "title": title,
            "description": description,
            "item_type": item_type,
            "length": length,
            "sequence": sequence,
            "future_item_field": "ignored",
        },
        "relationships": {"song": {"data": song_data}},
    }


class DiscoveryApi:
    def __init__(
        self,
        *,
        plans: list[JsonObject],
        plan_times_by_plan: Mapping[str, list[JsonObject]],
        items_by_plan: Mapping[str, list[JsonObject]] | None = None,
    ) -> None:
        self.plans = plans
        self.plan_times_by_plan = dict(plan_times_by_plan)
        self.items_by_plan = dict(items_by_plan or {})
        self.requests: list[httpx.Request] = []

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        path = request.url.path
        if path.endswith("/plans"):
            data = self.plans
        elif path.endswith("/plan_times"):
            plan_id = path.rsplit("/", 2)[-2]
            try:
                data = self.plan_times_by_plan[plan_id]
            except KeyError as exc:
                raise AssertionError(f"Unexpected plan-time request for {plan_id}") from exc
        elif path.endswith("/items"):
            plan_id = path.rsplit("/", 2)[-2]
            try:
                data = self.items_by_plan[plan_id]
            except KeyError as exc:
                raise AssertionError(f"Unexpected item request for {plan_id}") from exc
        else:
            raise AssertionError(f"Unexpected Planning Center request: {path}")
        return httpx.Response(200, request=request, json={"data": data})


class LookaheadDiscoveryApi:
    """Serve date-specific plan lists for upcoming-plan discovery tests."""

    def __init__(
        self,
        *,
        plans_by_date: Mapping[date, list[JsonObject]],
        plan_times_by_plan: Mapping[str, list[JsonObject]],
        items_by_plan: Mapping[str, list[JsonObject]] | None = None,
    ) -> None:
        self.plans_by_date = dict(plans_by_date)
        self.plan_times_by_plan = dict(plan_times_by_plan)
        self.items_by_plan = dict(items_by_plan or {})
        self.requests: list[httpx.Request] = []

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        path = request.url.path
        if path.endswith("/plans"):
            after = datetime.fromisoformat(request.url.params["after"].replace("Z", "+00:00"))
            before = datetime.fromisoformat(request.url.params["before"].replace("Z", "+00:00"))
            timezone = ZoneInfo(TIMEZONE_NAME)
            first_date = (after + timedelta(seconds=1)).astimezone(timezone).date()
            last_date = (before - timedelta(seconds=1)).astimezone(timezone).date()
            data = [
                plan
                for plan_date in sorted(self.plans_by_date)
                if first_date <= plan_date <= last_date
                for plan in self.plans_by_date[plan_date]
            ]
        elif path.endswith("/plan_times"):
            plan_id = path.rsplit("/", 2)[-2]
            try:
                data = self.plan_times_by_plan[plan_id]
            except KeyError as exc:
                raise AssertionError(f"Unexpected plan-time request for {plan_id}") from exc
        elif path.endswith("/items"):
            plan_id = path.rsplit("/", 2)[-2]
            try:
                data = self.items_by_plan[plan_id]
            except KeyError as exc:
                raise AssertionError(f"Unexpected item request for {plan_id}") from exc
        else:
            raise AssertionError(f"Unexpected Planning Center request: {path}")
        return httpx.Response(200, request=request, json={"data": data})


class MultiServiceDiscoveryApi:
    def __init__(
        self,
        *,
        plans_by_service_type: Mapping[str, list[JsonObject]],
        plan_times_by_plan: Mapping[str, list[JsonObject]],
        items_by_plan: Mapping[str, list[JsonObject]] | None = None,
    ) -> None:
        self.plans_by_service_type = dict(plans_by_service_type)
        self.plan_times_by_plan = dict(plan_times_by_plan)
        self.items_by_plan = dict(items_by_plan or {})
        self.requests: list[httpx.Request] = []

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        path = request.url.path
        parts = path.split("/")
        service_type_id = parts[4]
        if path.endswith("/plans"):
            data = self.plans_by_service_type.get(service_type_id, [])
        elif path.endswith("/plan_times"):
            plan_id = path.rsplit("/", 2)[-2]
            data = self.plan_times_by_plan[plan_id]
        elif path.endswith("/items"):
            plan_id = path.rsplit("/", 2)[-2]
            data = self.items_by_plan[plan_id]
        else:
            raise AssertionError(f"Unexpected Planning Center request: {path}")
        return httpx.Response(200, request=request, json={"data": data})


def mock_transport(handler: Handler) -> httpx.MockTransport:
    return httpx.MockTransport(handler)


class PartiallyFailingMultiServiceApi:
    """One service type's /plans response is malformed; the rest are valid."""

    def __init__(
        self,
        *,
        failing_service_type_id: str,
        plans_by_service_type: Mapping[str, list[JsonObject]],
        plan_times_by_plan: Mapping[str, list[JsonObject]],
        items_by_plan: Mapping[str, list[JsonObject]] | None = None,
    ) -> None:
        self.failing_service_type_id = failing_service_type_id
        self.plans_by_service_type = dict(plans_by_service_type)
        self.plan_times_by_plan = dict(plan_times_by_plan)
        self.items_by_plan = dict(items_by_plan or {})
        self.requests: list[httpx.Request] = []

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        path = request.url.path
        parts = path.split("/")
        service_type_id = parts[4]
        if path.endswith("/plans"):
            if service_type_id == self.failing_service_type_id:
                # HTTP 200, but a response shape the typed boundary rejects
                # (missing the required "type" discriminator on the resource).
                return httpx.Response(200, request=request, json={"data": [{"id": "plan-bad"}]})
            data = self.plans_by_service_type.get(service_type_id, [])
        elif path.endswith("/plan_times"):
            plan_id = path.rsplit("/", 2)[-2]
            data = self.plan_times_by_plan[plan_id]
        elif path.endswith("/items"):
            plan_id = path.rsplit("/", 2)[-2]
            data = self.items_by_plan[plan_id]
        else:
            raise AssertionError(f"Unexpected Planning Center request: {path}")
        return httpx.Response(200, request=request, json={"data": data})


@pytest.mark.asyncio
async def test_all_service_types_isolates_one_failing_service_type() -> None:
    """One service type's unparsable /plans response must not abort the rest.

    Regression test for the production bug where a single service type
    (e.g. real-world id 1387640) returning a plan shape that fails Pydantic
    validation aborted `load_plan_for_service_types` entirely, discarding
    every other configured service type's already-fetched valid candidates.
    """

    api = PartiallyFailingMultiServiceApi(
        failing_service_type_id="84",
        plans_by_service_type={
            "42": [plan_resource("plan-healthy", "Sunday")],
        },
        plan_times_by_plan={
            "plan-healthy": [plan_time_resource("time-healthy", "2026-07-12T16:00:00Z")],
        },
        items_by_plan={
            "plan-healthy": [item_resource("item-healthy", "Opening Song", "song", 1)],
        },
    )
    service_types = [
        weekend_service_type(),
        PlanningCenterServiceType(id="84", name="Midweek", sequence=2),
    ]

    with capture_logs() as logs:
        async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
            result = await client.load_plan_for_service_types(
                service_types,
                TARGET_DATE,
                TIMEZONE_NAME,
                lookahead_days=30,
            )

    assert isinstance(result, PlanLoadedResult)
    assert result.plan.id == "plan-healthy"
    assert result.plan.service_type_id == "42"
    failure_logs = [
        entry for entry in logs if entry.get("event") == "planning_center_service_type_load_failed"
    ]
    assert len(failure_logs) == 1
    assert failure_logs[0]["log_level"] == "warning"
    assert failure_logs[0]["exception_type"] == "PlanningCenterResponseError"


@pytest.mark.asyncio
async def test_all_service_types_raises_when_every_type_fails() -> None:
    """If no service type produces valid data, the aggregation must still error."""

    api = PartiallyFailingMultiServiceApi(
        failing_service_type_id="42",
        plans_by_service_type={},
        plan_times_by_plan={},
    )
    service_types = [weekend_service_type()]

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        with pytest.raises(PlanningCenterResponseError):
            await client.load_plan_for_service_types(
                service_types,
                TARGET_DATE,
                TIMEZONE_NAME,
                lookahead_days=30,
            )


@pytest.mark.asyncio
@pytest.mark.parametrize("connection_method", ["manual", "oauth"])
async def test_all_service_types_uses_nearest_plan_across_auth_modes(
    connection_method: str,
) -> None:
    api = MultiServiceDiscoveryApi(
        plans_by_service_type={
            "42": [plan_resource("plan-later", "Tuesday", sort_date="2026-07-14T16:00:00Z")],
            "84": [plan_resource("plan-nearest", "Monday", sort_date="2026-07-13T16:00:00Z")],
        },
        plan_times_by_plan={
            "plan-later": [plan_time_resource("time-later", "2026-07-14T16:00:00Z")],
            "plan-nearest": [plan_time_resource("time-nearest", "2026-07-13T16:00:00Z")],
        },
        items_by_plan={"plan-nearest": []},
    )
    settings = (
        client_settings()
        if connection_method == "manual"
        else PlanningCenterSettings(
            connection_method="oauth",
            access_token="oauth-access-token",
            request_timeout_seconds=3,
        )
    )
    service_types = [
        weekend_service_type(),
        PlanningCenterServiceType(id="84", name="Midweek", sequence=2),
    ]

    async with PlanningCenterClient(settings, transport=mock_transport(api)) as client:
        result = await client.load_plan_for_service_types(
            service_types,
            TARGET_DATE,
            TIMEZONE_NAME,
            lookahead_days=30,
        )

    assert isinstance(result, PlanLoadedResult)
    assert result.plan.id == "plan-nearest"
    assert result.plan.service_type_id == "84"
    assert result.plan.date == NEXT_DATE


@pytest.mark.asyncio
async def test_all_service_types_surfaces_cross_type_ties_as_candidates() -> None:
    api = MultiServiceDiscoveryApi(
        plans_by_service_type={
            "42": [plan_resource("plan-morning", "Morning")],
            "84": [plan_resource("plan-evening", "Evening", sort_date="2026-07-13T01:00:00Z")],
        },
        plan_times_by_plan={
            "plan-morning": [plan_time_resource("time-morning", "2026-07-12T16:00:00Z")],
            "plan-evening": [plan_time_resource("time-evening", "2026-07-13T01:00:00Z")],
        },
    )
    service_types = [
        weekend_service_type(),
        PlanningCenterServiceType(id="84", name="Evening Services", sequence=2),
    ]

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_service_types(
            service_types,
            TARGET_DATE,
            TIMEZONE_NAME,
            lookahead_days=30,
        )

    assert isinstance(result, PlanAmbiguousResult)
    assert result.service_type is None
    assert [candidate.id for candidate in result.candidates] == [
        "plan-morning",
        "plan-evening",
    ]
    assert [candidate.service_type_id for candidate in result.candidates] == ["42", "84"]


@pytest.mark.asyncio
async def test_all_service_types_ignores_types_without_plans() -> None:
    api = MultiServiceDiscoveryApi(
        plans_by_service_type={
            "42": [],
            "84": [plan_resource("plan-only", "Only plan")],
        },
        plan_times_by_plan={
            "plan-only": [plan_time_resource("time-only", "2026-07-12T16:00:00Z")],
        },
        items_by_plan={"plan-only": []},
    )
    service_types = [
        weekend_service_type(),
        PlanningCenterServiceType(id="84", name="Midweek", sequence=2),
    ]

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_service_types(
            service_types,
            TARGET_DATE,
            TIMEZONE_NAME,
        )

    assert isinstance(result, PlanLoadedResult)
    assert result.plan.id == "plan-only"
    plan_paths = [
        request.url.path for request in api.requests if request.url.path.endswith("/plans")
    ]
    assert plan_paths == [
        "/services/v2/service_types/42/plans",
        "/services/v2/service_types/84/plans",
    ]


@pytest.mark.asyncio
async def test_no_plans_returns_not_found_without_detail_requests() -> None:
    api = DiscoveryApi(plans=[], plan_times_by_plan={})

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanNotFoundResult)
    assert result.target_date == TARGET_DATE
    assert result.service_type is not None
    assert result.service_type.id == "42"
    assert [request.url.path for request in api.requests] == ["/services/v2/service_types/42/plans"]


@pytest.mark.asyncio
async def test_today_plan_wins_when_search_window_contains_future_plan() -> None:
    api = LookaheadDiscoveryApi(
        plans_by_date={
            TARGET_DATE: [plan_resource("plan-today", "Today's Service")],
            NEXT_DATE: [
                plan_resource(
                    "plan-tomorrow",
                    "Tomorrow's Service",
                    sort_date="2026-07-13T16:00:00Z",
                )
            ],
        },
        plan_times_by_plan={
            "plan-today": [plan_time_resource("time-today", "2026-07-12T16:00:00Z")],
            "plan-tomorrow": [plan_time_resource("time-tomorrow", "2026-07-13T16:00:00Z")],
        },
        items_by_plan={"plan-today": []},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            TIMEZONE_NAME,
            lookahead_days=30,
        )

    assert isinstance(result, PlanLoadedResult)
    assert result.plan.id == "plan-today"
    assert result.plan.date == TARGET_DATE
    plan_requests = [request for request in api.requests if request.url.path.endswith("/plans")]
    assert len(plan_requests) == 1


@pytest.mark.asyncio
async def test_nearest_upcoming_plan_wins_after_empty_dates() -> None:
    api = LookaheadDiscoveryApi(
        plans_by_date={
            TARGET_DATE: [],
            NEXT_DATE: [],
            LATER_DATE: [
                plan_resource(
                    "plan-nearest",
                    "Tuesday Service",
                    sort_date="2026-07-14T16:00:00Z",
                )
            ],
        },
        plan_times_by_plan={
            "plan-nearest": [plan_time_resource("time-nearest", "2026-07-14T16:00:00Z")]
        },
        items_by_plan={"plan-nearest": []},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            TIMEZONE_NAME,
            lookahead_days=30,
        )

    assert isinstance(result, PlanLoadedResult)
    assert result.candidate.target_date == LATER_DATE
    assert result.plan.id == "plan-nearest"
    assert result.plan.date == LATER_DATE
    plan_requests = [request for request in api.requests if request.url.path.endswith("/plans")]
    assert len(plan_requests) == 1


@pytest.mark.asyncio
async def test_plan_after_configured_lookahead_is_not_considered() -> None:
    api = LookaheadDiscoveryApi(
        plans_by_date={
            LATER_DATE: [
                plan_resource(
                    "plan-outside-window",
                    "Outside Window",
                    sort_date="2026-07-14T16:00:00Z",
                )
            ]
        },
        plan_times_by_plan={
            "plan-outside-window": [
                plan_time_resource("time-outside-window", "2026-07-14T16:00:00Z")
            ]
        },
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            TIMEZONE_NAME,
            lookahead_days=1,
        )

    assert isinstance(result, PlanNotFoundResult)
    assert result.target_date == TARGET_DATE
    assert [request.url.path for request in api.requests] == ["/services/v2/service_types/42/plans"]


@pytest.mark.asyncio
async def test_nearest_future_ambiguity_excludes_later_plans() -> None:
    api = LookaheadDiscoveryApi(
        plans_by_date={
            TARGET_DATE: [],
            NEXT_DATE: [
                plan_resource(
                    "plan-morning",
                    "Monday Morning",
                    sort_date="2026-07-13T16:00:00Z",
                ),
                plan_resource(
                    "plan-evening",
                    "Monday Evening",
                    sort_date="2026-07-14T01:00:00Z",
                ),
            ],
            LATER_DATE: [
                plan_resource(
                    "plan-later",
                    "Tuesday Service",
                    sort_date="2026-07-14T16:00:00Z",
                )
            ],
        },
        plan_times_by_plan={
            "plan-morning": [plan_time_resource("time-morning", "2026-07-13T16:00:00Z")],
            "plan-evening": [plan_time_resource("time-evening", "2026-07-14T01:00:00Z")],
            "plan-later": [plan_time_resource("time-later", "2026-07-14T16:00:00Z")],
        },
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            TIMEZONE_NAME,
            lookahead_days=30,
        )

    assert isinstance(result, PlanAmbiguousResult)
    assert result.target_date == NEXT_DATE
    assert [candidate.id for candidate in result.candidates] == [
        "plan-morning",
        "plan-evening",
    ]
    assert all(candidate.target_date == NEXT_DATE for candidate in result.candidates)
    plan_requests = [request for request in api.requests if request.url.path.endswith("/plans")]
    assert len(plan_requests) == 1
    assert not any(request.url.path.endswith("/items") for request in api.requests)


@pytest.mark.asyncio
async def test_explicit_selection_can_load_a_future_candidate() -> None:
    api = LookaheadDiscoveryApi(
        plans_by_date={
            TARGET_DATE: [],
            NEXT_DATE: [
                plan_resource(
                    "plan-morning",
                    "Monday Morning",
                    sort_date="2026-07-13T16:00:00Z",
                ),
                plan_resource(
                    "plan-evening",
                    "Monday Evening",
                    sort_date="2026-07-14T01:00:00Z",
                ),
            ],
        },
        plan_times_by_plan={
            "plan-morning": [plan_time_resource("time-morning", "2026-07-13T16:00:00Z")],
            "plan-evening": [plan_time_resource("time-evening", "2026-07-14T01:00:00Z")],
        },
        items_by_plan={"plan-evening": []},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            TIMEZONE_NAME,
            selected_plan_id="plan-evening",
            lookahead_days=30,
        )

    assert isinstance(result, PlanLoadedResult)
    assert result.candidate.id == "plan-evening"
    assert result.candidate.target_date == NEXT_DATE
    assert result.plan.id == "plan-evening"
    assert result.plan.date == NEXT_DATE
    item_requests = [request for request in api.requests if request.url.path.endswith("/items")]
    assert [request.url.path for request in item_requests] == [
        "/services/v2/service_types/42/plans/plan-evening/items"
    ]


@pytest.mark.asyncio
async def test_exact_local_date_matching_converts_utc_service_times() -> None:
    api = DiscoveryApi(
        plans=[
            plan_resource(
                "plan-1",
                "Sunday Worship",
                sort_date="2026-07-13T06:30:00Z",
            )
        ],
        plan_times_by_plan={
            "plan-1": [
                plan_time_resource("time-late-sunday", "2026-07-13T06:30:00Z"),
                plan_time_resource("time-monday", "2026-07-13T07:00:00Z"),
            ]
        },
        items_by_plan={"plan-1": []},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanLoadedResult)
    assert [value.isoformat() for value in result.candidate.service_times] == [
        "2026-07-12T23:30:00-07:00"
    ]
    assert result.plan.date == TARGET_DATE
    assert result.plan.service_type_id == "42"
    assert result.plan.service_times == ["23:30"]
    plan_request = api.requests[0]
    assert plan_request.url.params["filter"] == "after,before"
    assert plan_request.url.params["after"] == "2026-07-12T06:59:59Z"
    assert plan_request.url.params["before"] == "2026-07-13T07:00:00Z"


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("target_date", "expected_after", "expected_before"),
    [
        (date(2026, 3, 8), "2026-03-08T07:59:59Z", "2026-03-09T07:00:00Z"),
        (date(2026, 11, 1), "2026-11-01T06:59:59Z", "2026-11-02T08:00:00Z"),
    ],
)
async def test_plan_window_uses_consecutive_local_midnights_across_dst(
    target_date: date,
    expected_after: str,
    expected_before: str,
) -> None:
    api = DiscoveryApi(plans=[], plan_times_by_plan={})

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            target_date,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanNotFoundResult)
    request = api.requests[0]
    assert request.url.params["after"] == expected_after
    assert request.url.params["before"] == expected_before


@pytest.mark.asyncio
async def test_plan_with_only_next_local_day_service_time_is_not_found() -> None:
    api = DiscoveryApi(
        plans=[plan_resource("plan-1", "Sunday Worship")],
        plan_times_by_plan={"plan-1": [plan_time_resource("time-monday", "2026-07-13T07:00:00Z")]},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanNotFoundResult)
    assert not any(request.url.path.endswith("/items") for request in api.requests)


@pytest.mark.asyncio
@pytest.mark.parametrize("time_type", ["rehearsal", "other"])
async def test_non_service_times_never_qualify_a_plan(time_type: str) -> None:
    api = DiscoveryApi(
        plans=[plan_resource("plan-1", "Sunday Worship")],
        plan_times_by_plan={
            "plan-1": [
                plan_time_resource(
                    "time-1",
                    "2026-07-12T16:00:00Z",
                    time_type=time_type,
                )
            ]
        },
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanNotFoundResult)
    assert not any(request.url.path.endswith("/items") for request in api.requests)


@pytest.mark.asyncio
async def test_multiple_matching_plans_are_ambiguous_without_item_fetch() -> None:
    api = DiscoveryApi(
        plans=[
            plan_resource("plan-1", "Morning Worship"),
            plan_resource("plan-2", "Evening Worship"),
        ],
        plan_times_by_plan={
            "plan-1": [plan_time_resource("time-1", "2026-07-12T16:00:00Z")],
            "plan-2": [plan_time_resource("time-2", "2026-07-13T01:00:00Z")],
        },
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanAmbiguousResult)
    assert [candidate.id for candidate in result.candidates] == ["plan-1", "plan-2"]
    assert not any(request.url.path.endswith("/items") for request in api.requests)


@pytest.mark.asyncio
async def test_explicit_selection_loads_only_the_matching_selected_plan() -> None:
    api = DiscoveryApi(
        plans=[
            plan_resource("plan-1", "Morning Worship"),
            plan_resource("plan-2", "Evening Worship"),
        ],
        plan_times_by_plan={
            "plan-1": [plan_time_resource("time-1", "2026-07-12T16:00:00Z")],
            "plan-2": [plan_time_resource("time-2", "2026-07-13T01:00:00Z")],
        },
        items_by_plan={"plan-2": []},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
            selected_plan_id="plan-2",
        )

    assert isinstance(result, PlanLoadedResult)
    assert result.candidate.id == "plan-2"
    assert result.plan.id == "plan-2"
    item_requests = [request for request in api.requests if request.url.path.endswith("/items")]
    assert [request.url.path for request in item_requests] == [
        "/services/v2/service_types/42/plans/plan-2/items"
    ]


@pytest.mark.asyncio
async def test_explicit_selection_must_match_a_current_day_candidate() -> None:
    api = DiscoveryApi(
        plans=[plan_resource("plan-1", "Morning Worship")],
        plan_times_by_plan={"plan-1": [plan_time_resource("time-1", "2026-07-12T16:00:00Z")]},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        with pytest.raises(
            PlanningCenterPlanSelectionError,
            match="does not match a candidate on the nearest available service date",
        ):
            await client.load_plan_for_date(
                weekend_service_type(),
                TARGET_DATE,
                "America/Los_Angeles",
                selected_plan_id="different-plan",
            )

    assert not any(request.url.path.endswith("/items") for request in api.requests)


@pytest.mark.asyncio
async def test_song_extraction_orders_linked_and_generic_items_and_reports_skips() -> None:
    api = DiscoveryApi(
        plans=[plan_resource("plan-1", "Sunday Worship")],
        plan_times_by_plan={"plan-1": [plan_time_resource("time-1", "2026-07-12T16:00:00Z")]},
        items_by_plan={
            "plan-1": [
                item_resource(
                    "item-talk",
                    "Message",
                    "item",
                    50,
                    description="Pastor John",
                ),
                item_resource(
                    "item-linked",
                    "  Linked Song  ",
                    "song",
                    30,
                    length=281,
                    linked_song_id="song-10",
                    description="  Key of G  ",
                ),
                item_resource("item-header", "Worship", "header", 10),
                item_resource(
                    "item-generic",
                    "Generic Song",
                    "song",
                    20,
                    length=None,
                ),
                item_resource(
                    "item-zero",
                    "Zero-length Song",
                    "song",
                    25,
                    length=0,
                    linked_song_id="song-10",
                ),
                item_resource("item-media", "Video", "media", 40),
            ]
        },
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanLoadedResult)
    assert [(song.id, song.title, song.order) for song in result.plan.songs] == [
        ("item-generic", "Generic Song", 1),
        ("item-zero", "Zero-length Song", 2),
        ("item-linked", "Linked Song", 3),
    ]
    assert [song.service_sequence for song in result.plan.songs] == [20, 25, 30]
    generic, zero, linked = result.plan.songs
    assert generic.is_generic is True
    assert generic.source_song_id is None
    assert generic.duration_seconds is None
    assert zero.duration_seconds == 0
    assert zero.source_song_id == "song-10"
    assert zero.description is None
    assert generic.description is None
    assert linked.is_generic is False
    assert linked.source_song_id == "song-10"
    assert linked.duration_seconds == 281
    assert linked.description == "Key of G"
    assert [item.item_id for item in result.skipped_items] == [
        "item-header",
        "item-media",
        "item-talk",
    ]
    assert [item.reason for item in result.skipped_items] == [
        SkippedItemReason.HEADER,
        SkippedItemReason.MEDIA,
        SkippedItemReason.NOT_SONG,
    ]
    assert [item.duration_seconds for item in result.skipped_items] == [240, 240, 240]
    assert [item.description for item in result.skipped_items] == [
        None,
        None,
        "Pastor John",
    ]
    item_request = next(request for request in api.requests if request.url.path.endswith("/items"))
    assert item_request.url.params["include"] == "song"


@pytest.mark.asyncio
async def test_item_pagination_stays_on_item_endpoint_and_sorts_across_pages() -> None:
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        path = request.url.path
        if path.endswith("/plans"):
            payload: JsonObject = {"data": [plan_resource("plan-1", "Sunday Worship")]}
        elif path.endswith("/plan_times"):
            payload = {"data": [plan_time_resource("time-1", "2026-07-12T16:00:00Z")]}
        elif path.endswith("/items") and request.url.params.get("offset", "0") == "0":
            payload = {
                "data": [item_resource("item-later", "Later Song", "song", 30)],
                "meta": {"total_count": 2, "next": {"offset": 1}},
            }
        elif path.endswith("/items") and request.url.params["offset"] == "1":
            payload = {
                "data": [item_resource("item-earlier", "Earlier Song", "song", 20)],
                "meta": {"total_count": 2, "next": None},
            }
        else:
            raise AssertionError(f"Unexpected Planning Center request: {request.url}")
        return httpx.Response(200, request=request, json=payload)

    async with PlanningCenterClient(client_settings(), transport=mock_transport(handler)) as client:
        result = await client.load_plan_for_date(
            weekend_service_type(),
            TARGET_DATE,
            "America/Los_Angeles",
        )

    assert isinstance(result, PlanLoadedResult)
    assert [song.id for song in result.plan.songs] == ["item-earlier", "item-later"]
    item_requests = [request for request in requests if request.url.path.endswith("/items")]
    assert [request.url.params["offset"] for request in item_requests] == ["0", "1"]
    assert all(request.url.params["include"] == "song" for request in item_requests)


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("stage", "message"),
    [
        ("plan", "invalid plan response"),
        ("plan_time", "invalid plan time response"),
        ("item", "invalid plan item response"),
    ],
)
async def test_malformed_discovery_resources_are_rejected(stage: str, message: str) -> None:
    plans = [plan_resource("plan-1", "Sunday Worship")]
    plan_times = [plan_time_resource("time-1", "2026-07-12T16:00:00Z")]
    items = [item_resource("item-1", "Song", "song", 1)]
    if stage == "plan":
        plans = [
            {
                "type": "WrongType",
                "id": "plan-1",
                "attributes": {"title": "Sunday Worship"},
            }
        ]
    elif stage == "plan_time":
        plan_times = [plan_time_resource("time-1", "2026-07-12T16:00:00")]
    elif stage == "item":
        items = [item_resource("item-1", "Song", "song", 1, length=-1)]

    api = DiscoveryApi(
        plans=plans,
        plan_times_by_plan={"plan-1": plan_times},
        items_by_plan={"plan-1": items},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        with pytest.raises(PlanningCenterResponseError, match=message):
            await client.load_plan_for_date(
                weekend_service_type(),
                TARGET_DATE,
                "America/Los_Angeles",
            )


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("stage", "message"),
    [
        ("plan", "duplicate plans"),
        ("plan_time", "duplicate plan times"),
        ("item", "duplicate plan items"),
    ],
)
async def test_duplicate_discovery_resource_ids_are_rejected(stage: str, message: str) -> None:
    plan = plan_resource("plan-1", "Sunday Worship")
    plan_time = plan_time_resource("time-1", "2026-07-12T16:00:00Z")
    item = item_resource("item-1", "Song", "song", 1)
    plans = [plan, plan] if stage == "plan" else [plan]
    plan_times = [plan_time, plan_time] if stage == "plan_time" else [plan_time]
    items = [item, item] if stage == "item" else [item]
    api = DiscoveryApi(
        plans=plans,
        plan_times_by_plan={"plan-1": plan_times},
        items_by_plan={"plan-1": items},
    )

    async with PlanningCenterClient(client_settings(), transport=mock_transport(api)) as client:
        with pytest.raises(PlanningCenterResponseError, match=message):
            await client.load_plan_for_date(
                weekend_service_type(),
                TARGET_DATE,
                "America/Los_Angeles",
            )
