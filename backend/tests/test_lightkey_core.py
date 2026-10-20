"""Pure Lightkey recording/replay rules, driven by golden/lightkey_golden.json.

The JSON file is the contract for the Rust port: the same cases must pass there unchanged.
"""

from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, cast

import pytest
from pydantic import ValidationError

from stagepilot.core.config import LightkeySettings
from stagepilot.core.lightkey import (
    LightkeyEdge,
    LightkeyTake,
    OperatorHold,
    Verb,
    build_take_edges,
    effective_api_url,
    is_addressable,
    managed_cues,
    osc_address,
    replay_plan,
    state_at,
    take_is_worth_keeping,
)

GOLDEN: dict[str, Any] = json.loads(
    (Path(__file__).parent / "golden" / "lightkey_golden.json").read_text()
)
TAKE = LightkeyTake.model_validate(GOLDEN["take"])


@pytest.mark.parametrize("case", GOLDEN["state_at"])
def test_state_at(case: dict[str, Any]) -> None:
    assert state_at(TAKE, case["position"]) == set(case["active"])


def test_managed_cues() -> None:
    assert managed_cues(TAKE) == set(GOLDEN["managed"])


@pytest.mark.parametrize("case", GOLDEN["replay_plan"])
def test_replay_plan(case: dict[str, Any]) -> None:
    sync_now, later = replay_plan(TAKE, case["position"])
    assert [[c.cue, c.verb] for c in sync_now] == case["sync_now"]
    assert all(c.at_seconds == case["position"] for c in sync_now)
    assert [[c.at_seconds, c.cue, c.verb] for c in later] == case["later"]


def test_unaddressable_cue_names_are_dropped_not_sent() -> None:
    assert not is_addressable("Cue/Odd")
    sync_now, later = replay_plan(TAKE, 0)
    assert "Cue/Odd" not in {c.cue for c in sync_now} | {c.cue for c in later}


@pytest.mark.parametrize("cue,verb,expected", GOLDEN["osc_address"])
def test_osc_address(cue: str, verb: str, expected: str) -> None:
    assert osc_address(cue, cast(Verb, verb)) == expected


@pytest.mark.parametrize("case", GOLDEN["build_edges"], ids=lambda c: c["name"])
def test_build_take_edges(case: dict[str, Any]) -> None:
    initial, edges = build_take_edges(case["baseline"], case["observed"], case["duration"])
    assert initial == case["initial"]
    assert [e.model_dump() for e in edges] == case["edges"]


@pytest.mark.parametrize("covered,duration,expected", GOLDEN["worth_keeping"])
def test_worth_keeping(covered: float, duration: float, expected: bool) -> None:
    assert take_is_worth_keeping(covered, duration) is expected


@pytest.mark.parametrize("case", GOLDEN["operator_hold"], ids=lambda c: c["name"])
def test_operator_hold(case: dict[str, Any]) -> None:
    hold = OperatorHold()
    for step in case["steps"]:
        kind, cue, now = step[0], step[1], step[2]
        if kind == "sent":
            hold.note_sent(cue, now)
        elif kind == "change":
            assert hold.note_change(cue, now) is step[3]
        else:
            assert hold.is_held(cue, now) is step[3]


def test_edge_rules() -> None:
    with pytest.raises(ValidationError):
        LightkeyEdge(at_seconds=1)
    with pytest.raises(ValidationError):
        LightkeyEdge(at_seconds=1, added=["A"], removed=["A"])
    assert LightkeyEdge(at_seconds=1.23456, added=["A"]).at_seconds == 1.235


def test_take_sorts_its_edges() -> None:
    take = LightkeyTake(
        song_key="k",
        song_title="t",
        recorded_at=datetime(2026, 10, 20, tzinfo=UTC),
        duration_seconds=10,
        edges=[
            LightkeyEdge(at_seconds=5, added=["B"]),
            LightkeyEdge(at_seconds=1, added=["A"]),
        ],
    )
    assert [e.at_seconds for e in take.edges] == [1, 5]


@pytest.mark.parametrize(
    "raw,osc_host,expected",
    [
        ("", "10.30.0.211", "http://10.30.0.211:8770"),
        ("10.30.0.211", "x", "http://10.30.0.211:8770"),
        ("10.30.0.211:9000/", "x", "http://10.30.0.211:9000"),
        ("https://lights.local", "x", "https://lights.local:8770"),
    ],
)
def test_effective_api_url(raw: str, osc_host: str, expected: str) -> None:
    assert effective_api_url(LightkeySettings(api_url=raw), osc_host) == expected


@pytest.mark.parametrize("raw", ["http://a/b", "a b", "ftp://x/y z"])
def test_api_url_rejects_paths_and_spaces(raw: str) -> None:
    with pytest.raises(ValidationError):
        LightkeySettings(api_url=raw)


def test_replay_defaults_on() -> None:
    assert LightkeySettings().replay is True
