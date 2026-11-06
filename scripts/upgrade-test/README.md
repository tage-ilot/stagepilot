# Upgrade test: released StagePilot -> current checkout

Proves that people updating from the stable release (default `v2.42.0`) to this checkout keep
their data and that nothing breaks, and records what the released app does when it meets data the
newer build wrote (the downgrade risk). Local only: loopback ports, scratch directories, fake
values. It never uses the preview's files or ports, a real Lightkey, the rig, or the real
`~/.config/StagePilot` (each server gets its own `HOME`, settings path and keyring service).
Not part of CI.

    scripts/upgrade-test/run.sh [report.md]      # default report: scripts/upgrade-test/out/REPORT.md

Exit code 0 = every check passed, 1 = a check failed, 2 = harness or build error.

## Prerequisites
- Linux or macOS, Rust toolchain (`cargo`), `git` with the release tag available (`git fetch --tags`), Python 3.9+ (stdlib only).
- About 3 GB free for two debug builds (first run builds both, about 3 minutes each; later runs reuse them).
- Env: `OLD_TAG`, `OLD_BIN` / `NEW_BIN` (prebuilt `stagepilot-headless`, skips that build), `TARGET_ROOT` (default `~/.cache/stagepilot-upgrade-test`), `KEEP=1` (keep scratch dirs).

## What it does
1. Builds `stagepilot-headless` from the tag (separate git worktree and cargo target dir) and from this checkout.
2. **Upgrade, twice** (stored Lightkey Detail `fine`, then `coarse`): starts the old build on a scratch dir and, through its HTTP API, sets a dashboard PIN, a second user, Services/Playback/Presentation/Lighting settings, two cue maps and two pads, then plays a song over loopback OSC (a UDP socket in the harness stands in for Lightkey) as the baseline. A `lightkey-takes.json` in the v2.42.0 format is in the directory throughout. It then hand-writes what v2.42.0 cannot create but mainline builds did write: thinned `source:"captured"` look rows without `fade_seconds`, a stored `lights.lightkey.detail`, and a Discover-written song order. Starts the new build on the same files and checks: starts, no `settings.json.corrupt-*`, every pre-existing settings value unchanged (listing what main adds), cue maps and pads unchanged, PIN and users, `detail_level` accepted and echoed, takes file untouched, song A replays the identical OSC messages as on the old build, file not rewritten by a plain start. Then main saves, and the old build is started on the result.
3. **Downgrade with new fields**: main writes `fade_seconds`, `effects`, `messages`, `source`; the old build starts on that file, saves, and has a row deleted, to record exactly what it keeps, drops or moves.
4. **Fresh install** of both builds with no files, and of main with a minimal seeded file.

## Known limits (stated in the report)
- Planning Center secrets live in the OS keychain, which a headless run does not have, so no token is stored (settings only keep an app id).
- Remote users can be listed and bootstrapped locally but only log in through the remote ingress; "users still work" means the same users, roles and an unchanged `identity.sqlite3`.
- The learned song order is written only by Discover Song Order (needs a real playback app); it is hand-written in the shape Discover writes.
- Cue-map replay is checked over real loopback UDP at 1x speed (about 3 s per song), not on a simulated clock.
