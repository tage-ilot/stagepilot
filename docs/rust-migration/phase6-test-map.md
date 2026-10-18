## Remote modules (`stagepilot-remote`, phase 6b-B)

Python tests for `remote_*.py` mapped to `crates/stagepilot-remote/tests/`. The Python tests that
drive the real FastAPI app (`create_app`, `RemoteIngress`, `RemoteStore`, `/api/v1/remote-access`)
belong to the server crate and are not ported here; the rows below say what replaces them.

| Python test file | Rust file | Notes |
|---|---|---|
| `test_remote_control.py` (7) | `control.rs` | Same scenarios against an in-memory Cloudflare fake: fresh enable, lost responses at cuts 1-3 and 4-7, outage, ownership, parallel enable (4 threads), binding change. Plus a real-file lock test, `corrupt_state_still_closes_on_disable`. |
| `test_remote_files.py` (3) | `control.rs` | POSIX/Windows permission-bit tests keep their `cfg` split (`skipif` -> `#[cfg(unix)]` / `#[cfg(windows)]`). `desired_remote_validation` covers pydantic's `DesiredRemote`. |
| `test_remote_bootstrap.py` (21) | `bootstrap.rs` | All 21 ported one-for-one (8/16/32-char ids folded into one test), plus broker-URL validation, status mapping, state-file rules, retry pacing. The 8 s slow-broker test runs a real loopback server and the real reqwest transport (4 s here). The `inspect.getsource` timeout test is `BROKER_TIMEOUT_SECS >= 90` at compile time. `test_regenerate_preserves_remote_users_in_the_identity_store` is not ported: it exercises `RemoteStore` (server crate). |
| `test_remote_checkin.py` (5) | `checkin.rs` | All ported. |
| `test_remote_admin_actions.py` (7) | `feature.rs` | 6 control-plane tests ported; `test_reset_login_attempts_...` tests `RemoteStore` (server crate) and is not ported. |
| `test_remote_connector.py` (3) | `connector.rs` | Against `FakeSpawner` + `ManualClock` (portable, runs on Windows); `std_spawner_*` tests start and terminate a real child on Unix. |
| `test_remote_quick.py` (2) | `connector.rs` | Plus rate-limit, connected-health and spawn-failure cases. |
| `test_remote_feature.py` (9) | `feature.rs` | `RemoteFeature` tests ported; the 4 tests through `create_app`/`TestClient`, the viewer-policy, idempotency-journal and `_enable_error_message` tests belong to the server crate (`api/remote_feature.py`). `_worker_diagnostic` is covered by `beta_request_retry_and_error_mapping`. |
| `test_remote_health_alert.py` (6) | `feature.rs` | All 6 ported, plus `health_check_states` for `remote_health.py`. |
| `test_remote_runtime.py` (2) | `listener.rs` | Reconcile loop against a small axum app standing in for `RemoteIngress`. |
| `test_remote_server.py` (2 + 2 param) | `listener.rs` | Listener, port conflict, attach-once, invalid configuration. The cookie/CSRF/WebSocket auth assertions need the server crate's `RemoteIngress` and are not ported. |
| `test_remote_lifecycle.py` (2 + 2 param) | `listener.rs` | |
| `test_remote_transport.py` (1) | `listener.rs` | HTTPS-only check (426, `no-store`, duplicate/`https,http` headers). The identity-store assertions are server-crate. |
