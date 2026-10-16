"""Live Services source management: Demo <-> Planning Center without a restart."""

from __future__ import annotations

from collections.abc import Callable

from stagepilot.core.config import MidiSource, ServiceSource, Settings, TimerOutput
from stagepilot.core.event_bus import EventBus
from stagepilot.core.plan_cache import PlanCacheStore
from stagepilot.core.plugin import Plugin, PluginManager
from stagepilot.core.settings import SettingsService
from stagepilot.core.state import StateStore
from stagepilot.models.state import ApplicationState, PluginHealth, PluginStatus, TimerStatus
from stagepilot.plugins.demo import DemoPlugin
from stagepilot.plugins.planning_center import (
    PlanningCenterClientFactory,
    PlanningCenterPlugin,
    TodayProvider,
)
from stagepilot.services.state_service import StateService

DEMO_PLUGIN = "demo"
PLANNING_CENTER_PLUGIN = "planning_center"
BUSY_MESSAGE = "Stop the timer before switching between Demo and Planning Center."
CREDENTIAL_BUSY_MESSAGE = "Stop the timer before changing the Planning Center connection."


class ServiceSourceBusyError(Exception):
    """The show is running, so the Services connection cannot be changed now."""


class ServicePluginFactory:
    """Builds the Services plugin for a source; used by startup and by live switches."""

    def __init__(
        self,
        event_bus: EventBus,
        state_store: StateStore,
        *,
        client_factory: PlanningCenterClientFactory | None,
        today_provider: TodayProvider | None,
        plan_cache_store: PlanCacheStore,
    ) -> None:
        self._event_bus = event_bus
        self._state_store = state_store
        self._client_factory = client_factory
        self._today_provider = today_provider
        self._plan_cache_store = plan_cache_store

    def build(self, settings: Settings) -> DemoPlugin | PlanningCenterPlugin:
        modes = settings.integration_modes
        if modes.service_source is ServiceSource.DEMO:
            return DemoPlugin(
                self._event_bus,
                self._state_store,
                simulate_midi=modes.midi_source is MidiSource.SIMULATED,
                simulate_propresenter=modes.timer_output is TimerOutput.SIMULATED,
            )
        return PlanningCenterPlugin(
            self._event_bus,
            self._state_store,
            settings.planning_center,
            timezone_name=settings.timezone,
            client_factory=self._client_factory,
            today_provider=self._today_provider,
            plan_cache_store=self._plan_cache_store,
        )


class ServiceSourceController:
    """Switch or rebuild the Services plugin while StagePilot keeps running."""

    def __init__(
        self,
        *,
        plugin_manager: PluginManager,
        factory: ServicePluginFactory,
        settings_service: SettingsService,
        state_store: StateStore,
        state_service: StateService,
        on_planning_center_changed: Callable[[PlanningCenterPlugin | None], None],
    ) -> None:
        self._manager = plugin_manager
        self._factory = factory
        self._settings_service = settings_service
        self._state_store = state_store
        self._state_service = state_service
        self._on_changed = on_planning_center_changed

    @property
    def current_source(self) -> ServiceSource:
        if self._manager.has(DEMO_PLUGIN):
            return ServiceSource.DEMO
        return ServiceSource.PLANNING_CENTER

    async def ensure_idle(self, message: str = BUSY_MESSAGE) -> None:
        state: ApplicationState = await self._state_store.snapshot()
        if state.timer.status is TimerStatus.RUNNING:
            raise ServiceSourceBusyError(message)

    async def sync(self) -> PluginHealth | None:
        """Make the running Services plugin match the saved source; no-op when it already does."""

        settings = self._settings_service.effective_runtime_settings()
        target = settings.integration_modes.service_source
        if target is self.current_source and (
            target is ServiceSource.DEMO or self._manager.has(PLANNING_CENTER_PLUGIN)
        ):
            return None
        await self.ensure_idle()
        return await self._swap(settings)

    async def _swap(self, settings: Settings) -> PluginHealth:
        previous = self._manager.get(DEMO_PLUGIN) or self._manager.get(PLANNING_CENTER_PLUGIN)
        new_plugin: Plugin = self._factory.build(settings)
        switching_source = previous is None or previous.name != new_plugin.name
        if previous is not None:
            await self._manager.remove(previous.name)
            if previous.name == PLANNING_CENTER_PLUGIN:
                self._on_changed(None)
            await self._forget(previous.name, clear_service=switching_source)
        health = await self._manager.add(new_plugin)
        if isinstance(new_plugin, PlanningCenterPlugin) and (health.status is PluginStatus.RUNNING):
            self._on_changed(new_plugin)
        elif isinstance(new_plugin, PlanningCenterPlugin):
            self._on_changed(None)
        return health

    async def _forget(self, plugin_name: str, *, clear_service: bool) -> None:
        if clear_service:
            await self._state_service.clear_service()

        def mutation(state: ApplicationState) -> None:
            state.plugins.pop(plugin_name, None)

        await self._state_store.mutate(mutation)
