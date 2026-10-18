# Phase 6: status and what is still open

Branch `feat/rust-phase6d` = phase 6 integration (`feat/rust-phase6`: phases 2, 3a, 3b, 4, 5)
+ 6b-A (server routes: diagnostics, remote access/auth/ingress, static parity)
+ 6b-B (`stagepilot-remote` crate) + 6c (the two joined, `stagepilot-headless`) + 6d (this step:
Tauri shell linking the server in process behind the `in-process` cargo feature).

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
| ~~Tauri shell linking the server in process behind a cargo feature (sidecar and PyInstaller path stay)~~ | 6d `desktop/src-tauri` feature `in-process`, `crates/stagepilot-server/src/embedded.rs` (`EmbeddedServer`: start, graceful stop with abort after 8 s, crash/failed-start report), `launch::{run_production_until, serve_until}`. Default build is unchanged (sidecar) |
| ~~Desktop lifecycle checks for the in-process variant in CI~~ | `.github/workflows/ci.yml`: `cargo check` and `cargo test --features in-process` on Linux, Windows, macOS Apple Silicon and macOS Intel, next to the existing sidecar steps; `embedded.rs` tests start the real server, probe `/api/v1/health` as the shell does, stop it gracefully and check the port is released |

## Startup and shutdown order (matches Python)

Start: bind the listener, OAuth startup refresh, state service, plugins, `application.started`,
startup-activation task (+1 s), proactive OAuth refresh, then (only when both
`STAGEPILOT_DESKTOP_REMOTE_ROOT` and `STAGEPILOT_CLOUDFLARED_BINARY` are set) the managed-Remote
reconcile task and the desktop supervisor (`DesktopRemoteManager::run`).
Stop on Ctrl-C or SIGTERM: the supervisor (stops `cloudflared`, publishes the off status), the
reconcile task (closes the dedicated listener), `application.stopping`, plugins in reverse, state
service. `StdSpawner` also kills a child on drop, so a crash cannot leave a connector behind.

## Open (needs the rig or a real install; phase 7 prerequisites)

| Item | Why it is still open |
|---|---|
| Installer chain proof against the last Python release | needs real installers on each OS; the in-process variant is not packaged by any CI job |
| In-process shell on a real install: window, tray, updater hand-off (`prepare_for_update` releasing the executable on Windows/NSIS), autostart, macOS Gatekeeper/translocation | needs a real machine on each OS; CI only compiles and runs the unit and lifecycle tests |
| In-process direct `keyring` credential store | the broker-over-HTTP store is still used in both variants; the direct store needs the real OS keychain on Windows and macOS |
| Windows job objects / macOS zombie handling for `cloudflared` | needs a real machine on each OS |
| Python tests with no dedicated Rust port | `test_keychain_access`, `test_logging` (`redact_secrets`), `test_lights_settings`, the PCO API files (covered by the `pco` contract), `test_planning_center_plugin`, `_lengths_retry`, `test_restartless_serialised`, load-state invariants in `test_state_models`. `test_rate_anomaly` is not applicable until the module is wired into the app |
| Command-line entry points of the individual remote modules (`remote_control`, `remote_connector`, ...) | Python runs them as `python -m`; no Rust binary yet. Not needed by the app, which runs them in process |
| Any real-rig check | the 2026-10-17 rig authorisation has lapsed; rig checks are deferred |

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

## Differences of the `in-process` shell from the Python sidecar

- The server is tasks in the shell's own process on a private two-thread tokio runtime, not a child
  process. There is no process tree to kill, no PyInstaller unpack, and the executable replaced by
  the updater is the shell itself. A server panic is caught and reported; a hard crash (abort,
  stack overflow) takes the shell down with it.
- Configuration still travels as `STAGEPILOT_*` variables, set in the shell's own environment
  before the server starts (the sidecar gets them on its command line). They stay set for the life
  of the shell, and the shell is single-instance.
- Log lines go to `stagepilot-backend.log` as `tracing` JSON lines (not structlog). The file is
  rotated at each start as before; a restart inside one shell run keeps writing to the first
  start's handle, so a rotation mid-run does not move it.
- Crash reports use failure kind `ServerExited` (`SidecarExited` stays for the sidecar). The crash
  log, `Failed` status event and crash-loop detection are the shared path (`record_backend_failure`).
- Stop is graceful for up to 8 s (`DEFAULT_STOP_GRACE`) and then aborts the task; the sidecar is
  killed outright.
- Port selection, the external-backend and port-occupied probes, readiness polling and the 30 s
  startup timeout are the same code for both variants.

## Not verified

- The in-process shell was not compiled on this host (no `pkg-config`/webkit, so Tauri cannot
  build here); its compile and tests are proven by the CI legs only.

- Installer chain against the last Python release; any real-rig check (no rig use: the
  2026-10-17 authorisation has lapsed, rig checks are deferred).
- No real control plane, Cloudflare account, `cloudflared`, credential broker or Planning Center
  was contacted; mock control plane and fakes only.
- The headless binary was not run against real saved settings or the OS keychain.
- Windows file locking (`File::lock`) and the macOS/Windows cargo legs run only in CI.
- `midi-probe` cannot build on this host (no pkg-config/alsa); the Linux CI job excludes it too.
