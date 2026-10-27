# Rust migration: merge plan and post-merge cleanup

Branch `release/rust-integration` (one draft PR into `main`) contains every code and docs change
of the 18 stacked draft PRs #236-#255. The owner decides the merge. Nothing below is done before
that. Do not tag, release or publish as part of the merge.

## What the integration branch contains

Merged with `--no-ff` onto `main` (`9428dfa`): `feat/rust-test-gaps` (brings the whole line
phase1 -> 2 -> 6 -> 6c -> 6d), `fix/contract-today-date` (brings rig A), `docs/rust-rig-c`
(brings `feat/rust-lightkey`, rig B). The tip commits of `spike/rust-playback-client`,
`feat/rust-phase3a`, `feat/rust-phase3b`, `feat/rust-phase4`, `feat/rust-phase5`,
`feat/rust-phase6b-a` and `feat/rust-phase6b-b` are not ancestors of the integration branch, but
each is only the 2026-10-21 "remove the retired self-hosted runner" commit (or a merge of it), and
`main` already has that change from PR #252. Their code is in the branch through the phase 6
integration commits (checked with `git cherry`).

## Cleanup after the owner merges (in this order)

1. Merge the integration PR (owner). Confirm `main` CI is green on the merge commit.
2. Close as superseded, with a comment linking the merged PR, bottom of the stack first so no PR
   is left with a vanished base:
   `#236` spike, `#237` phase1, `#239` phase2, `#238` phase3a, `#241` phase3b, `#240` phase4,
   `#242` phase5, `#243` phase6, `#245` 6b-A, `#244` 6b-B, `#246` 6c, `#247` 6d, `#250` rig A,
   `#251` contract-today-date, `#249` Lightkey, `#253` rig B, `#254` rig C, `#255` test gaps.
   (Closing a PR whose base branch still exists does not retarget it; delete branches only after
   all of them are closed.)
3. Delete the old remote branches, top of the stack first:
   `feat/rust-test-gaps`, `fix/contract-today-date`, `docs/rust-rig-c`, `docs/rust-rig-b`,
   `docs/rust-rig-a`, `feat/rust-lightkey`, `feat/rust-phase6d`, `feat/rust-phase6c`,
   `feat/rust-phase6b-a`, `feat/rust-phase6b-b`, `feat/rust-phase6`, `feat/rust-phase5`,
   `feat/rust-phase4`, `feat/rust-phase3b`, `feat/rust-phase3a`, `feat/rust-phase2`,
   `feat/rust-phase1`, `spike/rust-playback-client`.
   Then delete `release/rust-integration` itself once merged.
4. Remove `release/rust-integration` and the old branch names from the `push.branches` list of
   `.github/workflows/rust.yml` (only `main` needs to stay).
5. Not part of this migration, leave alone: `feat/transparent-beta-guardrails`,
   `fix/planning-center-focus`.

## Phase 7 cut-over checklist (from PLAN.md)

Phase 7 means: Rust-only release; delete the Python backend, PyInstaller scripts and spec; update
docs and ARCHITECTURE.md; beta soak on jpl-graphics for a service weekend; then release.

| Item | State |
|---|---|
| All phases 0-6 on `main`, full CI on Linux, Windows, macOS Apple Silicon and Intel | Done on this branch once the PR is green; lands with the merge |
| Contract (frozen OpenAPI + golden transcripts) passes on Python and Rust, all profiles | Done: base 103, pco 96, outputs 75, remote 91, diagnostics 26, static 34, lightkey 54 |
| Lightkey record/replay (PR #248) ported | Done (Rust code, golden OSC, contract profile) |
| Python tests mapped or ported to Rust | Done: `phase6-test-map.md`, `lightkey-test-map.md` |
| Tauri shell linking the server in process (cargo feature `in-process`, sidecar default) | Done as a feature; default build still the Python sidecar |
| Rig A (graphics side by side, read-only) | Done, `rig-verification-a.md` |
| Rig B (active MIDI, ProPresenter, Lightkey) | Done, `rig-verification-b.md` |
| Rig C (soak, Lightkey Python comparison, Look) | Open: deferred, `rig-verification-c.md`; run by a separate card |
| Installer chain proof against the last Python release on each OS | Open: needs real installers |
| In-process shell on a real install (window, tray, updater hand-off, autostart, Gatekeeper) | Open: needs a real machine per OS |
| Direct `keyring` credential store on Windows and macOS (real keychain) | Open |
| Windows job objects / macOS zombie handling for `cloudflared` | Open |
| Make the in-process variant the default shell and stop shipping the PyInstaller sidecar | Open (phase 7 PR) |
| Delete `backend/`, PyInstaller scripts and spec; update ARCHITECTURE.md and docs | Open (one deliberate phase 7 PR, after the soak) |
| Beta soak on jpl-graphics for a live service weekend | Open |
| Release (owner approves) | Open |
