# Plugin development

Plugins isolate external systems from StagePilot's domain model. They are not a
general-purpose extension or code-loading mechanism in the foundation release.
A plugin is backend code reviewed and shipped with StagePilot.

## Responsibilities

A plugin may:

- translate vendor input into typed domain events;
- subscribe to typed events and perform one integration's side effects;
- expose validated, non-secret configuration status;
- report structured health and a useful last error; and
- own an integration client, reconnect policy, and cleanup.

A plugin must not:

- import or directly invoke another integration plugin;
- place vendor-specific logic in the event bus or UI;
- expose secrets in events, state, logs, or API responses;
- block the asyncio event loop with synchronous I/O;
- retry forever without backoff and shutdown cancellation; or
- let an expected integration failure terminate the application.

## Lifecycle

Plugins implement the core lifecycle contract and are constructed through the
plugin manager with explicit dependencies:

```python
class Plugin:
    async def start(self) -> None: ...
    async def stop(self) -> None: ...
    async def health(self) -> PluginHealth: ...
```

`start` registers subscriptions and opens resources. It should either become
ready, report degraded health, or raise a structured startup error that the
manager can isolate. `stop` is idempotent, unsubscribes handlers, cancels owned
tasks, and closes resources. Health checks should be fast and side-effect free.

The concrete base class and health model in `backend/src/stagepilot/core` are the
source of truth. This document describes the boundary, not a separate API.

## Events and state

Prefer a domain event such as `song.started` over `midi.note_100_received` when
other plugins care about the meaning rather than the transport. Preserve useful
source and correlation metadata for troubleshooting. Add a new payload model
instead of passing arbitrary dictionaries.

Plugins publish facts. Domain services or state reducers enforce shared rules
such as current-song selection, first/last bounds, duration validity, and
duplicate protection. A plugin can project connection and health information but
must not maintain a second authoritative copy of service navigation state.

## Suggested layout

```text
backend/src/stagepilot/plugins/example/
  __init__.py
  client.py          typed vendor transport
  config.py          validated plugin configuration
  models.py          vendor-specific internal models
  plugin.py          lifecycle and event translation

backend/tests/plugins/example/
  test_client.py
  test_plugin.py
```

Use fewer files when the integration is small. Avoid empty abstractions created
only to match this example.

## Reliability checklist

- Give every network operation an explicit timeout.
- Retry only transient failures, with a limit or backoff and cancellation.
- Make shutdown safe during connection and retry work.
- Bound queues and recent-message buffers.
- Treat malformed vendor input as data errors, not process errors.
- Re-discover cached remote identifiers after a peer restart.
- Make duplicate events safe and observable.
- Log operation, component, outcome, and correlation context without secrets.
- Report degraded or failed health with an actionable message.

## Testing checklist

Use a typed fake client or mocked transport; tests must not use real accounts or
equipment. Cover startup, shutdown, health, recognized input, ignored input,
timeouts, malformed responses, reconnects, event publication, subscription
cleanup, and failure isolation. Assert the order of side effects when order is a
safety property, as it is for ProPresenter's stop/set/reset/start sequence.

Run the backend quality checks documented in [../CONTRIBUTING.md](../CONTRIBUTING.md)
before submitting a plugin change.

## Changing plugins while StagePilot runs

`PluginManager.add`, `remove` and `replace` start, stop and swap a plugin without restarting
StagePilot. They are serialised by one lock, a plugin whose start fails is reported as an error
(`PLUGIN_FAILED`) and is not left registered, and other plugins are never touched.

Services uses this: `ServiceSourceController` (`services/service_source.py`) swaps the Demo and
Planning Center plugins when `service_source` is saved, and `ServicePluginFactory` is the single
place that builds the Services plugin for both startup and live switches. A switch is refused with
HTTP 409 while the timer is running, and a source pinned by `STAGEPILOT_SERVICE_SOURCE` (or
`STAGEPILOT_DEMO_MODE`) cannot be changed from the dashboard. Changing the Planning Center app id,
secret or sign-in rebuilds the client in place; no Services change needs a restart.
