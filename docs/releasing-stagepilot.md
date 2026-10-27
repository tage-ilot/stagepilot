# Releasing StagePilot

The tag-triggered `.github/workflows/release-macos.yml` is the coordinated
Windows x64, Intel macOS, and Apple Silicon macOS release pipeline. Its
historical filename is retained to avoid creating a competing pipeline. It
publishes `latest.json` only after every referenced updater artifact and
signature is available.

The macOS community release is ad-hoc signed without Hardened Runtime; it does
not require Apple Developer ID or notarization. See
[macOS ad-hoc signing](macos-adhoc-signing.md).

## Release channel

StagePilot ships from `tage-ilot/stagepilot` with plain `X.Y.Z` versions (for example
`2.41.0`). There is one channel: every install checks GitHub
`releases/latest/download/latest.json`, not a control-plane broker. The base config and
both release overlays select that endpoint, and no `STAGEPILOT_RELEASE_TOKEN` or broker
deployment is required. Never embed a GitHub PAT in Tauri, frontend code, or an artifact.

Read the current latest with `gh release view --repo tage-ilot/stagepilot --json tagName,isDraft,assets`
before choosing a new, unused version. A failed release still occupies its tag, so
the next release uses a higher version; tags are never moved or reused.

## One-time updater key setup

Generate the long-term key outside the repository. This prompts for a password:

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\.tauri"
npm --prefix desktop exec -- tauri signer generate -w "$env:USERPROFILE\.tauri\stagepilot-updater.key"
```

Copy the complete contents of `stagepilot-updater.key.pub`—not its path—into
`desktop/src-tauri/tauri.conf.json` as `plugins.updater.pubkey`, replacing
`STAGEPILOT_UPDATER_PUBLIC_KEY_REQUIRED`.

Keep `stagepilot-updater.key` and its password in a secure offline backup.
Never commit them. Losing the key prevents installed updater-enabled copies
from accepting future updates.

Configure GitHub without printing either secret:

```powershell
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo tage-ilot/stagepilot < "$env:USERPROFILE\.tauri\stagepilot-updater.key"
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo tage-ilot/stagepilot
```

The second command prompts securely. The release fails before building if
either secret is absent. The private key is never passed to frontend code.

## Publication

1. Update versions in the Tauri config, the desktop Cargo manifest and lockfile, and the
   desktop and frontend packages (`node scripts/set-release-version.mjs X.Y.Z` does it all;
   the Rust crates take the version from `tauri.conf.json` at build time).
2. Update `CHANGELOG.md`.
3. Run:

   ```powershell
   node scripts/validate_versions.mjs vX.Y.Z
   npm --prefix frontend run lint
   npm --prefix frontend run typecheck
   npm --prefix frontend test -- --run
   npm --prefix frontend run build
   cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
   cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test --manifest-path desktop/src-tauri/Cargo.toml
   cargo check --manifest-path desktop/src-tauri/Cargo.toml
   ```

4. Commit and push reviewed source.
5. Tag the exact version and push it:

   ```powershell
   git tag -a vX.Y.Z -m "StagePilot X.Y.Z"
   git push origin main
   git push origin vX.Y.Z
   ```

6. Watch **Release StagePilot**. It validates configuration and tests, builds
   native `darwin-aarch64`, `darwin-x86_64`, and `windows-x86_64` artifacts,
   verifies the final `.app`, DMG, and updater archive by starting their packaged
   backends, keeps the release draft, generates and validates `latest.json`,
   uploads user-facing installers and updater payloads, uploads `latest.json`
   last, then publishes the release. Standalone `.sig` files remain internal
   build inputs because their contents are embedded in `latest.json`.

### Release candidates and the Latest channel

Tags with a pre-release suffix (`vX.Y.Z-rc.N`, `-beta.N`, `-alpha.N`) are built exactly
like stable tags and published by `scripts/publish_release.mjs` as GitHub
**pre-releases**: `prerelease: true`, never `make_latest`. GitHub's
`/releases/latest` ignores drafts and pre-releases, so
`.../releases/latest/download/latest.json` (the updater endpoint compiled into every
installed app) keeps pointing at the last stable release and an rc cannot be offered by
the in-app updater. Plain `vX.Y.Z` tags are published and marked Latest. See
`docs/creating-a-release.md` for the rc procedure, the unsigned
**Rust installer CI** workflow and rollback (delete the pre-release; production is
untouched).

Never substitute a checksum for a Tauri signature. The macOS updater payload is
`.app.tar.gz`, not the `.dmg`.

## Bootstrap and two-update test

Use three new versions: bootstrap X.Y.0, then X.Y.1 and X.Y.2. Manually install
and approve X.Y.0. Confirm no button while current. Publish X.Y.1, confirm the
button is beside the logo, cancel once to prove no download starts, then accept
and observe unattended progress/relaunch. Verify version, button disappearance,
success message, visibility, focus, window geometry, maximized/fullscreen
state, and quarantine/signing output. Repeat with X.Y.2.

Record results on both Intel and Apple Silicon Macs. The workflow cannot prove
Gatekeeper behavior without this hardware test.

## Lesson: regular CI and the release path must run the same validation

`v2.42.0-rc.2` failed "Validate application" on all three builds while regular CI was green
on the same commit, because the release workflows ran a different command sequence than CI
(a macOS-only Tauri overlay needing a sidecar the Rust build does not have, an Intel
timing-dependent test, a POSIX-only call that failed type checking on Windows). The release
validation is now one script, `scripts/validate_application.sh`, called by
`release-macos.yml`, `release-windows.yml`, `create-release.ps1` and the no-tag dry run
`release-validate.yml`. Run the dry run on `main` before every tag.
