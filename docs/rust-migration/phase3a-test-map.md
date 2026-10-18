# Phase 3a: Python test file to Rust test mapping

Python counts are `def test_` counts; Rust counts are `#[test]`/`#[tokio::test]` counts (parametrised Python cases expand at run time).

| Python test file (count) | Rust coverage | Status |
|---|---|---|
| test_playback_scan_diagnostics.py (25) | crates/stagepilot-playback/tests/find.rs (46): classification, diagnosed scan, blocked-address detection, find_playback / scan_candidates, on tokio's paused clock | ported |
| test_playback_arbitration.py (10) | tests/arbiter.rs (19): arbiter and ArbitratedDispatcher with a hand-driven clock | ported (plugin/HTTP/MIDI-backend halves: 3b) |
| test_playback_arrangement.py (20) | tests/arrangement.rs (8): pairing around skipped songs and length alignment | pure-function half only; settings/HTTP/mapper tests: 3b |
| test_playback_lengths.py (15) | tests/lengths.rs (21): quick/precise measurement, restore, refused-while-playing, discovery walk, against SimPlayback on virtual clock | Playback half ported; HTTP/service half: 3b |
| test_playback_fast_transport.py (23), test_playback_api_transport.py (14) | tests/client.rs (11), tests/transport.rs (4, crafted byte streams), tests/wire_commands.rs (2) | client-level behaviour only (phase 1/3a); HTTP routes: 3b |
| test_playback_reconnect.py (12), test_playback_cancellation.py (3), test_playback_fake_harness.py (8) | tests/client.rs, src/client.rs unit tests, testkit.rs (FakePlayback / SimPlayback) | partial; harness is replaced by the Rust fakes |
| test_playback_integration.py (19), test_playback_mapping.py (12), test_playback_resume_mapping.py (14), test_playback_settings.py (12), test_midi_selection_api.py (6) | none | not ported: plugin glue, settings, HTTP and MIDI selection are phase 3b (and the phase-2 server work) |

Timing: discovery, arbiter and lengths tests use a virtual or hand-set clock; nothing waits on a real heartbeat margin.
