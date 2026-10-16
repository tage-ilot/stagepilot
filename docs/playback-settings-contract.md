# Playback connection settings contract

The settings schema is now version 2. New installations select
`integration_modes.midi_source: "playback_api"`. Loading a version-1 settings
file (including an old file without a schema version) switches the selected
source to Playback and atomically saves version 2. All existing MIDI fields,
transport selections, mappings, lighting settings and other ordinary settings
are retained. Unsupported versions remain rejected. Subsequent version-2 loads
never change an explicitly selected alternative.

## Fields

Runtime `Settings` and persisted `PersistentSettings` expose `playback_api`:

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| enabled | boolean | true | Operator on/off preference |
| host | string or null | null | Manual host override; whitespace/empty becomes null |
| port | integer 1–65535 | 8080 | Playback WebSocket port |
| auto_scan | boolean | true | Permit automatic LAN discovery after loopback check |
| song_order | unique strict integer array, max 200 | [] | Explicitly discovered ordered opaque song IDs |
| captured_version | strict integer or null | null | Heartbeat setlistCloudVersion at capture |
| captured_at | ISO datetime or null | null | Capture timestamp (integration should supply UTC) |
| skipped_song_ids | unique strict integer array, each in song_order | [] | Playback songs left out of the pairing (for example background music). Cleared by a new scan |
| skipped_plan_song_ids | unique string array | [] | Service-plan songs with no Playback track (for example an acoustic song). Kept across scans; cues are held back if one is no longer in the loaded plan |

`integration_modes.midi_source` remains the existing source-selector contract:
`playback_api` selects Playback; `real` selects the unchanged desktop/network
MIDI alternative; `simulated` retains simulation. Playback enablement and source
selection are independent. Integration must run Playback only when selected AND
enabled and must stop inactive listeners. MIDI runtime enablement continues to
follow the existing `real` selection, without overwriting saved MIDI enablement
or other values. `network_midi` IPC configuration is now also persisted and
round-trips with runtime settings.

The existing settings snapshot serialization carries this block and selected
source. Source-specific routes/UI wiring are integration work, not this settings
slice. The frontend should render Playback in the existing MIDI/Playback settings
window as the primary panel and keep MIDI controls in an initially collapsed
"Alternate connection: MIDI settings" disclosure, not a new page. Collapsed UI state is not persisted here.

## Discovery handoff

Only successful operator-confirmed Discover Song Order should update
`song_order`, `captured_version`, and `captured_at` together. No passive numbering
or epoch-confirmation endpoint is introduced. Settings validation does not arm
actions: integration owns playing/disconnected/conflict refusal, command bounds,
restore, event suppression and order trust. A changed cloud version or unknown
heartbeat song ID makes the order stale; reconnect alone does not invalidate,
but the first heartbeat must be checked against saved version and membership.
Stop/pause may still stop timers while order is stale. The settings migration
creates no discovered order and sends no Playback commands.

## Song arrangement

Neither list is ever reordered. `PUT /api/v1/playback-api/arrangement` accepts only
`skipped_song_ids` and `skipped_plan_song_ids`; the Nth Playback song not skipped pairs with the Nth plan
song not skipped (`plugins/playback_api/arrangement.py`). A skipped or unpaired Playback song does not start
or restart a plan song, and does not stop the timer. With no skips the original one-to-one behaviour is
unchanged. The Planning Center length preview uses the same pairing, so a skipped plan song gets no measured
length. Saving an arrangement clears `lengths_applied_key`, because the pairing of lengths to plan songs changed.
