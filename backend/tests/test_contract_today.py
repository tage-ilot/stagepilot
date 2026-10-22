"""The contract normaliser's "<today>" must not depend on the wall clock or host timezone."""

from __future__ import annotations

import importlib.util
import sys
import time
from datetime import UTC, datetime
from pathlib import Path
from types import ModuleType

import pytest

_CONTRACT_DIR = Path(__file__).resolve().parents[2] / "scripts" / "contract"
# The helper scripts import each other by bare name, so the folder stays on sys.path.
sys.path.insert(0, str(_CONTRACT_DIR))


def _load_contract() -> ModuleType:
    """Load scripts/contract/contract.py by path.

    A plain `import contract` is ambiguous: the repository root also has a `contract/` data
    folder (openapi.json, transcripts.json). mypy run from the repository root, as the release
    workflow and create-release.ps1 do, resolves that folder as a namespace package and reports
    every attribute as missing; run from backend/ (regular CI) it did not. Loading by path
    type-checks the same from both.
    """
    spec = importlib.util.spec_from_file_location("contract", _CONTRACT_DIR / "contract.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["contract"] = module
    spec.loader.exec_module(module)
    return module


contract: ModuleType = _load_contract()


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
