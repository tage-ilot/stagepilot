# Phase 6: status and what is still open

Branch `feat/rust-phase6c` = phase 6 integration (`feat/rust-phase6`: phases 2, 3a, 3b, 4, 5)
+ 6b-A (server routes: diagnostics, remote access/auth/ingress, static parity)
+ 6b-B (`stagepilot-remote` crate) + 6c (this step: the two joined, `stagepilot-headless`).

## Done

| Item | Where |
|---|---|
| ~~Diagnostics endpoint~~, ~~remote access routes~~, ~~remote auth routes, ingress, retry middleware~~, ~~static frontend parity~~ | 6b-A (`stagepilot-server`) |
| ~~Remote modules (all 13: files, provider, control, beta control, bootstrap, feature, connector, quick, health, health alert, runtime, server, desktop)~~ | 6b-B (`stagepilot-remote`) |
| ~~Real `RemoteManager`, `DiagnosticsBootstrap`, `IngressFactory`, `GenerationSink`~~ | 6c `stagepilot-server/src/launch.rs` (`RealRemoteManager`, `RealBootstrap`, `ingress_factory`, `StoreGeneration`). The fakes stay in `remote_fakes.rs` for tests and the contract profiles |
| ~~Lifespan wiring (`attach_desktop_remote` + `attach_managed_remote`)~~ | 6c `launch::attach_desktop_remote`; `RemoteLifecycle::stop` stops in Python's reverse order |
| ~~Startup activation (`services/startup_activation.py`)~~ | 6c `startup_activation.rs`, run one second after start in production only |
| ~~`stagepilot-headless` binary~~ | 6c `crates/stagepilot-headless` |
| ~~Test map: every file under `backend/tests` mapped~~ | `phase6-test-map.md` (rows marked "open" are tests with no dedicated Rust port yet, listed below) |
| ~~Contract: remote (91), diagnostics (26), static (34)~~ | `scripts/contract` |

## Startup and shutdown order (matches Python)

Start: bind the listener, OAuth startup refresh, state service, plugins, `application.started`,
startup-activation task (+1 s), proactive OAuth refresh, then (only when both
`STAGEPILOT_DESKTOP_REMOTE_ROOT` and `STAGEPILOT_CLOUDFLARED_BINARY` are set) the managed-Remote
reconcile task and the desktop supervisor (`DesktopRemoteManager::run`).
Stop on Ctrl-C or SIGTERM: the supervisor (stops `cloudflared`, publishes the off status), the
reconcile task (closes the dedicated listener), `application.stopping`, plugins in reverse, state
service. `StdSpawner` also kills a child on drop, so a crash cannot leave a connector behind.

## Open

| Item | Notes |
|---|---|
| Tauri shell linking the server in process behind a cargo feature (sidecar and PyInstaller path stay) | next task; 3-5 days, includes the three-OS CI legs. Also the in-process direct `keyring` credential store (the broker-over-HTTP store is used now) |
| Installer chain proof against the last Python release | needs real installers on each OS |
| Python tests with no dedicated Rust port | `test_keychain_access`, `test_logging` (`redact_secrets`), `test_lights_settings`, the PCO API files (covered by the `pco` contract), `test_planning_center_plugin`, `_lengths_retry`, `test_restartless_serialised`, load-state invariants in `test_state_models`. `test_rate_anomaly` is not applicable until the module is wired into the app |
| Command-line entry points of the individual remote modules (`remote_control`, `remote_connector`, ...) | Python runs them as `python -m`; no Rust binary yet. Not needed by the app, which runs them in process |
| Windows job objects / macOS zombie handling for `cloudflared` | needs a real machine on each OS |

## Documented differences from Python

- `stagepilot-headless` accepts `--host`, `--port`, `--help`, `--version` (Python has no flags;
  `STAGEPILOT_HOST` / `STAGEPILOT_PORT` / saved settings decide the bind, default 127.0.0.1).
- Logs are `tracing` JSON lines on stdout, not structlog; `STAGEPILOT_LOG_LEVEL` sets the level.
- The listener is bound before plugins start, so a port conflict fails before anything runs.
- WebSocket close code 4403 on the remote ingress: axum rejects the upgrade with 426 first.
- `OSError`-class failures from the manager surface as a 500 (as in Python, where neither
  `except` clause catches them).
- `--contract-*` flags stay in `stagepilot-server` (the contract scripts are unchanged); only
  `stagepilot-server` without a contract flag now wires the desktop Remote like the headless binary.

## Not verified

- Installer chain against the last Python release; any real-rig check (no rig use: the
  2026-10-17 authorisation has lapsed, rig checks are deferred).
- No real control plane, Cloudflare account, `cloudflared`, credential broker or Planning Center
  was contacted; mock control plane and fakes only.
- The headless binary was not run against real saved settings or the OS keychain.
- Windows file locking (`File::lock`) and the macOS/Windows cargo legs run only in CI.
- `midi-probe` cannot build on this host (no pkg-config/alsa); the Linux CI job excludes it too.
