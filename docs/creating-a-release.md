# Creating a StagePilot release

From the repository root in PowerShell, run the release script (a local `create-new-release.ps1` / `.cmd` launcher that calls it is
not tracked in git):

```powershell
.\scripts\create-release.ps1
```

The command reads the version tags (`vX.Y.Z`) from `tage-ilot/stagepilot` and suggests
one patch past the highest. Existing tags from an unsuccessful release are included, so
the suggested version is never already occupied. Press Enter to accept that version or
type a different one; versions that are not plain `X.Y.Z` or not newer than the latest
tag are refused. The destination defaults to `main`; another branch may be selected when
needed. It first checks that `origin` is `tage-ilot/stagepilot` (it will not tag or push
anything else) and that `CHANGELOG.md` already has a `## [VERSION]` heading, before it
changes anything. It then:


1. Shows the release title, source branch, destination branch, and current changes.
2. Requires one final typed confirmation: `YES`.
3. Updates every StagePilot application and lockfile version.
4. Refreshes the Python, frontend, and desktop lockfiles.
5. Runs backend, frontend, desktop/Rust, and MultiTracks CLI validation.
6. Displays the release diff without opening an interactive pager.
7. Commits and pushes all intended changes to the confirmed destination.
8. Creates and pushes the version tag.
9. Finds the GitHub release run and prints its URL. It does NOT sit and watch the
   build (20-40 minutes): the Hermes host's `stagepilot-release-watch` job polls
   the run with plain `gh` calls (no model) and sends one WhatsApp message when the
   release is published or when a job fails, with the failing step and log tail.
   To watch in this window instead, set `STAGEPILOT_RELEASE_WATCH=1` first.

After the release plan is confirmed, no further input is required.

You may supply the version directly:

```powershell
.\scripts\create-release.ps1 2.41.1
```

For an unattended invocation after reviewing the working tree, add `-Yes`:

```powershell
.\scripts\create-release.ps1 2.41.1 -Yes
```

`-Yes` uses `main` by default. To publish the release commit to another branch:

```powershell
.\scripts\create-release.ps1 2.41.1 -TargetBranch release/test -Yes
```

The release stops immediately if a test fails, the tag already exists, GitHub
authentication is unavailable, the current checkout is behind `origin/main`,
or generated/sensitive files are staged. A failure before confirmation makes
no changes; a later validation failure leaves changes local without committing,
tagging, or pushing.

This helper releases `tage-ilot/stagepilot` only, with plain `X.Y.Z` versions
(for example `2.41.0`; the suggestion is one patch past the highest tag).
The helper functions are tested with `pwsh -NoProfile -File scripts/release-helpers.test.ps1`.
