# Playback frontend handoff

Implements the existing Playback configuration window on top of backend arbitration head `0e7c1d1312ce63efdbf73a67a68a7d56ed6bb0d6` (PR #83, `feat/playback-api-source`). No backend, native MIDI, network MIDI, gateway, dependency, CI workflow, release or shared-checkout changes.

## UI and contract

- Default panel: Playback API on/off, API-specific connected address, saved manual host override (blank restores automatic local-first/LAN discovery), Find / Scan, recent normalized events.
- One header connection light and backend `reason` for overall Playback. API-only controls use `sources.playback_api.connected`, never overall `connected`. There is no MIDI fallback: in API mode, when Playback is offline the status is "Not connected. Looking for Playback…". The stopped timer keeps its existing wording and shows the frozen remaining time.
- "Alternate connection: MIDI settings" starts collapsed on every mount; source selector and all prior MIDI mapping/input/simulation/monitor controls stay there. MIDI handlers and backend behavior unchanged.
- Discover Song Order requires the plain-language confirmation dialog. Disconnected/playing/running states disable it and explain why. Only the exact backend `POST /discover-song-order` with `{confirm:true}` is available; no individual transport controls. Status polling continues during this long request; failed HTTP 200 results show `last_error`.
- Connection health remains green with stale/unknown API order. Separate start-readiness warning/check requires discovery; invalid API order does not block readiness when MIDI actually owns.
- All calls use existing authenticated API request helper. Configuration polling is disabled for Viewers, serialized at 750 ms, and stops on unmount/access removal. Generation/revision guards reject stale results; mutations serialize. Source changes read fresh settings first to preserve MIDI values and discovered order.
- Find's backend response means discovery was requested, not completed. UI polls API connection/error; pending UI is bounded to 60 seconds and explains that reconnection continues afterward (does not cancel backend reconnect). No invented backend scan field.
- Backend event timestamps are monotonic seconds; UI labels them honestly, not as wall-clock times. Unknown/stale IDs never receive invented song numbers.
- Schema 2 and Playback API settings added to frontend types; schema 1/optional API settings retained as compatibility for old snapshots/fixtures. New and migrated defaults are backend-owned.

## Actual local execution

In a clean throwaway clone, Node `22.23.3`:

- TypeScript `tsc --noEmit`: passed.
- Vitest full suite: **37 files / 303 tests passed** (35 additional tests relative to 268-test backend parent).
- `npm run build`: passed (Vite 6.4.3, 102 modules).
- `npm run lint`: 0 errors, 3 existing warnings (Dashboard unused disable, ErrorBoundary unused disable, PlanningCenterSetupPanel fast refresh). No added warnings.
- Initial full run exposed five old Dashboard label assertions; updated those to the new Playback labels. No remaining failures.

Backend (unchanged from base), frozen dev sync passed:

- `uv run --frozen pytest`: **566 passed, 1 Windows-only platform skip, 1 existing Starlette/httpx warning**, 42.95 s.
- Ruff check: passed. Format check: 139 files already formatted.
- Strict mypy: 139 source files passed.
- `git diff --check`: passed.

Regression coverage includes default/collapsed/expanded "Alternate connection: MIDI settings", source switch, scan pending/result/timeout, host validation, enable/disable, confirmation/cancel/recheck of playing state, discovery progress/order/stale/failure, recent events, API-only/MIDI-only/both/neither shared panel/dashboard/checklist status, invalid order vs connection readiness, polling failover/cleanup/access removal/stale responses, serialized operations, failed discovery 200/conflicts, exact routes/confirmation payload and fresh settings preservation.

No real Playback or operator-network probe, no merge/tag/release/deploy. Browser/device/mobile visual QA and full combined-stack/release acceptance belong to existing downstream `t_9ad0892e`. `npm ci` reported 14 audit findings in the unchanged dependency lock (4 moderate, 10 high); dependency changes were out of scope.

Required `AGENTS.md` and `docs/server-midi-agent-handoff.md` were absent from the tracked clean clone; read the authoritative shared repository copies without modifying that checkout. `ARCHITECTURE.md` and backend typed API/module definitions were read from this exact clone.
