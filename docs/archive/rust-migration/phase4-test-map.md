# Phase 4 test map (Python test -> Rust test)

- `test_midi_plugin.py` -> `crates/stagepilot-midi/tests/plugin_behaviour.rs`
- `test_lights_plugin.py`, plugin parts of `test_lights_osc.py` -> `crates/stagepilot-lights/tests/plugin_behaviour.rs`
- OSC wire format -> `crates/stagepilot-osc/tests/golden_packets.rs` (+ `fixtures/osc_golden.json`)
- `test_propresenter_client.py` -> `crates/stagepilot-propresenter/tests/client_http.rs`
- `test_propresenter_plugin.py`, `_recovery.py`, `_api.py` -> `crates/stagepilot-propresenter/tests/plugin_behaviour.rs`
- `test_midi_selection_api.py`, `test_midi_config_api.py`, route shapes -> contract "outputs" profile (`scripts/contract`)
