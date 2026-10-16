"""Live Presentation output management: simulated <-> ProPresenter without a restart."""

from __future__ import annotations

from collections.abc import Callable

from stagepilot.core.config import ProPresenterSettings, Settings, TimerOutput
from stagepilot.core.event_bus import EventBus
from stagepilot.core.plugin import PluginManager
from stagepilot.core.settings import PersistentSettings, SettingsService
from stagepilot.core.state import StateStore
from stagepilot.models.state import ApplicationState, PluginHealth, PluginStatus, TimerStatus
from stagepilot.plugins.demo import DemoPlugin
from stagepilot.plugins.propresenter import ProPresenterClientFactory, ProPresenterPlugin
from stagepilot.services.state_service import StateService

PROPRESENTER_PLUGIN = "propresenter"
DEMO_PLUGIN = "demo"
BUSY_MESSAGE = "Stop the timer before switching Presentation between simulated and ProPresenter."


class PresentationBusyError(Exception):
    """The show is running, so the Presentation output cannot be switched now."""


class PresentationPluginFactory:
    """Builds the ProPresenter plugin; used by startup and by live switches."""

    def __init__(
        self,
        event_bus: EventBus,
        state_store: StateStore,
        *,
        client_factory: ProPresenterClientFactory | None,
    ) -> None:
        self._event_bus = event_bus
        self._state_store = state_store
        self._client_factory = client_factory

    @staticmethod
    def wanted(settings: Settings | PersistentSettings) -> bool:
        """True when these settings put the timer on a real ProPresenter."""

        return (
            settings.propresenter.enabled
            and settings.integration_modes.timer_output is TimerOutput.PROPRESENTER
        )

    def build(self, settings: Settings) -> ProPresenterPlugin | None:
        if not self.wanted(settings):
            return None
        return ProPresenterPlugin(
            self._event_bus,
            self._state_store,
            settings.propresenter,
            client_factory=self._client_factory,
        )


class PresentationController:
    """Create, remove or reconfigure the ProPresenter plugin while StagePilot keeps running."""

    def __init__(
        self,
        *,
        plugin_manager: PluginManager,
        factory: PresentationPluginFactory,
        settings_service: SettingsService,
        state_store: StateStore,
        state_service: StateService,
        on_changed: Callable[[ProPresenterPlugin | None], None],
    ) -> None:
        self._manager = plugin_manager
        self._factory = factory
        self._settings_service = settings_service
        self._state_store = state_store
        self._state_service = state_service
        self._on_changed = on_changed
        self._applied: ProPresenterSettings | None = None

    def adopt(self, plugin: object | None, settings: ProPresenterSettings) -> None:
        """Record the settings the plugin built at startup is running with."""

        self._applied = settings if plugin is not None else None

    async def ensure_idle(self, message: str = BUSY_MESSAGE) -> None:
        state: ApplicationState = await self._state_store.snapshot()
        if state.timer.status is TimerStatus.RUNNING:
            raise PresentationBusyError(message)

    async def sync(self) -> PluginHealth | None:
        """Make the running Presentation output match the saved settings.

        Returns the new plugin's health when one was started, else ``None``.
        """

        settings = self._settings_service.effective_runtime_settings()
        wanted = self._factory.wanted(settings)
        present = self._manager.get(PROPRESENTER_PLUGIN)
        if wanted and isinstance(present, ProPresenterPlugin):
            if settings.propresenter != self._applied:
                await present.reconfigure(settings.propresenter)
                self._applied = settings.propresenter
            return None
        if not wanted and present is None:
            await self._set_simulated(True)
            return None
        await self.ensure_idle()
        if wanted:
            return await self._start(settings)
        await self._stop()
        return None

    async def _start(self, settings: Settings) -> PluginHealth:
        plugin = self._factory.build(settings)
        assert plugin is not None
        # The simulation stops owning the timer: forget whatever it left behind.
        await self._set_simulated(False)
        await self._state_service.clear_timer()
        health = await self._manager.add(plugin)
        if health.status is PluginStatus.RUNNING:
            self._applied = settings.propresenter
            self._on_changed(plugin)
        else:
            self._applied = None
            self._on_changed(None)
            await self._forget()
        return health

    async def _stop(self) -> None:
        await self._manager.remove(PROPRESENTER_PLUGIN)
        self._applied = None
        self._on_changed(None)
        await self._forget()
        await self._state_service.clear_timer()
        await self._set_simulated(True)

    async def _set_simulated(self, simulated: bool) -> None:
        demo = self._manager.get(DEMO_PLUGIN)
        if isinstance(demo, DemoPlugin):
            await demo.set_simulate_propresenter(simulated)

    async def _forget(self) -> None:
        def mutation(state: ApplicationState) -> None:
            state.plugins.pop(PROPRESENTER_PLUGIN, None)

        await self._state_store.mutate(mutation)
