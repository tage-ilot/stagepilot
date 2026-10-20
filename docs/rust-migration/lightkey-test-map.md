# Lightkey test map (Python test -> Rust test)

Feature: Lightkey record/replay (PR #248, `docs/rust-handoff/lightkey-record-replay.md`), ported into
`stagepilot-lights`, `stagepilot-core` and `stagepilot-server`. Rust files:

- `crates/stagepilot-lights/src/lightkey.rs`: pure logic, models (port of `core/lightkey.py`)
- `crates/stagepilot-lights/src/lightkey_link.rs`: the async link and take stores (port of `plugins/lights/lightkey.py`)
- `crates/stagepilot-core/src/config.rs`: `LightkeySettings`, `LightsSettings.lightkey`
- `crates/stagepilot-server/src/routes_outputs.rs`: the four routes, `POST /lights/settings` carry-over
- `scripts/contract/{mock_lightkey,lightkey_app,serve_python_lightkey}.py`, `scenarios.py`: the contract "lightkey" profile

## `test_lightkey_core.py` -> `crates/stagepilot-lights/tests/lightkey_golden.rs`

The Rust test reads `backend/tests/golden/lightkey_golden.json` through `include_str!`; the file is
the one Python reads, unmodified.

| Python | Rust |
|---|---|
| `test_state_at` (6 vectors) | `state_at_matches_every_golden_case` |
| `test_managed_cues` | `managed_cues_match_golden` |
| `test_replay_plan` (3 vectors) | `replay_plan_matches_every_golden_case` |
| `test_unaddressable_cue_names_are_dropped_not_sent` | `unaddressable_cue_names_are_dropped_not_sent` |
| `test_osc_address` (2 vectors) | `osc_addresses_match_golden_and_never_use_toggle` |
| `test_build_take_edges` (6 vectors) | `build_take_edges_matches_every_golden_case` |
| `test_worth_keeping` (4 vectors) | `worth_keeping_matches_golden` |
| `test_operator_hold` (4 vectors) | `operator_hold_matches_every_golden_case` (+ `held_cues_lists_only_live_holds_sorted`, extra) |
| `test_edge_rules` | `edge_rules` |
| `test_take_sorts_its_edges` | `take_sorts_its_edges` |
| `test_effective_api_url` (4 cases) | `effective_api_url_cases` |
| `test_api_url_rejects_paths_and_spaces` (3 cases) | `api_url_rejects_paths_and_spaces` |
| `test_replay_defaults_on` | `replay_defaults_on_and_old_settings_load_with_replay_on` |

## `test_lightkey_link.py` -> `crates/stagepilot-lights/tests/lightkey_link.rs`

The fake agent is a real axum server on loopback (Python: an httpx mock transport) reporting
timestamps on a clock 5 s ahead of ours; the link uses its real `reqwest` client. OSC goes to an
in-memory recorder.

| Python | Rust |
|---|---|
| `test_connects_and_reports_status` | `connects_and_reports_status` |
| `test_unreachable_then_recovers` | `unreachable_then_recovers` |
| `test_refused_address_says_how_to_fix_it` | `refused_address_says_how_to_fix_it` (asserts the full string) |
| `test_lightkey_closed_is_its_own_state` | `lightkey_closed_is_its_own_state` |
| `test_cannot_record_when_not_connected_or_not_osc` | `cannot_record_when_not_connected_or_not_osc` (+ the "Turn Lighting on first." refusal) |
| `test_records_changes_with_song_times_and_saves_them` | `records_changes_with_song_times_and_saves_them` |
| `test_short_recording_does_not_overwrite_a_good_take` | `short_recording_does_not_overwrite_a_good_take` |
| `test_seek_while_recording_discards_the_take` | `seek_while_recording_discards_the_take` |
| `test_recording_turned_off_mid_song_saves_if_enough_was_captured` | `recording_turned_off_mid_song_saves_if_enough_was_captured` |
| `test_replay_syncs_then_fires_edges_slightly_early` | `replay_syncs_then_fires_edges_slightly_early` |
| `test_replay_switch_off_sends_nothing` | `replay_switch_off_sends_nothing` |
| `test_replay_does_not_run_while_recording_armed` | `replay_does_not_run_while_recording_armed` |
| `test_replay_stands_back_from_a_cue_the_operator_just_changed` | `replay_stands_back_from_a_cue_the_operator_just_changed` |
| `test_stop_cancels_remaining_replay` | `stop_cancels_remaining_replay` |
| `test_seek_replays_from_the_new_position` | `seek_replays_from_the_new_position` |
| `test_agent_restart_resets_the_sequence_without_a_false_change` | `agent_restart_resets_the_sequence_without_a_false_change` |
| `test_file_store_round_trip_and_bad_file` | `file_store_round_trip_and_bad_file` |

Rust tests with no Python counterpart (the handoff note lists the behaviour, Python did not test it):

| Rust | Pins |
|---|---|
| `api_url_is_derived_from_the_osc_host_when_unset` | derived address on the link status |
| `a_silent_agent_is_a_timeout_not_a_refusal` | "Lightkey API did not answer." |
| `lighting_off_reports_off` | link `off`, "Lighting is off." |
| `clock_offset_is_measured_with_seven_samples_on_connect` | 7 `/time` samples on connect, no refresh inside 120 s |
| `offset_uses_the_lowest_round_trip_sample` | lowest-RTT sample wins |
| `a_recording_turned_off_short_says_not_saved` | "Not saved: only N of M seconds of ..." |
| `timer_reset_while_recording_discards_the_take` | reset discards |
| `a_paused_recording_resumes_in_place_and_backfills_changes` | resume within 2 s, back-fill |
| `a_new_song_finishes_the_one_before` | song change while recording |
| `replay_blocked_strings_are_exact_and_ordered` | the four `replay_blocked` strings and their precedence |
| `a_song_without_a_take_sends_nothing` | no take, no OSC |
| `clear_take_removes_it_and_reports_a_missing_one` | clear / missing |
| `file_store_writes_atomically_in_the_python_layout` | `version`, `indent=1`, `Z` timestamps, no temp file left |
| `a_file_with_an_invalid_take_is_renamed_bad_and_ignored` | invalid take in a valid JSON file -> `.bad` |

## `test_lightkey_api.py` -> `crates/stagepilot-server/tests/lightkey_api.rs` and the contract

| Python | Rust |
|---|---|
| `test_defaults_replay_on_and_url_derived_from_the_osc_host` | `defaults_replay_on_and_url_derived_from_the_osc_host` |
| `test_settings_save_only_what_changed_and_persist` | `settings_save_only_what_changed_and_persist` |
| `test_bad_address_is_rejected_with_422` | `bad_address_is_rejected_with_422_in_the_no_context_form` (asserts the exact body) |
| `test_recording_refused_until_connected` | `recording_refused_until_connected` (fake agent down; Python's address is unreachable) |
| `test_clearing_a_missing_take_is_reported_not_an_error` | `clearing_a_missing_take_is_reported_not_an_error` |
| `test_saving_lighting_settings_keeps_the_lightkey_block` | `saving_lighting_settings_keeps_the_lightkey_block` |
| `test_old_settings_without_a_lightkey_block_load_with_replay_on` | `old_settings_without_a_lightkey_block_load_with_replay_on` |

Every one of these is also a step of the contract "lightkey" profile (54 steps, recorded from the
Python backend, replayed against both), see `docs/rust-migration/contract.md`. The extra Rust test
`the_four_lightkey_routes_validate_their_bodies_like_fastapi` pins the 422 shapes.

## OSC byte equality

`tools/rust-output/gen_lightkey_osc_golden.py` sends every command of Python's real `replay_plan`
(9 take/position cases, 125 packets, including cue names with spaces, `%`, accents, CJK, 200
characters and 1-5 character names for padding) through python-osc to a loopback socket and
records the datagrams. `crates/stagepilot-lights/tests/lightkey_osc_packets.rs` asserts the Rust
command list, address and encoded bytes equal Python's for each one, and that the whole chain
(event bus -> link -> plugin OSC output) emits those datagrams in order for the cases that finish
within 1.5 s. CI regenerates the file and fails on a diff.

## Not ported, and why

- `frontend/src/components/LightkeyStrip.test.tsx`: frontend, unchanged by the migration.
- `test_lightkey_link.py` runs the link against `httpx.MockTransport`; Rust uses a real loopback
  HTTP server and the real `reqwest` client, which is a stronger test, not a weaker one. A
  `LightkeyHttp` trait is also injectable (used by the server API tests).
- Python's `test_file_store_round_trip_and_bad_file` writes `{not json`; Rust additionally covers a
  structurally valid file holding an invalid take.
- Real Lightkey timing (the PR's 41 ms median OSC-to-DMX, 28-52 ms, n=12) is not re-verified on
  Rust: the rig authorisation has lapsed. Nothing here sends OSC or MIDI anywhere real.
