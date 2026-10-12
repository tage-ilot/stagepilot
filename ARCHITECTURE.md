# StagePilot architecture

## Desktop updater

The production Tauri shell owns update transport and signature verification
through official updater, process, and window-state plugins. The React
`UpdaterAdapter` is a narrow boundary over those APIs. `useUpdater` schedules
non-blocking checks and owns the state machine; header and modal components only
present normalized state.

The endpoint is restricted to the repository's HTTPS `latest.json`. The
window-state plugin restores the hidden `main` window before the existing
startup lifecycle unminimizes, shows, and focuses it. A version-bound local
marker differentiates an update relaunch from ordinary startup.

## Goals and constraints

StagePilot coordinates systems used in live production, where a single failed
integration must not freeze the UI or take down unrelated automation. The design
therefore favors typed events, observable state, bounded asynchronous work, and
small plugin boundaries over direct integration-to-integration calls.

The desktop application is one host for StagePilot, not the architecture's
center. The FastAPI backend and built frontend remain independently runnable for
headless and browser-based deployments.

## System context

```text
Planning Center API       Playback / MIDI       ProPresenter API
        |                        |                      ^
        v                        v                      |
  planning_center         midi_playback          propresenter
        plugin                plugin                 plugin
             \                 |                  /
              +---------- typed event bus -------+
                             |
                       application state
                             |
                  REST API + WebSocket stream
                             |
                   React dashboard / Tauri
```

An input plugin publishes a domain event such as `song.started`. Output plugins,
state projection, logs, and the WebSocket publisher can react independently. A
MIDI plugin must never know how to call ProPresenter.

## Backend boundaries

- **Core** owns typed events, the asynchronous event bus, application state,
  configuration contracts, plugin lifecycle contracts, and structured logging.
- **Plugins** isolate external systems. They translate external input into domain
  events and domain events into external side effects.
- **Services** coordinate domain workflows that do not belong to one external
  integration.
- **API** validates requests, invokes the same application actions used by
  plugins, and serializes safe state. It does not contain production logic.
- **Models** define stable API and domain data structures. Secrets use separate
  input models and are never included in public state.

Dependencies point inward: plugins and API may depend on core contracts; core
does not import plugins, FastAPI routes, or frontend concepts.

## Typed event model

Events have a stable name, timestamp, source, correlation identifier where
useful, and a typed payload. Arbitrary dictionaries should not cross core
boundaries. Event handlers are asynchronous, and a handler failure is recorded
without preventing other subscribers from running.

Commands from the dashboard and MIDI inputs converge on the same event-driven
application actions. This prevents manual controls from developing behavior that
differs from the live cue path.

The event bus is process-local in the initial architecture. Persisted queues or
distributed brokers should only be introduced when deployment requirements make
them necessary.

## Plugin lifecycle and isolation

Plugins expose an asynchronous lifecycle:

```python
class Plugin:
    async def start(self) -> None: ...
    async def stop(self) -> None: ...
    async def health(self) -> PluginHealth: ...
```

The plugin manager controls startup and shutdown, records health, and contains
failures at plugin boundaries. Network calls require explicit timeouts. Retries
must be bounded or use backoff and must stop during application shutdown.
Blocking MIDI or vendor-library work belongs in a worker thread or executor.

The first MIDI Playback slice uses Mido with the RtMidi backend. Port discovery,
open, close, and other potentially blocking backend operations run in one
dedicated worker thread. The vendor callback normalizes supported note messages
and hands them back to the asyncio loop through a bounded queue; application
actions never execute in the vendor callback thread. A supervisor re-enumerates
the exact configured input name and reconnects with capped backoff. Each open
port receives a connection generation identifier so late callbacks from a
closed port are ignored. Held-note latching and monotonic debounce suppress
duplicates without changing the order of accepted cues.

Hardware notes and manual cue simulation enter the same queue and dispatch
through the same application action service. Simulation therefore tests action
behavior, but does not pretend that a missing MIDI device is connected. The
input discovery endpoint exposes safe port metadata with opaque input IDs. An
API or dashboard selection resolves one of those IDs against the latest unique
port list, changes the in-memory selection, and wakes the supervisor to
reconnect. Sending a null selection disconnects the port. Selection is scoped to
the running backend process: shutdown discards it, and the next start uses
`STAGEPILOT_MIDI_INPUT_NAME` as the default again.

See [docs/plugins.md](docs/plugins.md) for the contribution checklist.

The selected-input lifecycle additionally supports the schema-2 default Playback
API source. Its copied stdlib WebSocket transport hands typed observations to a
64-entry asyncio queue and a separate 100-event monitor. Validation precedes
queueing; immutable mapping/discovery and listener generations prevent suppressed
or old-connection work from dispatching later. The same action dispatcher used
by MIDI handles known 1-based song positions, restarts and timer stops. API mode
keeps the existing MIDI listener as fallback. A connection-only arbiter selects
one action owner, prefers heartbeat-healthy API after stable failback, projects
one connected state, and deduplicates cross-source actions. Stale song order
gates API starts without changing ownership. Source switches close and join old
listeners; native and network MIDI transports are unchanged. The only outbound Playback commands
are Next/Previous inside an explicitly confirmed, stopped-only, bounded discovery
operation, with Previous-only restoration and complete-result persistence. See
[docs/playback-api.md](docs/playback-api.md) for trust and API contracts.

## Observable application state

The backend is the source of truth. Its state projection includes application
status, loaded service, current and next songs, selection index, timer state,
integration connection state, plugin health, recent events and errors, and the
last successful reload. Service-load state separately represents loading,
loaded, not-found, ambiguous, and error outcomes so API connectivity is not
mistaken for plan readiness. Its target date is the actual service date when a
current or upcoming plan is found.

Connection state describes verified external connectivity, not internal
capability. `connected` is published only after a real Planning Center request,
MIDI port open, ProPresenter API probe, or lighting-output open succeeds.
Disabled, unconfigured, demo, and simulated integrations remain
`disconnected`. The demo plugin may load and control its sample plan and
simulated timer, but it never publishes connection events for Planning Center,
MIDI, or ProPresenter. Dashboard readiness derives production checks from the
saved integration modes, real connection state, plan source/freshness, and
timer availability; a demo plan cannot satisfy Planning Center readiness.
The header system-status control exposes those checks on hover and keyboard
focus. Lighting always appears in that summary but remains optional until an
output is configured; a configured output that becomes unavailable blocks
readiness.

Planning Center discovery treats the configured local date as a search anchor.
Plans with service times on that date always take precedence. Only when none
match does discovery consider later service times, up to the configured
lookahead window, which defaults to 30 days. The earliest qualifying future
local date wins. Multiple plans on that date remain explicitly ambiguous until
the operator selects one; plans on later dates are not offered. Past plans,
plans without service times, and rehearsal-only times never qualify.

A failed refresh retains an active current or future plan as stale. Advancing
between pre-event dates does not discard a preloaded upcoming plan; once its
service date is in the past, rollover clears the actionable plan. State rollover
also requests an external timer stop and resets the local timer state. State
transitions enforce index bounds, duplicate-event protection, and valid timer
durations.

REST provides snapshots and commands. WebSockets provide live state and event
updates. Clients must tolerate reconnects and replace local state from a fresh
snapshot after reconnecting rather than assuming no events were missed.

## API and network boundary

Versioned endpoints live under `/api/v1`; the live stream uses `/ws`. Request
and response bodies use typed models. The initial server binds to `127.0.0.1`,
and CORS is limited to known development and desktop origins.

LAN binding is an explicit administrator choice. It requires a deployment-level
review of authentication, transport security, firewall rules, allowed origins,
and secret exposure. A permissive CORS setting is not an authentication system.

## Configuration and secrets

Configuration precedence is:

1. Safe application defaults.
2. An ignored local configuration file.
3. Environment-variable overrides.
4. Validated runtime settings.

The backend validates and owns integration configuration. The UI may submit a
new secret but must receive only an `is_configured`-style indication in return.
Logs and exported diagnostics redact sensitive values. See
[docs/configuration.md](docs/configuration.md) and
[docs/security.md](docs/security.md).

## Frontend and desktop boundaries

React renders backend state and sends user intent through typed HTTP commands.
It does not implement service navigation or timer sequencing. Its WebSocket
client owns connection and resynchronization behavior.

Dashboard geometry is isolated under `frontend/src/components/dashboard`.
GridStack core manages only grid shells, collision reflow, resizing, responsive
columns, and compaction; React retains ownership of all widget content. Layout
state is versioned, validated, and persisted locally after stable operations,
not timer or WebSocket updates. Desktop and tablet geometry remain independent,
while mobile uses a deterministic one-column order. The legacy order-only key
is migrated once and retained for rollback. See
[docs/dashboard-layout.md](docs/dashboard-layout.md).

Tauri v2 supplies a native window and, later, operating-system packaging. The
current shell loads Vite during development and `frontend/dist` in production.
It grants only Tauri core defaults and does not expose shell execution or file
system capabilities. Backend sidecar supervision, signing, installers, and
updaters remain explicit future packaging work.

## Standalone MultiTracks cue client

`tools/multitracks-cues` is an independent Node.js/TypeScript package and direct
Streamable HTTP MCP client. It does not depend on the Python backend or Tauri
shell. OAuth/credential storage, MCP transport, MultiTracks normalization,
canonical cue planning, sequential application, verification, reporting, and
CLI presentation are separate boundaries. This keeps the deterministic safety
logic reusable by a future desktop panel or supervised sidecar without moving
remote-write behavior into the React UI.

The package enforces a fixed tool allowlist. Read operations are available to
inspection and dry-run; the three create operations require explicit apply
mode. Destructive MIDI tools are rejected before transport invocation. Every
write is preceded by a fresh read and followed by a complete scoped read-back.

## Technology decisions

- **FastAPI and Pydantic** provide asynchronous request handling, WebSockets,
  runtime validation, and an inspectable typed API without coupling the service
  to a desktop runtime.
- **React and TypeScript** support a strongly typed, component-oriented live
  dashboard. **Vite** keeps the development and build path small and fast.
- **Tauri v2** provides a lightweight cross-platform native shell while allowing
  the same frontend to run in a browser.
- **Python asyncio** fits concurrent HTTP, WebSocket, plugin, and lifecycle work;
  blocking libraries remain isolated rather than blocking the event loop.

## Major decisions not yet made

The project has not yet committed to a secure credential-store implementation,
a Python sidecar packaging toolchain, remote authentication, or a stable plugin
distribution ABI. Those decisions require implementation and platform testing;
the current interfaces should not be presented as compatibility guarantees.
