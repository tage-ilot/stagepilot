"""The contract normaliser's "<today>" must not depend on the wall clock or host timezone."""

from __future__ import annotations

import sys
import time
from datetime import UTC, datetime
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "contract"))
import contract


@pytest.mark.parametrize("tz", ["UTC", "America/Los_Angeles", "Pacific/Auckland"])
@pytest.mark.parametrize(
    "now",
    [
        datetime(2026, 10, 20, 23, 30, tzinfo=UTC),
        datetime(2027, 3, 1, 0, 5, tzinfo=UTC),
    ],
)
def test_today_is_stable(monkeypatch: pytest.MonkeyPatch, tz: str, now: datetime) -> None:
    monkeypatch.setenv("TZ", tz)
    time.tzset()
    try:
        assert contract.today_values(now) == contract.today_values(now.replace(tzinfo=None))
        assert now.date().isoformat() in contract.today_values(now)
        assert contract.PINNED_TODAY in contract.today_values(now)
    finally:
        monkeypatch.undo()
        time.tzset()


def test_pinned_date_normalised_after_it_expires(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        contract, "today_values", lambda now=None: frozenset({"2030-01-01", contract.PINNED_TODAY})
    )
    out = contract.normalise({"target_date": "2026-10-18", "x": ["2030-01-01", "2029-12-31"]})
    assert out == {"target_date": "<today>", "x": ["<today>", "2029-12-31"]}


def test_pco_profile_pins_health_clock() -> None:
    import pco_app

    from stagepilot.api import routes

    saved = routes._current_local_date
    try:
        pco_app.create(1)
        assert routes._current_local_date("UTC") == pco_app.TODAY
    finally:
        routes._current_local_date = saved
