from __future__ import annotations

from datetime import UTC, date, datetime
from pathlib import Path
from zoneinfo import ZoneInfo

import pytest
from fastapi.testclient import TestClient

from stagepilot.api.routes import _production_service_ready
from stagepilot.core.config import PlanningCenterSettings, Settings
from stagepilot.main import create_app
from stagepilot.models.state import (
    ApplicationState,
    ConnectionStatus,
    ServiceLoadState,
    ServiceLoadStatus,
    ServicePlan,
    Song,
)
from stagepilot.plugins.planning_center.models import (
    PlanAmbiguousResult,
    PlanLoadedResult,
    PlanningCenterPlanCandidate,
    PlanningCenterServiceType,
)

TARGET_DATE = date(2026, 7, 12)
PRIVATE_APP_ID = "private-app-id"
PRIVATE_SECRET = "private-secret"


def planning_center_settings(*, demo_mode: bool = False) -> Settings:
    return Settings(
        demo_mode=demo_mode,
        timezone="America/Los_Angeles",
        planning_center=PlanningCenterSettings(
            app_id=PRIVATE_APP_ID,
            secret=PRIVATE_SECRET,
            service_type_id="42",
        ),
    )


def fixed_today(_timezone: ZoneInfo) -> date:
    return TARGET_DATE


class FakePlanningCenterClient:
    def __init__(self) -> None:
        self.service_type = PlanningCenterServiceType(
            id="42",
            name="Weekend Services",
            sequence=1,
        )
        self.candidates = [
            PlanningCenterPlanCandidate(
                id="plan-early",
                title="Sunday Worship — 9:00",
                service_type_id="42",
                service_type_name="Weekend Services",
                target_date=TARGET_DATE,
                service_times=[datetime(2026, 7, 12, 16, 0, tzinfo=UTC)],
            ),
            PlanningCenterPlanCandidate(
                id="plan-late",
                title="Sunday Worship — 11:00",
                service_type_id="42",
                service_type_name="Weekend Services",
                target_date=TARGET_DATE,
                service_times=[datetime(2026, 7, 12, 18, 0, tzinfo=UTC)],
            ),
        ]
        self.selected_plan_ids: list[str | None] = []
        self.lookahead_days: list[int] = []
        self.closed = False

    async def list_service_types(self) -> list[PlanningCenterServiceType]:
        return [self.service_type]

    async def load_plan_for_date(
        self,
        service_type: PlanningCenterServiceType,
        target_date: date,
        timezone_name: str,
        *,
        selected_plan_id: str | None = None,
        lookahead_days: int = 0,
    ) -> PlanAmbiguousResult | PlanLoadedResult:
        assert service_type == self.service_type
        assert target_date == TARGET_DATE
        assert timezone_name == "America/Los_Angeles"
        self.selected_plan_ids.append(selected_plan_id)
        self.lookahead_days.append(lookahead_days)
        if selected_plan_id is None:
            return PlanAmbiguousResult(
                service_type=self.service_type,
                target_date=TARGET_DATE,
                candidates=self.candidates,
            )

        candidate = next(
            (value for value in self.candidates if value.id == selected_plan_id),
            None,
        )
        if candidate is None:
            raise AssertionError(f"Unexpected selected plan: {selected_plan_id}")
        return PlanLoadedResult(
            candidate=candidate,
            plan=ServicePlan(
                id=candidate.id,
                title=candidate.title,
                date=TARGET_DATE,
                service_type=self.service_type.name,
                service_times=["09:00"],
                duration_source="Planning Center scheduled item length",
                songs=[
                    Song(
                        id="item-1",
                        title="Battle Belongs",
                        duration_seconds=281,
                        order=1,
                        source_song_id="song-1",
                    )
                ],
            ),
        )

    async def load_plan_for_service_types(
        self,
        service_types: list[PlanningCenterServiceType],
        target_date: date,
        timezone_name: str,
        *,
        selected_plan_id: str | None = None,
        lookahead_days: int = 0,
    ) -> PlanAmbiguousResult | PlanLoadedResult:
        return await self.load_plan_for_date(
            service_types[0],
            target_date,
            timezone_name,
            selected_plan_id=selected_plan_id,
            lookahead_days=lookahead_days,
        )

    async def resolve_selected_plan(
        self,
        candidates: list[PlanningCenterPlanCandidate],
        service_types: list[PlanningCenterServiceType],
        target_date: date,
        *,
        selected_plan_id: str,
    ) -> PlanAmbiguousResult | PlanLoadedResult:
        return await self.load_plan_for_date(
            service_types[0],
            target_date,
            "America/Los_Angeles",
            selected_plan_id=selected_plan_id,
        )

    async def close(self) -> None:
        self.closed = True


class RecordingPlanningCenterFactory:
    def __init__(self, client: FakePlanningCenterClient) -> None:
        self.client = client
        self.calls: list[PlanningCenterSettings] = []

    def __call__(self, settings: PlanningCenterSettings) -> FakePlanningCenterClient:
        self.calls.append(settings)
        return self.client


def test_health_state_and_action_endpoints() -> None:
    app = create_app(Settings(demo_mode=True))

    with TestClient(app) as client:
        health = client.get("/api/v1/health")
        state = client.get("/api/v1/state")
        action = client.post("/api/v1/actions/start_next")

    assert health.status_code == 200
    assert health.json()["status"] == "healthy"
    assert state.status_code == 200
    assert len(state.json()["plan"]["songs"]) == 4
    assert action.status_code == 200
    assert action.json()["accepted"] is True
    assert action.json()["state"]["current_song"]["title"] == "Battle Belongs"
    assert action.json()["state"]["timer"]["status"] == "running"


def test_dashboard_is_served_from_the_backend_root(tmp_path: Path) -> None:
    (tmp_path / "index.html").write_text(
        '<html><script src="/assets/dashboard.js"></script></html>',
        encoding="utf-8",
    )
    assets = tmp_path / "assets"
    assets.mkdir()
    (assets / "dashboard.js").write_text("window.stagepilot = true;", encoding="utf-8")
    app = create_app(Settings(demo_mode=True), web_root=tmp_path)

    with TestClient(app) as client:
        dashboard = client.get("/")
        asset = client.get("/assets/dashboard.js")
        health = client.get("/api/v1/health")

    assert dashboard.status_code == 200
    assert 'src="/assets/dashboard.js"' in dashboard.text
    assert asset.status_code == 200
    assert asset.text == "window.stagepilot = true;"
    assert health.status_code == 200


def test_unknown_action_is_rejected_by_validation() -> None:
    app = create_app(Settings(demo_mode=True))

    with TestClient(app) as client:
        response = client.post("/api/v1/actions/not-an-action")

    assert response.status_code == 422


def test_demo_mode_never_constructs_the_real_planning_center_client() -> None:
    planning_center_client = FakePlanningCenterClient()
    factory = RecordingPlanningCenterFactory(planning_center_client)
    app = create_app(
        planning_center_settings(demo_mode=True),
        planning_center_client_factory=factory,
        planning_center_today_provider=fixed_today,
    )

    with TestClient(app) as client:
        state = client.get("/api/v1/state")

    assert state.status_code == 200
    body = state.json()
    assert body["plan"]["title"] == "Sunday Worship — Demo"
    assert body["planning_center_status"] == ConnectionStatus.DISCONNECTED
    assert body["midi_status"] == ConnectionStatus.DISCONNECTED
    assert body["propresenter_status"] == ConnectionStatus.DISCONNECTED
    assert body["lights_status"] == ConnectionStatus.DISCONNECTED
    assert factory.calls == []
    assert planning_center_client.closed is False


def test_non_demo_startup_exposes_ambiguous_planning_center_state() -> None:
    planning_center_client = FakePlanningCenterClient()
    factory = RecordingPlanningCenterFactory(planning_center_client)
    app = create_app(
        planning_center_settings(),
        planning_center_client_factory=factory,
        planning_center_today_provider=fixed_today,
    )

    with TestClient(app) as client:
        health = client.get("/api/v1/health")
        state = client.get("/api/v1/state")

    assert health.status_code == 200
    assert health.json()["status"] == "degraded"
    assert state.status_code == 200
    body = state.json()
    assert body["planning_center_status"] == ConnectionStatus.CONNECTED
    assert body["plan"] is None
    assert body["service_load"]["status"] == ServiceLoadStatus.AMBIGUOUS
    assert body["service_load"]["target_date"] == TARGET_DATE.isoformat()
    assert [candidate["id"] for candidate in body["service_load"]["candidates"]] == [
        "plan-early",
        "plan-late",
    ]
    assert planning_center_client.selected_plan_ids == [None]
    assert len(factory.calls) == 1
    assert planning_center_client.closed is True


def test_valid_planning_center_selection_loads_the_requested_candidate(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        "stagepilot.api.routes._current_local_date",
        lambda _timezone_name: TARGET_DATE,
    )
    planning_center_client = FakePlanningCenterClient()
    app = create_app(
        planning_center_settings(),
        planning_center_client_factory=RecordingPlanningCenterFactory(planning_center_client),
        planning_center_today_provider=fixed_today,
    )

    with TestClient(app) as client:
        pending = client.get("/api/v1/planning-center/plans/pending-selection")
        response = client.post(
            "/api/v1/planning-center/plans/select",
            json={"plan_id": "plan-early"},
        )
        health = client.get("/api/v1/health")

    assert response.status_code == 200
    assert pending.status_code == 200
    assert pending.json()["pending"] is True
    assert [value["id"] for value in pending.json()["candidates"]] == [
        "plan-early",
        "plan-late",
    ]
    assert health.status_code == 200
    assert health.json()["status"] == "healthy"
    body = response.json()
    assert body["accepted"] is True
    assert body["message"] == 'Loaded "Sunday Worship — 9:00".'
    assert body["state"]["service_load"]["status"] == ServiceLoadStatus.LOADED
    assert body["state"]["service_load"]["candidates"] == []
    assert body["state"]["plan"]["id"] == "plan-early"
    assert body["state"]["plan"]["songs"][0]["title"] == "Battle Belongs"
    assert planning_center_client.selected_plan_ids == [None, "plan-early"]
    assert planning_center_client.closed is True


def test_planning_center_reload_endpoint_dispatches_the_reload_action() -> None:
    app = create_app(Settings(demo_mode=True))

    with TestClient(app) as client:
        response = client.post("/api/v1/planning-center/plan/reload")

    assert response.status_code == 200
    assert response.json()["action"] == "reload_plan"
    assert response.json()["accepted"] is True


def _ready_production_plan() -> ServicePlan:
    return ServicePlan(
        id="plan-1",
        title="Sunday Worship",
        date=TARGET_DATE,
        service_type="Weekend Services",
        songs=[
            Song(
                id="item-1",
                title="Battle Belongs",
                duration_seconds=281,
                order=1,
            )
        ],
    )


def _ready_production_state() -> ApplicationState:
    return ApplicationState(
        planning_center_status=ConnectionStatus.CONNECTED,
        service_load=ServiceLoadState(
            status=ServiceLoadStatus.LOADED,
            target_date=TARGET_DATE,
        ),
        plan=_ready_production_plan(),
    )


@pytest.mark.parametrize(
    "state",
    [
        _ready_production_state().model_copy(
            update={"planning_center_status": ConnectionStatus.DISCONNECTED},
            deep=True,
        ),
        _ready_production_state().model_copy(
            update={
                "service_load": ServiceLoadState(
                    status=ServiceLoadStatus.NOT_FOUND,
                    target_date=TARGET_DATE,
                )
            },
            deep=True,
        ),
        _ready_production_state().model_copy(
            update={
                "service_load": ServiceLoadState(
                    status=ServiceLoadStatus.NOT_FOUND,
                    target_date=TARGET_DATE,
                    is_stale=True,
                )
            },
            deep=True,
        ),
        _ready_production_state().model_copy(update={"plan": None}, deep=True),
        _ready_production_state().model_copy(
            update={
                "plan": _ready_production_plan().model_copy(update={"date": date(2026, 7, 11)})
            },
            deep=True,
        ),
        _ready_production_state().model_copy(
            update={"plan": _ready_production_plan().model_copy(update={"songs": []})},
            deep=True,
        ),
    ],
    ids=[
        "disconnected",
        "not-loaded",
        "stale",
        "missing-plan",
        "wrong-date",
        "no-songs",
    ],
)
def test_production_service_readiness_rejects_unusable_state(
    state: ApplicationState,
) -> None:
    assert _production_service_ready(state, TARGET_DATE) is False


def test_production_service_readiness_accepts_current_loaded_plan() -> None:
    assert _production_service_ready(_ready_production_state(), TARGET_DATE) is True


def test_production_service_readiness_accepts_clean_upcoming_plan() -> None:
    upcoming_date = date(2026, 7, 14)
    state = _ready_production_state().model_copy(
        update={
            "service_load": ServiceLoadState(
                status=ServiceLoadStatus.LOADED,
                target_date=upcoming_date,
            ),
            "plan": _ready_production_plan().model_copy(
                update={"date": upcoming_date},
                deep=True,
            ),
        },
        deep=True,
    )

    assert _production_service_ready(state, TARGET_DATE) is True


def test_production_service_readiness_rejects_loaded_past_plan() -> None:
    assert (
        _production_service_ready(
            _ready_production_state(),
            date(2026, 7, 13),
        )
        is False
    )


def test_invalid_planning_center_candidate_returns_conflict_without_loading() -> None:
    planning_center_client = FakePlanningCenterClient()
    app = create_app(
        planning_center_settings(),
        planning_center_client_factory=RecordingPlanningCenterFactory(planning_center_client),
        planning_center_today_provider=fixed_today,
    )

    with TestClient(app) as client:
        response = client.post(
            "/api/v1/planning-center/plan-selection",
            json={"plan_id": "plan-not-offered"},
        )

    assert response.status_code == 409
    assert response.json() == {
        "detail": "The selected plan is not a current Planning Center candidate."
    }
    assert planning_center_client.selected_plan_ids == [None]


def test_planning_center_credentials_are_absent_from_all_public_responses() -> None:
    planning_center_client = FakePlanningCenterClient()
    app = create_app(
        planning_center_settings(),
        planning_center_client_factory=RecordingPlanningCenterFactory(planning_center_client),
        planning_center_today_provider=fixed_today,
    )

    with TestClient(app) as client:
        responses = [
            client.get("/api/v1/health"),
            client.get("/api/v1/state"),
            client.post(
                "/api/v1/planning-center/plan-selection",
                json={"plan_id": "plan-not-offered"},
            ),
            client.post(
                "/api/v1/planning-center/plan-selection",
                json={"plan_id": "plan-late"},
            ),
        ]

    assert [response.status_code for response in responses] == [200, 200, 409, 200]
    public_text = "\n".join(response.text for response in responses)

    assert PRIVATE_APP_ID not in public_text
    assert PRIVATE_SECRET not in public_text


def test_websocket_sends_initial_and_updated_full_state() -> None:
    app = create_app(Settings(demo_mode=True))

    with TestClient(app) as client, client.websocket_connect("/ws") as websocket:
        initial = websocket.receive_json()
        client.post("/api/v1/actions/start_next")
        updated = websocket.receive_json()
        while updated["data"]["current_song"] is None:
            updated = websocket.receive_json()

    assert initial["type"] == "state.snapshot"
    assert initial["data"]["plan"]["title"] == "Sunday Worship — Demo"
    assert updated["type"] == "state.snapshot"
    assert updated["data"]["current_song"]["title"] == "Battle Belongs"


def test_health_reports_when_the_backend_started() -> None:
    """The StagePilot panel shows Running since: a read-only UTC timestamp of the process start."""
    from datetime import UTC, datetime

    with TestClient(create_app(Settings(demo_mode=True))) as client:
        started = client.get("/api/v1/health").json()["started_at"]
    parsed = datetime.fromisoformat(started)
    assert parsed.tzinfo is not None
    assert 0 <= (datetime.now(UTC) - parsed).total_seconds() < 120
