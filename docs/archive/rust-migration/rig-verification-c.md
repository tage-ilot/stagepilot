# Rust migration: real-rig verification C (soak, Lightkey Python comparison, Look)

Status: **rig checks deferred**. No rig traffic was sent and no rig system was touched.

## Why deferred

The owner authorisation for full active testing starts 2026-10-21 00:00 America/Los_Angeles and lapses
2026-10-24 23:59. This task ran at 2026-10-20 22:51 PDT (2026-10-21 05:51 UTC), before the window opens,
so only build/prep work was done. (Rig verification B used the UTC date; the rule here is the Los Angeles date.)

## Prep done

- Newest stack head: `3cd8aa85` (PR #253, `docs/rust-rig-b`). Its only change over `af453579` is
  `docs/archive/rust-migration/rig-verification-b.md`, so no code differs.
- Binary: `stagepilot-headless` built with `stagepilot-headless/midir-backend` from CI run 39547406118
  (SHA `af453579ecee281610b3a873c824b345ab1513fd`, artifact `rig-headless-macos-15-intel`, 3-day retention).
  Re-downloaded and verified:
  - `stagepilot-headless` sha256 `aec67216242a4e5518907b88c08f19846d1c328ac681bc927c199ae8d28e876e`
    (same as rig B).
  - `midi-probe` sha256 `cc1101094d6548bf5adfc4f2aca6b55ce7b30a7602c683525006d8fbe5df6577`.
- CI state: the Linux and contract jobs of that run, and the CI run on `3cd8aa85`, are still queued because
  the only self-hosted runner `stagepilot-ci` is offline. The macOS and Windows Rust jobs are green.
  "Green on the exact head SHA" is therefore not yet met for the stack.

## Plan for the rig window (unchanged from the task)

1. Record pre-test state of Playback, ProPresenter, Lightkey, OLA and MIDI sessions to a file first.
2. 60 min soak, Rust vs a Python scratch server that must be shown connected to Playback (not
   `not_playback` / `WSClosed`) before any number is kept; samples of RSS/CPU every 30 s; <=5 commands/s.
3. Lightkey record/replay, Python build from main containing #248 as a scratch process vs Rust; single-cue
   and Stars/Triangles latency; evidence on whether `REPLAY_LEAD_SECONDS` (40 ms) should depend on cue fade.
4. ProPresenter read-only investigation of `/v1/look/current` id missing from `/v1/looks`; Rust vs Python.
5. Restore proof by before/after diffs.

## Not verified (everything rig-related)

- Soak RSS/CPU trend and whether Rust RSS plateaus (rig B: 11.7 -> 18.2 MB in 15 min, monotonic).
- Why the Python scratch server stays in `not_playback / WSClosed`.
- Python vs Rust Lightkey latency and the `REPLAY_LEAD_SECONDS` question.
- Look id mismatch and Rust/Python Look handling.
- Linux and contract CI on the stack head.
