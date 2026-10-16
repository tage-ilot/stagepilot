from __future__ import annotations

from dataclasses import dataclass, field

from fastapi.testclient import TestClient

from stagepilot.core.config import DemoSettings, ProPresenterSettings, Settings
from stagepilot.main import create_app
from stagepilot.plugins.propresenter import (
    ProPresenterCountdown,
    ProPresenterIdentifier,
    ProPresenterLook,
    ProPresenterTimer,
)


def make_timer(
    *,
    name: str = "Song Countdown",
    uuid: str = "timer-uuid",
    index: int = 0,
    countdown: bool = True,
) -> ProPresenterTimer:
    return ProPresenterTimer(
        id=ProPresenterIdentifier(uuid=uuid, name=name, index=index),
        countdown=ProPresenterCountdown(duration=60) if countdown else None,
        state="stopped",
    )


@dataclass
class FakeClient:
    timers: list[ProPresenterTimer] = field(default_factory=lambda: [make_timer()])
    closed: bool = False
    looks: list[ProPresenterLook] = field(
        default_factory=lambda: [
            ProPresenterLook(
                id=ProPresenterIdentifier(uuid="look-worship", name="Worship", index=0)
            )
        ]
    )
    current: ProPresenterLook | None = None
    triggered_look_ids: list[str] = field(default_factory=list)
    stale_current_look_reads: int = 0
    current_look_reads_after_trigger: int = 0

    async def close(self) -> None:
        self.closed = True

    async def list_timers(self) -> list[ProPresenterTimer]:
        return list(self.timers)

    async def find_timer(self, name: str) -> ProPresenterTimer:
        return next(timer for timer in self.timers if timer.id.name == name)

    async def list_looks(self) -> list[ProPresenterLook]:
        return list(self.looks)

    async def current_look(self) -> ProPresenterLook:
        if (
            self.triggered_look_ids
            and self.current_look_reads_after_trigger < self.stale_current_look_reads
        ):
            self.current_look_reads_after_trigger += 1
            return ProPresenterLook(
                id=ProPresenterIdentifier(uuid="old-live-look", name="Previous", index=99)
            )
        return self.current or self.looks[0]

    async def trigger_look(self, look_id: str) -> None:
        self.triggered_look_ids.append(look_id)
        selected = next(look for look in self.looks if look.id.uuid == look_id)
        self.current = ProPresenterLook(
            id=ProPresenterIdentifier(
                uuid="unique-live-look",
                name=selected.id.name,
                index=selected.id.index,
            )
        )

    async def stop_timer(self, _timer_id: str) -> None:
        return None

    async def set_timer_duration(
        self,
        timer: ProPresenterTimer,
        duration_seconds: int,
    ) -> ProPresenterTimer:
        return timer.model_copy(
            update={"countdown": ProPresenterCountdown(duration=duration_seconds)}
        )

    async def reset_timer(self, _timer_id: str) -> None:
        return None

    async def start_timer(self, _timer_id: str) -> None:
        return None


class RecordingFactory:
    def __init__(self, clients: list[FakeClient]) -> None:
        self.clients = clients
        self.settings: list[ProPresenterSettings] = []

    def __call__(self, settings: ProPresenterSettings) -> FakeClient:
        self.settings.append(settings)
        return self.clients.pop(0)


def enabled_settings() -> Settings:
    return Settings(
        demo_mode=True,
        demo=DemoSettings(simulate_propresenter=False),
        propresenter=ProPresenterSettings(
            enabled=True,
            health_check_interval_seconds=300,
        ),
    )


def test_propresenter_status_and_timer_refresh_are_exposed() -> None:
    fake = FakeClient(
        timers=[
            make_timer(),
            make_timer(name="Message Timer", uuid="other", index=1, countdown=False),
        ]
    )
    app = create_app(
        enabled_settings(),
        propresenter_client_factory=lambda _settings: fake,
    )

    with TestClient(app) as client:
        status = client.get("/api/v1/propresenter")
        assert status.status_code == 200
        payload = status.json()
        assert payload["enabled"] is True
        assert payload["connection_status"] == "connected"
        assert payload["timer_found"] is True
        assert payload["selected_timer_id"] == "timer-uuid"
        assert [timer["name"] for timer in payload["timers"]] == [
            "Song Countdown",
            "Message Timer",
        ]

        refreshed = client.post("/api/v1/propresenter/timers/refresh")
        assert refreshed.status_code == 200
        assert refreshed.json()["accepted"] is True
        assert refreshed.json()["propresenter"]["timer_found"] is True


def test_session_settings_recreate_client_and_report_missing_timer() -> None:
    first = FakeClient()
    second = FakeClient(timers=[make_timer(name="Different Timer")])
    factory = RecordingFactory([first, second])
    app = create_app(enabled_settings(), propresenter_client_factory=factory)

    with TestClient(app) as client:
        response = client.post(
            "/api/v1/propresenter/settings",
            json={
                "host": "192.168.4.40",
                "port": 1026,
                "timer_name": "Missing Timer",
                "look_id": "look-worship",
                "request_timeout_seconds": 4,
            },
        )
        assert response.status_code == 200
        payload = response.json()
        assert payload["accepted"] is False
        assert payload["propresenter"]["connection_status"] == "connected"
        assert payload["propresenter"]["timer_found"] is False
        assert payload["propresenter"]["host"] == "192.168.4.40"
        assert payload["propresenter"]["port"] == 1026
        assert "not found" in payload["message"].lower()

    assert first.closed is True
    assert len(factory.settings) == 2
    assert factory.settings[-1].timer_name == "Missing Timer"
    assert second.triggered_look_ids == ["look-worship"]
    persisted = app.state.runtime.settings_service.snapshot().propresenter
    assert persisted.host == "192.168.4.40"
    assert persisted.port == 1026
    assert persisted.timer_name == "Missing Timer"
    assert persisted.request_timeout_seconds == 4


def test_saved_look_tolerates_delayed_current_look_confirmation() -> None:
    first = FakeClient()
    second = FakeClient(stale_current_look_reads=2)
    factory = RecordingFactory([first, second])
    app = create_app(enabled_settings(), propresenter_client_factory=factory)

    with TestClient(app) as client:
        response = client.post(
            "/api/v1/propresenter/settings",
            json={
                "host": "127.0.0.1",
                "port": 1025,
                "timer_name": "Song Countdown",
                "look_id": "look-worship",
                "request_timeout_seconds": 3,
            },
        )

    assert response.status_code == 200
    assert response.json()["accepted"] is True
    assert "Look was not applied" not in response.json()["message"]
    assert second.triggered_look_ids == ["look-worship"]
    assert second.current_look_reads_after_trigger == 2


def test_disabled_propresenter_returns_safe_status() -> None:
    app = create_app(Settings())
    with TestClient(app) as client:
        response = client.get("/api/v1/propresenter")
        saved = client.post(
            "/api/v1/propresenter/settings",
            json={
                "host": "192.168.1.20",
                "port": 1026,
                "timer_name": "Worship",
                "look_id": None,
                "request_timeout_seconds": 4,
            },
        )
        assert response.status_code == 200
        payload = response.json()
        assert payload["enabled"] is False
        assert payload["connection_status"] == "disconnected"
        assert payload["timer_found"] is False
        assert saved.status_code == 200
        assert saved.json()["accepted"] is True
        assert "restart" not in saved.json()["message"].lower()

    persisted = app.state.runtime.settings_service.snapshot().propresenter
    assert persisted.host == "192.168.1.20"
    assert persisted.port == 1026
    assert persisted.timer_name == "Worship"
