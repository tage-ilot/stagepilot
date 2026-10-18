# Phase 6 test map (Python test -> Rust test)

Every file under `backend/tests` appears once below (counts are `def test_` per file). `contract` = a recorded transcript both backends replay (`scripts/contract`). "Open" = no Rust test yet; the remaining items are listed in `phase6-remaining.md`.

| Python test file (count) | Rust counterpart | Status |
|---|---|---|
| test_api.py (15) | stagepilot-server/tests/api.rs, remote.rs (`dashboard_and_assets_are_served...`), contract base profile | ported (route level); Starlette-internal cases n/a |
| test_config.py (5) | stagepilot-core/tests/settings_compat.rs (`invalid_timezone_and_port...`, `planning_center_secret_is_never_written...`), stagepilot-pco/src/settings.rs env overrides | ported in part. Not applicable: pydantic `SecretStr` masking (`repr`) has no Rust equivalent; secrets use redacted-`Debug` types instead (phase 5) |
| test_critical_alerts.py (12) | stagepilot-core/tests/runtime_behaviour.rs (alerts section) | ported |
| test_dashboard_auth.py (4) | stagepilot-server/tests/api.rs | ported |
| test_diagnostics_api.py (7) | stagepilot-server/tests/remote.rs | ported (see rows below) |
| test_event_bus.py (6) | stagepilot-core/tests/runtime_behaviour.rs | ported |
| test_keychain_access.py (4) | stagepilot-core/src/settings.rs keychain store (one read per launch, no read when unconfigured) | not ported as a test: needs a fake `keyring` backend. Behaviour is the same by construction; open |
| test_lights_api.py (8), test_midi_config_api.py (11), test_propresenter_api.py (4) | contract profile `outputs` (75 steps) and stagepilot-lights/-midi/-propresenter plugin tests | ported via contract |
| test_lights_osc.py (21), test_lights_plugin.py (15) | stagepilot-osc/tests/golden_packets.rs, stagepilot-lights/tests/plugin_behaviour.rs | ported (phase4-test-map.md) |
| test_lights_settings.py (2) | stagepilot-core settings round trip, contract `outputs` | ported via contract; no dedicated test, open |
| test_logging.py (1) | none | not ported: `redact_secrets` has no Rust counterpart yet (Rust logs go through `tracing`; the server logs no credentials). Open |
| test_midi_client.py (7) | stagepilot-midi/src/lib.rs, output.rs unit tests | n/a for the `mido` backend (Python library); Rust uses `midir` behind a trait and the in-memory fake |
| test_midi_config_api.py, test_midi_plugin.py (14), test_midi_selection_api.py (6) | stagepilot-midi/tests/plugin_behaviour.rs, contract `outputs` | ported |
| test_plan_cache.py (4) | stagepilot-pco/tests/plan_cache.rs (5) | ported in 6c |
| test_planning_center_client.py (19), _plan_discovery.py (22) | stagepilot-pco/tests/client.rs (47) | ported (phase 5) |
| test_planning_center_oauth.py (16) | stagepilot-pco/tests/oauth.rs (23) | ported (phase 5) |
| test_planning_center_oauth_api.py (9), _setup_api.py (4), _setup_oauth_refresh.py (3), _settings_async.py (1), _settings_live_apply.py (1), _lengths_api.py (7) | contract profile `pco` (96 steps) and its request log | ported via contract; no dedicated Rust test, open |
| test_planning_center_lengths.py (21), _lengths_retry.py (4) | stagepilot-pco/tests/client.rs (item PATCH/undo cases) | ported in part; retry-queue cases open |
| test_planning_center_plugin.py (39), _multi_type_integration.py (3) | stagepilot-pco/src/plugin.rs behaviour through the `pco` contract profile | not ported as unit tests; open |
| test_playback_*.py (14 files) | see phase3a-test-map.md | ported |
| test_plugin_manager.py (4), test_runtime_plugin_lifecycle.py (5) | stagepilot-core/tests/runtime_behaviour.rs | ported |
| test_propresenter_client.py (8), _plugin.py (4), _recovery.py (4), _config.py (3) | stagepilot-propresenter/tests/client_http.rs, plugin_behaviour.rs | ported (config: models.rs unit tests) |
| test_rate_anomaly.py (9) | none | not applicable for now: `services/rate_anomaly.py` is imported by no application module (only by its test), so no running code path depends on it. Port when it is wired into the app |
| test_remote_*.py (13 files) | stagepilot-remote/tests/*.rs, stagepilot-server/tests/remote.rs, stagepilot-server/tests/launch.rs | ported (tables below) |
| test_restartless_midi.py (6), _presentation.py (9), _serialised.py (5), _services.py (7) | stagepilot-server/tests/api.rs (service source refusal), stagepilot-midi/-propresenter plugin tests, contract `outputs` and `pco` live-apply steps | ported in part; the serialisation-order cases are open |
| test_runtime_plugin_lifecycle.py (5) | stagepilot-core/tests/runtime_behaviour.rs | ported |
| test_settings_persistence.py (15) | stagepilot-core/tests/settings_compat.rs (19) | ported |
| test_startup_activation.py (2) | stagepilot-server/tests/launch.rs `activation::*` (3) | ported in 6c |
| test_state_models.py (10) | stagepilot-core/tests/python_contract.rs, state.rs serde | not applicable: these are pydantic validators; Rust types plus serde enforce the shape. Cases about the load-state invariants are open |
| test_state_service.py (40) | stagepilot-core/tests/runtime_behaviour.rs | ported |
| conftest.py, playback_fake.py | stagepilot-playback/src/testkit.rs | not applicable: Python fixtures |

## Server, headless and remote rows (detail)


 `contract` means the route is covered by a recorded transcript that both
backends replay (`scripts/contract`: profiles `remote`, `diagnostics`, `static`); `remote.rs` is
`crates/stagepilot-server/tests/remote.rs`.

| Python test | Rust / contract counterpart | Status |
|---|---|---|
| `test_diagnostics_api.py::test_bundle_too_large_returns_413` | `remote.rs::bundle_too_large_returns_413`, contract "diagnostics send an oversize bundle" | ported |
| `test_diagnostics_api.py::test_no_remote_manager_returns_503`, `_remote_manager_without_bootstrap_`, `_bootstrap_state_raises_`, `_unenrolled_`, `_untrusted_control_plane_origin_` | `remote.rs::unenrolled_and_broken_bootstraps_return_503` (all five), contract "diagnostics send after the identity was reset" | ported |
| `test_diagnostics_api.py::test_forwarding_path_statuses` (201/429/500) | `remote.rs::forwarding_path_statuses`, contract "diagnostics upstream rate limit / error" and the mock's request log | ported |
| `test_playback_scan_diagnostics.py` (playback section) | `remote.rs::json_bundles_gain_a_playback_section_and_a_failed_credential_is_a_502`; the scan summary itself is in `stagepilot-playback` tests | ported (route side) |
| `test_remote_feature.py::test_atomic_write_skips_unsupported_directory_fsync_on_windows` | not ported: Python-specific patching; Rust fsyncs the directory only under `cfg(unix)` | n/a |
| `test_remote_feature.py::test_enable_succeeds_with_zero_operators_on_fresh_install` | `remote.rs::enable_succeeds_with_zero_operators_on_fresh_install` | ported |
| `test_remote_feature.py::test_last_enabled_operator_removal_guard_is_unaffected` | `remote.rs::last_enabled_operator_removal_guard_is_unaffected`; store rules in `stagepilot-core` auth tests | ported |
| `test_remote_feature.py::test_product_bootstrap_and_intent` | `remote.rs::product_bootstrap_and_intent`, contract "remote bootstrap ..." / "remote enable ..." | ported |
| `test_remote_feature.py::test_remote_retry_and_viewer_policy` | `remote.rs::remote_retry_and_viewer_policy`, contract "ingress keyed action ..." / "ingress viewer ..." | ported |
| `test_remote_feature.py::test_journal_restart_uncertainty_and_conflict` | `remote.rs::journal_restart_uncertainty_and_conflict` | ported |
| `test_remote_feature.py::test_stale_status_and_foreign_route` | `remote.rs::stale_status_and_foreign_route` | ported |
| `test_remote_feature.py::test_named_marker_survives_ui_disable_and_reports_stable_url` | `remote.rs::named_marker_survives_ui_disable_and_reports_stable_url` | ported |
| `test_remote_feature.py::test_hostname_conflict_maps_to_recovery_text_...` | `remote.rs::enable_error_messages_never_name_infrastructure`; `_worker_diagnostic` belongs to `remote_beta_control` (6b-B) | route half ported |
| `test_remote_transport.py` (HTTPS-only wrapper over the ingress) | `HTTPSOnlyIngress` is `remote_server.py` (6b-B); the ingress half is `remote.rs::ingress_denies_lan_pin_and_unauthenticated_state` | partly ported |
| `test_api.py::test_dashboard_assets_are_served...` (web root) | `remote.rs::dashboard_and_assets_are_served_and_the_api_still_answers` | ported |
| static 404/405/redirect/range behaviour (no Python unit test; Starlette's) | `remote.rs::static_*`, contract profile `static` (34 steps) | new coverage |
| remote login/session/logout/revoke, users CRUD, throttle (exercised via `test_remote_*.py` and the store tests) | `remote.rs::login_*`, `user_administration_*`, contract profile `remote` (91 steps) | ported |

Earlier maps: `phase3a-test-map.md`, `phase4-test-map.md`; Playback and Planning Center route
shapes are covered by the contract.

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

## Integration (6c)

| Python behaviour | Rust test |
|---|---|
| `main.create_app` + `attach_desktop_remote` + `attach_managed_remote`: real manager wired, status/bootstrap/diagnostics identity, ordered shutdown | `launch.rs::real_manager_is_wired_and_shut_down_in_order` |
| `attach_managed_remote` refuses a relative path | `launch.rs::a_relative_root_is_refused_and_nothing_is_left_attached` |
| `POST /remote-access/enable` with an untrusted/unreachable control plane -> provider error (503 text) | `launch.rs::enable_without_a_reachable_control_plane_is_a_provider_error` |
| both env vars required (`STAGEPILOT_DESKTOP_REMOTE_ROOT`, `STAGEPILOT_CLOUDFLARED_BINARY`) | `launch.rs::environment_needs_both_variables` |
| headless argument handling | `stagepilot-headless/src/main.rs` tests (3) |
