"""Plugin lifecycle contracts and failure-isolated plugin manager."""

from __future__ import annotations

import asyncio
from abc import ABC, abstractmethod
from datetime import UTC, datetime
from typing import Any

from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import EventType, PluginPayload, new_event
from stagepilot.core.logging import get_logger
from stagepilot.core.state import StateStore
from stagepilot.models.state import PluginHealth, PluginStatus


class Plugin(ABC):
    """Base contract implemented by every StagePilot integration."""

    name: str
    version: str

    def __init__(self, event_bus: EventBus, state_store: StateStore) -> None:
        self.event_bus = event_bus
        self.state_store = state_store

    @abstractmethod
    async def start(self) -> None:
        """Initialize resources and event subscriptions."""

    @abstractmethod
    async def stop(self) -> None:
        """Release resources and event subscriptions."""

    @abstractmethod
    async def health(self) -> PluginHealth:
        """Return the plugin's current health."""


class ReconcileLock:
    """Task-reentrant async lock serialising live connection changes.

    Held around read-saved-settings -> decide -> act -> publish so concurrent saves apply one at a
    time and the last writer wins. Reentrant per task, so a route holding it can call a
    controller's ``sync()`` which takes it again. Separate from the PluginManager lifecycle lock,
    which ``add``/``remove`` take internally.
    """

    def __init__(self) -> None:
        self._lock = asyncio.Lock()
        self._owner: asyncio.Task[Any] | None = None
        self._depth = 0

    async def __aenter__(self) -> None:
        task = asyncio.current_task()
        if self._owner is not None and self._owner is task:
            self._depth += 1
            return
        await self._lock.acquire()
        self._owner = task
        self._depth = 1

    async def __aexit__(self, *exc: object) -> None:
        self._depth -= 1
        if self._depth == 0:
            self._owner = None
            self._lock.release()


class PluginManager:
    """Register and supervise plugins without allowing one failure to stop others."""

    def __init__(self, event_bus: EventBus) -> None:
        self._event_bus = event_bus
        self._plugins: dict[str, Plugin] = {}
        self._health: dict[str, PluginHealth] = {}
        self._logger = get_logger("plugin_manager")
        # One runtime lifecycle change (add / remove / replace) at a time.
        self._lifecycle_lock = asyncio.Lock()
        # Outer lock for whole reconcile sequences in the live controllers and routes.
        self.reconcile_lock = ReconcileLock()

    def has(self, name: str) -> bool:
        return name in self._plugins

    def get(self, name: str) -> Plugin | None:
        return self._plugins.get(name)

    async def add(self, plugin: Plugin) -> PluginHealth:
        """Register and start a plugin while the application is running.

        A failed start never leaves the plugin registered: the failure is reported through the
        usual PLUGIN_FAILED event and the returned ERROR health, and other plugins are untouched.
        """

        async with self._lifecycle_lock:
            return await self._add_locked(plugin)

    async def remove(self, name: str) -> PluginHealth | None:
        """Stop and unregister a plugin; unknown names return ``None``."""

        async with self._lifecycle_lock:
            return await self._remove_locked(name)

    async def replace(self, name: str, plugin: Plugin) -> PluginHealth:
        """Swap the named plugin for ``plugin`` as one serialised change.

        The old plugin is stopped and unregistered first. If the new plugin fails to start it is
        not left registered either, so the failure is visible (ERROR health, PLUGIN_FAILED event)
        and no stale plugin keeps running with settings that were just replaced.
        """

        async with self._lifecycle_lock:
            await self._remove_locked(name)
            return await self._add_locked(plugin)

    async def _add_locked(self, plugin: Plugin) -> PluginHealth:
        self.register(plugin)
        await self._start_plugin(plugin)
        health = self._health[plugin.name]
        if health.status is PluginStatus.RUNNING:
            return health.model_copy(deep=True)
        failed = health.model_copy(deep=True)
        try:
            await plugin.stop()
        except Exception:  # best-effort cleanup of a plugin that never started
            self._logger.warning("plugin_cleanup_after_failed_start_failed", plugin=plugin.name)
        del self._plugins[plugin.name]
        del self._health[plugin.name]
        return failed

    async def _remove_locked(self, name: str) -> PluginHealth | None:
        plugin = self._plugins.get(name)
        if plugin is None:
            return None
        await self._stop_plugin(plugin)
        result = self._health[name].model_copy(deep=True)
        del self._plugins[name]
        del self._health[name]
        return result

    def register(self, plugin: Plugin) -> None:
        if plugin.name in self._plugins:
            msg = f"Plugin already registered: {plugin.name}"
            raise ValueError(msg)
        self._plugins[plugin.name] = plugin
        self._health[plugin.name] = PluginHealth(
            name=plugin.name,
            version=plugin.version,
            status=PluginStatus.STOPPED,
        )

    async def start_all(self) -> None:
        await asyncio.gather(*(self._start_plugin(plugin) for plugin in self._plugins.values()))

    async def stop_all(self) -> None:
        await asyncio.gather(
            *(self._stop_plugin(plugin) for plugin in reversed(self._plugins.values()))
        )

    async def restart_failed(self) -> None:
        """Retry plugins whose first startup attempt failed after dependencies settle."""

        failed = [
            plugin
            for plugin in self._plugins.values()
            if self._health[plugin.name].status is PluginStatus.ERROR
        ]
        for plugin in failed:
            await self._stop_plugin(plugin)
            if self._health[plugin.name].status is PluginStatus.STOPPED:
                await self._start_plugin(plugin)

    async def health(self) -> list[PluginHealth]:
        return list(
            await asyncio.gather(
                *(self._plugin_health(plugin) for plugin in self._plugins.values())
            )
        )

    async def _plugin_health(self, plugin: Plugin) -> PluginHealth:
        managed = self._health[plugin.name]
        if managed.status is not PluginStatus.RUNNING:
            return managed.model_copy(deep=True)

        try:
            reported = await plugin.health()
        except Exception as exc:  # one failed health probe must not hide the other plugins
            self._logger.warning(
                "plugin_health_failed",
                plugin=plugin.name,
                error_type=type(exc).__name__,
            )
            current = self._health[plugin.name]
            if current.status is not PluginStatus.RUNNING:
                return current.model_copy(deep=True)
            return PluginHealth(
                name=plugin.name,
                version=plugin.version,
                status=PluginStatus.ERROR,
                last_error="Plugin health check failed.",
                last_activity_at=current.last_activity_at,
            )

        current = self._health[plugin.name]
        if current.status is not PluginStatus.RUNNING:
            return current.model_copy(deep=True)
        return PluginHealth(
            name=plugin.name,
            version=plugin.version,
            status=reported.status,
            last_error=reported.last_error,
            last_activity_at=reported.last_activity_at or current.last_activity_at,
        )

    async def _start_plugin(self, plugin: Plugin) -> None:
        await self._set_health(plugin, PluginStatus.STARTING)
        try:
            await plugin.start()
        except Exception as exc:  # plugin isolation is the purpose of this boundary
            self._logger.exception("plugin_start_failed", plugin=plugin.name)
            await self._set_health(plugin, PluginStatus.ERROR, str(exc))
            await self._event_bus.publish(
                new_event(
                    EventType.PLUGIN_FAILED,
                    source="plugin_manager",
                    payload=PluginPayload(
                        name=plugin.name,
                        version=plugin.version,
                        status=PluginStatus.ERROR,
                        error=str(exc),
                    ),
                )
            )
            return
        await self._set_health(plugin, PluginStatus.RUNNING)

    async def _stop_plugin(self, plugin: Plugin) -> None:
        health = self._health[plugin.name]
        if health.status is PluginStatus.STOPPED:
            return
        await self._set_health(plugin, PluginStatus.STOPPING)
        try:
            await plugin.stop()
        except Exception as exc:  # shutdown must continue for remaining plugins
            self._logger.exception("plugin_stop_failed", plugin=plugin.name)
            await self._set_health(plugin, PluginStatus.ERROR, str(exc))
            return
        await self._set_health(plugin, PluginStatus.STOPPED)

    async def _set_health(
        self,
        plugin: Plugin,
        status: PluginStatus,
        error: str | None = None,
    ) -> None:
        health = PluginHealth(
            name=plugin.name,
            version=plugin.version,
            status=status,
            last_error=error,
            last_activity_at=datetime.now(UTC),
        )
        self._health[plugin.name] = health
        await self._event_bus.publish(
            new_event(
                EventType.PLUGIN_STATUS_CHANGED,
                source="plugin_manager",
                payload=PluginPayload(
                    name=plugin.name,
                    version=plugin.version,
                    status=status,
                    error=error,
                ),
            )
        )
