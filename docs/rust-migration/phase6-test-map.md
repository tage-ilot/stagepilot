# Phase 6 test map (Python test -> Rust test)

Started in 6b-A. `contract` means the route is covered by a recorded transcript that both
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
| `test_remote_admin_actions.py`, `test_remote_server.py`, `test_remote_runtime.py`, `test_remote_lifecycle.py` | the listener, reconcile loop and admin actions are 6b-B | not ported (6b-B) |
| `test_remote_bootstrap.py`, `test_remote_checkin.py`, `test_remote_connector.py`, `test_remote_control.py`, `test_remote_files.py`, `test_remote_health_alert.py`, `test_remote_quick.py` | remote-modules crate | not ported (6b-B) |
| `test_api.py::test_dashboard_assets_are_served...` (web root) | `remote.rs::dashboard_and_assets_are_served_and_the_api_still_answers` | ported |
| static 404/405/redirect/range behaviour (no Python unit test; Starlette's) | `remote.rs::static_*`, contract profile `static` (34 steps) | new coverage |
| remote login/session/logout/revoke, users CRUD, throttle (exercised via `test_remote_*.py` and the store tests) | `remote.rs::login_*`, `user_administration_*`, contract profile `remote` (91 steps) | ported |

Earlier maps: `phase3a-test-map.md`, `phase4-test-map.md`; Playback and Planning Center route
shapes are covered by the contract.
