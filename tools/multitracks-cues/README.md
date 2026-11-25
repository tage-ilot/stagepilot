# StagePilot MultiTracks Cues

`stagepilot-cues` is a standalone, fail-closed MCP client for the official MultiTracks endpoint.

Deterministic mode does not require ChatGPT or OpenAI. Optional agent mode uses Codex App Server and the user’s authenticated ChatGPT account for natural-language interaction. All MultiTracks writes remain controlled by StagePilot’s deterministic planner, confirmation, applier, and verifier.

## One-command macOS setup

From the StagePilot repository root:

```sh
./scripts/setup-multitracks-cues.sh
./bin/stagepilot-cues --version
./bin/stagepilot-cues doctor
```

The idempotent setup accepts a supported system Node.js 22 or 24. Otherwise it downloads pinned Node.js 22.23.1 for Intel or Apple Silicon directly from `nodejs.org`, verifies the archive against the official `SHASUMS256.txt`, installs it under ignored `.tools/node/`, installs locked dependencies, and runs typecheck, tests, lint, and build. It also installs the official, lockfile-verified `@openai/codex` 0.146.0 package locally, checks its version, and generates temporary App Server JSON/TypeScript schemas to verify the protocol surface. It never uses `sudo`, `npm link`, a global package install, floating `npx`, or PATH changes.

## Optional ChatGPT agent

ChatGPT and MultiTracks are separate connections:

```sh
./bin/stagepilot-cues auth chatgpt login --browser
# or: ./bin/stagepilot-cues auth chatgpt login --device-code
./bin/stagepilot-cues auth chatgpt status

./bin/stagepilot-cues auth multitracks login
./bin/stagepilot-cues auth multitracks status
```

The ChatGPT session is owned by Codex under StagePilot’s application-data directory, not the user’s normal global Codex home. StagePilot never parses or copies Codex credentials. Logging out through `auth chatgpt logout` affects only this StagePilot-scoped session.

Agent mode requires a one-time privacy disclosure because normalized setlist names, song titles, dates, cue plans, and sanitized errors may be sent to OpenAI:

```sh
./bin/stagepilot-cues ask \
  "Show my upcoming setlists. Do not make changes."

./bin/stagepilot-cues chat
```

The agent can list, inspect, prepare, verify, and create an in-memory proposal. No model-callable tool performs a write. A proposal contains a random ID, short expiry, canonical plan digest, exact positions, and typed confirmation. StagePilot re-runs the deterministic plan before confirmation and invalidates changed plans. Only the existing applier performs an approved write, and success is reported only after read-back verification.

Manage agent state with:

```sh
./bin/stagepilot-cues agent status
./bin/stagepilot-cues agent privacy status
./bin/stagepilot-cues agent privacy reset
./bin/stagepilot-cues agent sessions list
./bin/stagepilot-cues agent sessions resume THREAD_ID "continue the inspection"
./bin/stagepilot-cues agent sessions delete THREAD_ID
```

Agent sessions are Codex App Server threads created for StagePilot. They are not normal ChatGPT web conversations and do not provide access to ChatGPT history, memories, projects, files, connectors, or another app’s tokens. See [the agent architecture](../../docs/chatgpt-agent-cli.md).

## OAuth

Set the issued OAuth values only in the current shell or through the secure `configure` flow:

```sh
export MULTITRACKS_MCP_CLIENT_ID='YOUR_CLIENT_ID'
export MULTITRACKS_MCP_CLIENT_SECRET='YOUR_CLIENT_SECRET'
./bin/stagepilot-cues auth multitracks login
./bin/stagepilot-cues auth multitracks status
```

Tokens and configured secrets are stored in macOS Keychain or Windows Credential Manager. They are redacted from logs, errors, reports, and tests. If the registered client rejects the dynamically selected `127.0.0.1` callback, the CLI stops; it does not reuse another application’s tokens or bypass OAuth.

## Explicit ordinal test profile

The new test profile is deliberately separate from StagePilot’s production mapping where E7 velocity 100 means **Start next**:

- Profile: `setlist-ordinal-test`
- Channel: `1`
- Note: `112` (`E7`)
- Velocity: qualifying song ordinal (`1` through `127`)
- Position: exactly one proven musical beat after song start
- Library bank: the one existing, unambiguous Default MIDI Bank
- Bus: an explicitly selected stable bus

Headers, notes, non-song items, ambiguous items, and unsupported targets do not consume ordinals. Items are sorted by numeric setlist position first. A `--song-position` filter is applied only after the complete setlist receives ordinals. More than 127 qualifying songs fails before any write.

The position adapter requires a schema with provable measure/bar, beat, and optional tick starts. It increments only the beat and preserves the earliest measure/bar and tick. Millisecond-only, tick-only without PPQ, missing-beat, and ambiguous schemas fail closed.

## Safe real-setlist workflow

Configure a dedicated MIDI bus by stable ID:

```sh
./bin/stagepilot-cues configure
```

Run the guided read-only workflow:

```sh
./bin/stagepilot-cues test-real-setlist
```

Or prepare an exact setlist directly:

```sh
./bin/stagepilot-cues prepare \
  --setlist-id SETLIST_ID \
  --cue-profile setlist-ordinal-test
```

The terminal, JSON, CSV, and text reports separately show setlist position, song ordinal, velocity, bank ID, bus ID, and resolved position. Dry-run performs no create calls.

Test exactly one song first:

```sh
./bin/stagepilot-cues apply \
  --setlist-id SETLIST_ID \
  --song-position POSITION \
  --cue-profile setlist-ordinal-test
```

Type `APPLY SETLIST_ID POSITION` when prompted. The utility re-inspects immediately, creates at most one event, re-reads the complete scoped event list, and verifies every canonical field. Refresh or reopen the setlist in Playback and confirm E7 is emitted with the displayed ordinal velocity.

Verify without writing:

```sh
./bin/stagepilot-cues verify \
  --setlist-id SETLIST_ID \
  --song-position POSITION \
  --cue-profile setlist-ordinal-test
```

Only after the one-song result is confirmed should a separate full-setlist apply be considered:

```sh
./bin/stagepilot-cues apply \
  --setlist-id SETLIST_ID \
  --cue-profile setlist-ordinal-test
```

That command requires `APPLY ALL SETLIST_ID`. It is never run by the guided workflow.

## Safety and limitations

- The Default MIDI Bank must be proven by explicit server metadata or exactly one normalized documented default name.
- No bank is ever created, copied, replaced, imported, uploaded, or deleted.
- Existing MIDI events are never changed or deleted.
- Exact cues are skipped; wrong velocity, wrong position, duplicates, malformed events, and matches on another bus or bank are conflicts.
- A reusable Library target appearing more than once cannot safely hold multiple ordinal velocities and is skipped unless the payload proves distinct writable Cloud Arrangements.
- A Library cue is stored on a reusable asset. Its ordinal is tied to the current test setlist order and is not suitable as a permanent production cue.
- Test cues must be removed manually in MultiTracks/Playback; deletion is intentionally unavailable.

Apply uses an atomic journal. An interrupted or uncertain create is reconciled through read-back before any retry. Re-running after a verified create is idempotent.

## Reports and recovery

Reports are written under the configured application-data report directory. They contain sanitized normalized data, not raw private MCP responses. Save live schemas for diagnosis with:

```sh
./bin/stagepilot-cues tools --output multitracks-tools.sanitized.json
```

If the Default bank, target, setlist, bus, organization, position semantics, or existing cue state is ambiguous, stop and resolve it manually. Logout revokes tokens when supported and removes local credentials:

```sh
./bin/stagepilot-cues auth multitracks logout
```

If OAuth reports that the loopback `redirect_uri` is not valid for the client application, that registered client cannot be used by this standalone CLI. Request a StagePilot/native client registration that permits dynamic `http://127.0.0.1:{port}/oauth/callback` redirects. Do not substitute another application’s tokens.

## Developer checks

```sh
npm --prefix tools/multitracks-cues run typecheck
npm --prefix tools/multitracks-cues test -- --run
npm --prefix tools/multitracks-cues run lint
npm --prefix tools/multitracks-cues run build
```

