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
(for example `2.41.0`; the suggestion is one patch past the highest tag) or
`X.Y.Z-rc.N` release candidates (see below).
The helper functions are tested with `pwsh -NoProfile -File scripts/release-helpers.test.ps1`.

## Cutting a release candidate (Rust installers)

A release candidate (rc) is a pre-release build of the **Rust (in-process)** variant: the
app starts the Rust `stagepilot-server` inside the desktop process, ships no Python
sidecar (no PyInstaller `stagepilot-backend`), and bundles the built frontend plus
`cloudflared`. Plain `X.Y.Z` tags keep the Python sidecar until phase 7 of the Rust
migration (`docs/rust-migration/merge-plan.md`).

```powershell
.\scripts\create-release.ps1 2.42.0-rc.1
```

Rules enforced by the script: only `X.Y.Z-rc.N`; the base version must be newer than the
latest stable tag; the rc number must be higher than every existing rc of that base;
`CHANGELOG.md` needs a `## [2.42.0-rc.1]` heading. rc tags never change the suggested
next stable version. The manual path is the same as for a stable release
(`docs/releasing-stagepilot.md`): set the version everywhere with
`node scripts/set-release-version.mjs 2.42.0-rc.1`, run the validation, commit, then tag
`v2.42.0-rc.1`. `-alpha.N` / `-beta.N` tags (manual only) behave like rc tags.

What the tag does: `release-macos.yml` reads the tag, selects variant `rust` for a
pre-release suffix, builds macOS Apple Silicon, macOS Intel and Windows x64 with
`tauri build --features in-process` (no `build:sidecar`), verifies the bundles
(`scripts/verify_macos_release_bundle.sh --variant rust` rejects any sidecar or Python
runtime file, then launches the app's server and checks `/api/v1/health`), smokes the
built app once, signs the updater payloads as usual, and publishes through
`scripts/publish_release.mjs`.

### Why an rc never reaches the installed app

The in-app updater reads
`https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json`. GitHub's
`/releases/latest` resolves to the most recent release that is neither a draft nor a
pre-release. `publish_release.mjs` creates a pre-release tag's release with
`prerelease: true`, never sends `make_latest`, and fails if GitHub reports the published
release as anything but a pre-release. The rc's own `latest.json` is therefore reachable
only at `.../releases/download/v2.42.0-rc.1/latest.json`; the production channel keeps
serving the last stable release. An rc is installed by hand from its release page
(download the DMG / `-setup.exe`). Because `2.42.0-rc.1` is newer than the current stable
version, an installed rc does not offer a downgrade; the next stable tag supersedes it.

### Installer artifacts without tagging

Run **Rust installer CI** (`rust-installer-ci.yml`, *Run workflow*) on any branch. It builds
the same three Rust installers without secrets, publishes nothing, launches each built
app once (health from the in-process server, no Python process, clean stop) and keeps the
installers as workflow artifacts for 3 days. These builds are not updater-signed.

### Rolling back a release candidate

Delete the pre-release on GitHub (Releases -> the rc -> Delete, or
`gh release delete v2.42.0-rc.1 --cleanup-tag`). The production channel is untouched: the
rc was never "Latest", so `releases/latest/download/latest.json` was never changed. A
published release tag is not reused; the next attempt is `-rc.2`. Machines that installed
the rc by hand are reinstalled from the last stable DMG / installer.
