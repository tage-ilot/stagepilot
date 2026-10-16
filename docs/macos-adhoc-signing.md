# macOS ad-hoc signing

StagePilot's community macOS release does not currently use Apple Developer ID
or notarization. It uses the supported ad-hoc identity (`-`), so users may need
to right-click **Open** or approve StagePilot under **System Settings → Privacy
& Security** after downloading it.

## Why Hardened Runtime is disabled

The backend is a PyInstaller one-file executable. At startup it extracts its
embedded Python framework into a temporary `_MEI...` directory. Tauri's default
macOS Hardened Runtime setting signs external sidecars with the `runtime` flag.
Apple's library validation then rejects the extracted ad-hoc Python framework
because an ad-hoc build has no Apple Team ID.

StagePilot therefore explicitly combines:

- `signingIdentity: "-"` for ad-hoc application signing;
- `hardenedRuntime: false` for this non-notarized release flavor; and
- `minimumSystemVersion: "12.0"`.

This reproduces the signature produced by the successful incident repair
without requiring users to repair an installed application. StagePilot does not
use a self-signed certificate and does not add a disable-library-validation
entitlement.

Apple explains that Hardened Runtime enables library validation, which requires
loaded code to be Apple-signed or signed by the same Team ID:
[Disable Library Validation Entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.disable-library-validation).
Tauri documents `hardenedRuntime` and its default in the
[Tauri 2 configuration reference](https://v2.tauri.app/reference/config/).
PyInstaller documents its macOS binary processing and code-signing behavior in
its [macOS feature notes](https://pyinstaller.org/en/stable/feature-notes.html#macos-binary-code-signing).

## Final-artifact verification

`scripts/verify_macos_release_bundle.sh` verifies the actual `.app`, DMG, and
`.app.tar.gz` updater payload. For each format it:

1. checks the expected Intel or Apple Silicon architecture;
2. verifies the application bundle recursively with `codesign`;
3. rejects a backend sidecar carrying the Hardened Runtime flag or an unexpected
   Team ID;
4. verifies the main executable and sidecar do not require newer than macOS
   12.0; and
5. starts that exact packaged sidecar on an isolated port and requires successful
   `/api/v1/health` and `/api/v1/state` responses.

The GitHub release workflow runs all three checks on native Intel and Apple
Silicon runners before it collects or uploads release assets.

Developers can inspect a built sidecar with:

```sh
codesign --verify --strict --verbose=4 /path/to/stagepilot-backend
codesign -d --verbose=4 /path/to/stagepilot-backend 2>&1
codesign -d --entitlements :- /path/to/stagepilot-backend 2>/dev/null
xcrun vtool -show-build /path/to/stagepilot-backend
```

The backend log is
`~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`. StagePilot keeps
one rotated previous log after the current file reaches 5 MiB.

## Future Developer ID releases

A future paid Developer ID build should use a real Developer ID Application
identity, Hardened Runtime, secure timestamps, appropriate narrowly scoped
entitlements if needed, and Apple notarization. That is a separate release
flavor and must be tested against PyInstaller's extracted libraries. Tauri
updater signatures remain independent: they authenticate updates inside
StagePilot and are required whether Apple signing is ad-hoc or Developer ID.

## Stable signing identity (from 1.1.104-beta.30)

Releases are re-signed after `tauri build` by `scripts/resign_macos_release.sh` with a
free, self-created code-signing certificate ("StagePilot Beta Code Signing", public
certificate: `docs/stagepilot-signing-cert.crt`, SHA-1 `78:E4:F3:FA:D1:52:19:6A:9D:C3:58:62:9F:7F:36:C5:C4:B5:7B:74`,
valid to 2041). Nested code is signed inside-out with fixed identifiers (no `--deep`,
no Hardened Runtime): `org.stagepilot.cloudflared`, `org.stagepilot.backend`, then the app
`org.stagepilot.desktop`. The designated requirement becomes

    identifier "org.stagepilot.desktop" and certificate root = H"78e4f3fa..."

and is identical across builds (a self-signed leaf is its own root), so Local Network and
Keychain approvals follow the app across updates. The updater archive is rebuilt from the
re-signed bundle and given a fresh minisign signature; the DMG is repacked the same way.
The private key lives only in the `MACOS_SIGNING_P12_BASE64` / `MACOS_SIGNING_P12_PASSWORD`
repository secrets plus an owner-only offline backup; losing it costs one more round of
prompts. `scripts/macos_signing_stability.sh` (CI: "macOS signing stability") builds two
different bundles and fails on any requirement/identifier drift, and
`verify_macos_release_bundle.sh` pins the requirement in release builds.

This is NOT Apple notarization: Gatekeeper still shows its first-launch warning for a
downloaded app. The goal is that approvals survive later updates.

### One-time prompts after the first update to the stable identity

macOS sees a new identity once. Click, once each: **Allow** for Local Network (run Scan
Network), **Always Allow** for the Keychain password prompt (Planning Center), and approve
any "App Management"/background-item prompt. Later updates should show none.

## Keychain and Local Network prompts (ad-hoc builds, up to 1.1.104-beta.29)


Prompt sources on an ad-hoc build, what StagePilot does about each, and what cannot
be fixed without a paid Developer ID:

| Prompt | Cause | Status |
| --- | --- | --- |
| Login Keychain password (Planning Center secrets) | A Keychain item's access list names the program that created it. An ad-hoc program's designated requirement is its `cdhash`, which changes with every build, so each update looks like a new program and macOS asks for the login password. | Reads are minimized (none when Planning Center is not configured, one per launch otherwise). Click **Always Allow** once after each update. Cannot be removed without a stable signing identity. |
| Local Network access | Granted per executable. The packaged backend is a separate executable with its own identity, and a rebuilt sidecar is a new one. | Scan now detects and explains it; see `playback-api.md`. |
| Gatekeeper first open | Unsigned/unnotarized app. | Unavoidable without Developer ID. Right-click **Open** once. |
| Updater replacing the app | Tauri swaps the bundle; macOS may re-ask the above. | Unavoidable ad-hoc. |

A fixed designated requirement (`codesign -i org.stagepilot.backend -r=...`) would
make the Keychain and Local Network grants survive updates, but: Tauri signs the
sidecar and the updater archive during `tauri build`, so re-signing afterwards
invalidates the archive's minisign signature, and an identifier-only requirement
lets any ad-hoc binary with that identifier match the grant. That change was not
shipped without an end-to-end update test from an installed beta, because a
mistake would strand installed betas.
