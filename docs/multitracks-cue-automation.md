# MultiTracks Playback cue automation

StagePilot includes an independent TypeScript MCP client at `tools/multitracks-cues`. The explicit `setlist-ordinal-test` profile validates cue creation without changing StagePilot’s production E7/velocity-100 runtime behavior.

The deterministic workflow remains fully usable without ChatGPT or OpenAI. An optional [ChatGPT/Codex agent interface](chatgpt-agent-cli.md) translates natural-language requests into narrowly scoped StagePilot read and proposal tools. It does not replace MultiTracks OAuth, and it cannot directly create, update, or delete MIDI data.

The profile assigns velocity from the complete, position-sorted setlist: qualifying song 1 receives velocity 1, song 2 receives 2, and so on. Non-song, ambiguous, and unsupported items do not consume ordinals. The CLI fails before writing if more than 127 songs qualify.

The cue is placed exactly one schema-proven musical beat after song start. The adapter accepts nested or flat measure/bar, beat, and tick coordinates only when their start values and the beat increment are unambiguous. It never substitutes milliseconds or guesses tempo, time signature, PPQ, or pickup behavior.

Library events use only an existing, uniquely proven Default MIDI Bank. Bank creation is not allowlisted. Repeated reusable Library targets are rejected when their occurrences require different velocities. Cloud Arrangements remain occurrence-specific only when stable writable identities are explicit.

The safety sequence is:

```text
complete setlist classification and ordinal assignment
                       ↓
bus, target, Default-bank, and event inspection
                       ↓
schema-proven one-beat position + deterministic dry-run
                       ↓
explicit one-song confirmation and one create call
                       ↓
complete scoped read-back and canonical verification
                       ↓
atomic journal + sanitized text/JSON/CSV reports
```

No bank or event is overwritten, moved, updated, or deleted. No MIDI file, copied bank, or production-cue import is used. Exact existing cues are idempotently skipped; every discrepancy becomes a conflict.

Run `./scripts/setup-multitracks-cues.sh`, then follow the complete commands and recovery guidance in the [package guide](../tools/multitracks-cues/README.md).

