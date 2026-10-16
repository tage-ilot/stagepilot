"""Reconcile saved integration settings once desktop startup has settled."""

from __future__ import annotations

import asyncio
from typing import Protocol

from stagepilot.core.config import LightsSettings, ProPresenterSettings
from stagepilot.core.lights import LightsController
from stagepilot.core.logging import get_logger
from stagepilot.core.midi import MidiController
from stagepilot.core.propresenter import ProPresenterController
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus


class PlanningCenterStartupController(Protocol):
    async def refresh_now(self) -> None: ...


class StartupPluginManager(Protocol):
    async def restart_failed(self) -> None: ...


class StartupActivationService:
    """Repeat safe discovery operations after vendor applications finish starting."""

    def __init__(
        self,
        *,
        plugin_manager: StartupPluginManager,
        state_store: StateStore,
        midi: MidiController | None,
        propresenter: ProPresenterController | None,
        propresenter_settings: ProPresenterSettings,
        lights: LightsController | None,
        lights_settings: LightsSettings,
        planning_center: PlanningCenterStartupController | None,
    ) -> None:
        self._plugin_manager = plugin_manager
        self._state_store = state_store
        self._midi = midi
        self._propresenter = propresenter
        self._propresenter_settings = propresenter_settings
        self._lights = lights
        self._lights_settings = lights_settings
        self._planning_center = planning_center
        self._logger = get_logger("startup_activation")

    def set_propresenter(
        self, propresenter: ProPresenterController | None, settings: ProPresenterSettings
    ) -> None:
        """Follow a live Presentation change so no stale plugin or settings are kept."""

        self._propresenter = propresenter
        self._propresenter_settings = settings

    async def run(self) -> None:
        """Retry failed plugins, then reconcile configured integrations concurrently."""

        try:
            await self._plugin_manager.restart_failed()
        except Exception:
            self._logger.exception("startup_plugin_retry_failed")

        operations = {
            "midi": self._activate_midi(),
            "propresenter": self._activate_propresenter(),
            "lights": self._activate_lights(),
            "planning_center": self._activate_planning_center(),
        }
        results = await asyncio.gather(*operations.values(), return_exceptions=True)
        for integration, result in zip(operations, results, strict=True):
            if isinstance(result, BaseException):
                self._logger.warning(
                    "startup_integration_activation_failed",
                    integration=integration,
                    error_type=type(result).__name__,
                )

    async def _activate_midi(self) -> None:
        if self._midi is not None:
            await self._midi.input_snapshot(refresh=True)

    async def _activate_propresenter(self) -> None:
        if self._propresenter is None:
            return
        snapshot = await self._propresenter.snapshot()
        if snapshot.connection_status is not ConnectionStatus.CONNECTED or not snapshot.timer_found:
            await self._propresenter.reconfigure(self._propresenter_settings)

    async def _activate_lights(self) -> None:
        if self._lights is None or not self._lights_settings.enabled:
            return
        snapshot = await self._lights.snapshot()
        if snapshot.connection_status is not ConnectionStatus.CONNECTED:
            await self._lights.reconfigure(self._lights_settings)

    async def _activate_planning_center(self) -> None:
        if self._planning_center is None:
            return
        state = await self._state_store.snapshot()
        if state.planning_center_status is not ConnectionStatus.CONNECTED:
            await self._planning_center.refresh_now()
