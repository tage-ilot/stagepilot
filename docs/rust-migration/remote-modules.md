# Rust migration: the three unscoped remote modules

Scope note for phase 6 planning. Read from `backend/src/stagepilot/` at 2.41.1. Line counts are
the Python source lines (`wc -l`); the Rust estimate is code plus ported tests, in agent-days.
The three files total 1,463 lines; the 10 other `remote_*.py` files they depend on add another
1,481 lines, and `backend/tests/test_remote_*.py` is the existing contract for all of them.

## Summary

| Module | Python lines | OS features touched | Main crates | Rust lines (est.) | Risk |
|---|---:|---|---|---:|---|
| `remote_beta_control.py` | 388 | file lock, atomic file writes, HTTPS to the control plane, a CLI entry point | `reqwest`, `serde`, `fs4` (or `fd-lock`), `tempfile` | 450-600 | Low |
| `remote_bootstrap.py` | 559 | atomic file writes, loopback HTTP to the Tauri credential broker (Keychain / Credential Manager), HTTPS to the control plane | `reqwest`, `serde`, `uuid`, `url` | 600-800 | Medium |
| `remote_desktop.py` | 516 | child-process supervision of `cloudflared` (via `remote_connector`), asyncio timers, file polling, FastAPI lifespan hook | `tokio::process`, `tokio::time`, `axum` | 700-900 | Medium-high |

Estimate for porting all three plus tests: 8-12 agent-days, inside phase 6 ("Server and shell").

## `remote_beta_control.py` (388 lines)

- What it is: a command-line tool and library that provisions or retires this installation with the
  control plane (`enable | disable | revoke | reconcile | status`), driven by a config JSON file.
- OS features: an exclusive cross-platform file lock (`file_lock.py`: `msvcrt.locking` on Windows,
  `fcntl.flock` on POSIX), atomic file replace with fsync, and plain HTTPS calls. No registry, no
  launchd/Task Scheduler, no signals.
- Crate equivalents: `reqwest` (rustls, no redirects, no env proxies, 20 s timeout to match
  `httpx.Client(trust_env=False, follow_redirects=False)`), `fs4` for the lock, `tempfile::persist` plus an
  explicit `File::sync_all` and parent-directory fsync for `atomic_write`. `clap` for the CLI.
- Risk: low. It is pure request/response logic with an explicit state machine; the existing tests
  mock `httpx`, which maps to `wiremock`.

## `remote_bootstrap.py` (559 lines)

- What it is: first-run enrolment, re-enrolment, revoke and reactivate of the installation identity.
  Holds `BootstrapMetadata`/`BootstrapState` (pydantic) and a `NativeRemoteCredentialStore`.
- OS features: the OS secret store, but only indirectly. It never touches the Keychain itself: it talks to the
  Tauri shell's credential broker on `http://127.0.0.1:<port>/v1/credentials/<id>` with a bearer token
  from the environment (`STAGEPILOT_CREDENTIAL_BROKER_ORIGIN` / `_TOKEN`). Atomic file writes.
- Crate equivalents: `reqwest` for the broker call. When the backend is linked in-process into Tauri
  (plan section 2), the broker HTTP hop disappears and the store becomes a direct `keyring` call, so
  this class gets simpler, not harder. Keep the HTTP implementation for the headless binary only if
  headless needs the OS store (decision for phase 6).
- Risk: medium. The logic has many revoke/re-enrol corner cases (`finish_revoke`, `reactivate`,
  `discard_identity`) and a 90 s timeout tuned for human Keychain prompts. Port the existing tests
  one for one and keep the timeout.

## `remote_desktop.py` (516 lines)

- What it is: `DesktopRemoteManager`, the object the desktop app talks to for Remote Access:
  enable / disable / reset / regenerate, status publishing, a control reconcile loop, and the FastAPI lifespan wiring
  (`attach_desktop_remote`).
- OS features: it supervises a child process (the staged `cloudflared` binary, started by
  `remote_connector.Connector` with `subprocess.Popen`, stdout/stderr discarded, stopped with a timeout then kill),
  polls a control file and writes a status file, and reads `platform.node()` for the device name. Everything runs in an asyncio
  loop with timers.
- Crate equivalents: `tokio::process::Command` with `kill_on_drop`, `tokio::time::interval`,
  `hostname` crate for `platform.node()`, `axum` router state/extension in place of the FastAPI lifespan.
  `cloudflared` itself stays a bundled binary (no Rust replacement is planned).
- Risk: medium-high, because it is the only one that manages a long-lived child process across three
  OSes (zombie handling on macOS, job objects/console-window flags on Windows, orphan cleanup when the
  app is killed). The process lifecycle needs a real-machine test on Windows and both macOS
  architectures, not just unit tests. The connector logic itself is small (212 lines in `remote_connector.py`).

## Other notes for phase 6

- `remote_quick.py` and `remote_connector.py` share the child-process code; port them with `remote_desktop`.
- Nothing here needs unsafe code, registry access, or platform-specific system APIs beyond the file lock and the child process.
  That keeps the workspace-wide `unsafe_code = "forbid"` lint viable.
- None of this is needed for phases 2-5.
