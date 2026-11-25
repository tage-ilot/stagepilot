# StagePilot ChatGPT agent CLI

## Architecture and trust boundary

Optional agent mode embeds the official Codex App Server as a local JSON-RPC child process:

```text
user → StagePilot CLI → Codex App Server → StagePilot dynamic tools
                                           ↓
                           deterministic planner/gateway/verifier
                                           ↓
                                    MultiTracks MCP
```

Codex supplies managed ChatGPT browser or device-code authentication and natural-language coordination. StagePilot retains its separate MultiTracks OAuth connection. Neither connection substitutes for the other, and StagePilot never asks Codex for a MultiTracks token.

The pinned runtime is the official `@openai/codex` npm package at version 0.146.0, installed from the package lock with npm integrity verification. Setup generates temporary JSON and TypeScript protocol schemas and checks the account, thread, and dynamic-tool methods used by StagePilot.

## Account isolation

StagePilot sets `CODEX_HOME` to its application-data directory before launching App Server. Codex owns token persistence and refresh. StagePilot interacts only through `account/login/start`, `account/read`, and `account/logout`; it does not parse credential files. This login is separate from any global Codex installation.

Browser login opens the managed authorization URL. Device-code login prints the one-time verification URL and code. Status masks email unless JSON output is explicitly requested. External-token and cookie-import modes are not implemented.

## Agent controls

Threads use an empty StagePilot application-data working directory, read-only sandboxing, disabled web search, and approval policy `never`. StagePilot rejects command, file-change, network-permission, and MCP-elicitation requests. External setlist content is passed only as structured tool data and is explicitly classified as untrusted data in the versioned agent instructions.

The model receives these client-executed tools:

- `stagepilot_connection_status`
- `stagepilot_list_upcoming_setlists`
- `stagepilot_inspect_setlist`
- `stagepilot_prepare_ordinal_cues`
- `stagepilot_verify_ordinal_cues`
- `stagepilot_propose_apply_one`
- `stagepilot_propose_apply_all`

There is no direct apply, HTTP, shell, raw MCP, credential, create, update, delete, bank, upload, copy, or import tool.

Proposal tools perform a fresh dry-run and store the authoritative proposal only in process memory. Each proposal has a cryptographically random ID, five-minute expiry, exact positions, operation count, profile, and SHA-256 digest of canonical deterministic plan data. Before execution StagePilot re-runs the plan, compares the digest, displays the current plan, and requires the existing typed confirmation. The existing applier and verifier remain authoritative.

## Privacy

Before first use, StagePilot explains that the user request and normalized MultiTracks data needed for the response may be sent to OpenAI. It never sends OAuth tokens, client secrets, authorization codes, cookies, Keychain contents, credential files, or raw MCP responses. Consent stores only acceptance, disclosure version, and timestamp; a version change requires renewed acceptance.

Normal diagnostics contain prompt version, pinned Codex version, shortened identifiers, tool names, and sanitized outcomes—not raw prompts, conversations, protocol traffic, hidden reasoning, or credentials.

`agent sessions delete` removes StagePilot’s local resume shortcut. Codex 0.146.0 did not reliably complete `thread/delete` during local validation, so StagePilot does not pretend the managed Codex thread was erased; use Codex account controls for managed-history deletion.

## Setup and operation

```sh
./scripts/setup-multitracks-cues.sh
./bin/stagepilot-cues setup
./bin/stagepilot-cues codex version

./bin/stagepilot-cues auth chatgpt login --browser
./bin/stagepilot-cues auth chatgpt login --device-code
./bin/stagepilot-cues auth chatgpt status
./bin/stagepilot-cues auth chatgpt logout

./bin/stagepilot-cues auth multitracks login
./bin/stagepilot-cues ask "Prepare the next unambiguous Sunday setlist. Do not apply it."
./bin/stagepilot-cues chat
```

`--model` is optional and validated with `model/list`; otherwise Codex chooses the account’s supported default. ChatGPT usage limits and workspace restrictions stop only agent mode. Deterministic commands remain available.

## Recovery and compatibility

App Server transport is newline-delimited JSON-RPC over stdio. StagePilot enforces initialization ordering, request correlation, timeouts, line-size limits, diagnostics redaction, pending-request cancellation, and bounded shutdown. Unknown server requests are rejected.

Dynamic tools are experimental in Codex 0.146.0. Updating Codex requires:

1. Change the exact package version.
2. Run `npm install` to refresh lockfile integrity.
3. Run both App Server schema generators.
4. Verify `account/login/start`, `thread/start`, `turn/start`, `item/tool/call`, and the dynamic-tool response schema.
5. Run the complete tests and a read-only login/agent smoke test.

If dynamic tools become incompatible, agent mode fails closed while deterministic mode remains available. If MultiTracks rejects the loopback callback, request a dedicated StagePilot native OAuth registration; never reuse ChatGPT connector tokens or impersonate its redirect URI.

The pinned schema exposes `mcpServer/oauth/login`, `mcpServerStatus/list`, and `mcpServer/tool/call`, but the supported Codex configuration does not expose documented fields for supplying an arbitrary MCP OAuth client ID, client secret, scopes, and callback registration as one static native-client configuration. Direct Codex-to-MultiTracks OAuth is therefore not enabled. The production path remains Codex → StagePilot dynamic tools → StagePilot OAuth gateway → MultiTracks MCP.

