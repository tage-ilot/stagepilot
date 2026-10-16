"""PluginManager add/remove/replace while running."""

from __future__ import annotations

import asyncio

from stagepilot.core.event_bus import EventBus
from stagepilot.core.plugin import PluginManager
from stagepilot.core.state import StateStore
from stagepilot.models.state import PluginStatus
from test_plugin_manager import TestPlugin


def _manager() -> tuple[PluginManager, EventBus, StateStore]:
    bus = EventBus()
    return PluginManager(bus), bus, StateStore()


async def test_add_starts_plugin_and_reports_health() -> None:
    manager, bus, store = _manager()
    await manager.start_all()
    plugin = TestPlugin("late", bus, store, fail=False)
    health = await manager.add(plugin)
    assert health.status is PluginStatus.RUNNING
    assert plugin.started
    assert [h.name for h in await manager.health()] == ["late"]


async def test_failed_add_leaves_nothing_registered_and_spares_others() -> None:
    manager, bus, store = _manager()
    good = TestPlugin("good", bus, store, fail=False)
    manager.register(good)
    await manager.start_all()
    health = await manager.add(TestPlugin("bad", bus, store, fail=True))
    assert health.status is PluginStatus.ERROR
    assert health.last_error == "intentional plugin failure"
    assert not manager.has("bad")
    assert [h.name for h in await manager.health()] == ["good"]
    assert good.started
    # The name is free again for a corrected plugin.
    assert (await manager.add(TestPlugin("bad", bus, store, fail=False))).status is (
        PluginStatus.RUNNING
    )


async def test_remove_stops_and_unregisters() -> None:
    manager, bus, store = _manager()
    plugin = TestPlugin("gone", bus, store, fail=False)
    await manager.add(plugin)
    health = await manager.remove("gone")
    assert health is not None and health.status is PluginStatus.STOPPED
    assert not plugin.started
    assert await manager.health() == []
    assert await manager.remove("gone") is None


async def test_replace_swaps_plugin_and_failed_replace_registers_nothing() -> None:
    manager, bus, store = _manager()
    old = TestPlugin("svc", bus, store, fail=False)
    await manager.add(old)
    new = TestPlugin("svc", bus, store, fail=False)
    assert (await manager.replace("svc", new)).status is PluginStatus.RUNNING
    assert not old.started and new.started and manager.get("svc") is new

    broken = TestPlugin("svc", bus, store, fail=True)
    assert (await manager.replace("svc", broken)).status is PluginStatus.ERROR
    assert not manager.has("svc")
    assert not new.started


async def test_concurrent_lifecycle_changes_are_serialised() -> None:
    manager, bus, store = _manager()
    active = 0
    peak = 0

    class Slow(TestPlugin):
        async def start(self) -> None:
            nonlocal active, peak
            active += 1
            peak = max(peak, active)
            await asyncio.sleep(0.02)
            active -= 1
            await super().start()

    await asyncio.gather(*(manager.add(Slow(f"p{i}", bus, store, fail=False)) for i in range(5)))
    assert peak == 1
    assert len(await manager.health()) == 5
