# macOS signing

StagePilot's community macOS release does not use Apple Developer ID or notarization.
Gatekeeper therefore shows its first-launch warning for a downloaded app: right-click
**Open** or approve StagePilot under **System Settings → Privacy & Security** once.

The goal of the signing setup is narrower: approvals that macOS ties to the app's identity
(Local Network, Keychain access) must survive updates.

## Stable signing identity

Release builds are re-signed after `tauri build` by `scripts/resign_macos_release.sh` with a
free, self-created code-signing certificate ("StagePilot Beta Code Signing", public
certificate: `docs/stagepilot-signing-cert.crt`, SHA-1
`78:E4:F3:FA:D1:52:19:6A:9D:C3:58:62:9F:7F:36:C5:C4:B5:7B:74`, valid to 2041). Code is signed
inside-out with fixed identifiers (no `--deep`, no Hardened Runtime): the bundled
`org.stagepilot.cloudflared`, then the app `org.stagepilot.desktop`. The designated
requirement becomes

    identifier "org.stagepilot.desktop" and certificate root = H"78e4f3fa..."

and is identical across builds (a self-signed leaf is its own root), so Local Network and
Keychain approvals follow the app across updates. The updater archive is rebuilt from the
re-signed bundle and given a fresh minisign signature; the DMG is repacked the same way.

The private key lives only in the `MACOS_SIGNING_P12_BASE64` / `MACOS_SIGNING_P12_PASSWORD`
repository secrets plus an owner-only offline backup; losing it costs one more round of
prompts. `scripts/macos_signing_stability.sh` (CI: "macOS signing stability") builds two
different bundles and fails on any requirement or identifier drift.

Tauri is configured with `signingIdentity: "-"`, `hardenedRuntime: false` and
`minimumSystemVersion: "12.0"` (`desktop/src-tauri/tauri.macos.conf.json`); the identity
above is applied afterwards. Updater signatures are independent: they authenticate updates
inside StagePilot and are required whatever the Apple signing is.

## Final-artifact verification

`scripts/verify_macos_release_bundle.sh` verifies the actual `.app`, DMG and `.app.tar.gz`
updater payload. For each format it:

1. checks the expected Intel or Apple Silicon architecture;
2. verifies the application bundle recursively with `codesign`, and in release builds pins the
   designated requirement above;
3. rejects the Hardened Runtime flag, any separate backend executable or Python runtime files,
   and a missing bundled frontend or `cloudflared`;
4. checks the Help Book is registered and searchable;
5. verifies the executable does not require newer than macOS 12.0; and
6. (without `--no-launch`) starts the app on an isolated port and requires `/api/v1/health`,
   a dashboard login, `/api/v1/state` and the bundled frontend to answer.

The release workflow runs the three static checks on native Intel and Apple Silicon runners,
then launches the app once with `scripts/smoke_rust_app.mjs` before collecting assets.

The application log is `~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`.
StagePilot keeps two rotated previous logs, rotates at 5 MiB and prunes old files at start to
stay under 100 MB.

## One-time prompts

macOS sees a new identity once. After the first update to the stable identity, click once
each: **Allow** for Local Network (run Scan Network), **Always Allow** for the Keychain
password prompt (Planning Center), and approve any "App Management" or background-item
prompt. Later updates should show none.

| Prompt | Status |
| --- | --- |
| Login Keychain password (Planning Center secrets) | Once per new identity; reads are minimized (none when Planning Center is not configured, one per launch otherwise). |
| Local Network access | Granted per executable; with a single executable and a stable identity it survives updates. Scan now detects and explains a refusal; see `playback-api.md`. |
| Gatekeeper first open | Unavoidable without Developer ID. Right-click **Open** once. |

## Future Developer ID releases

A paid Developer ID build would use a real Developer ID Application identity, Hardened
Runtime, secure timestamps, narrowly scoped entitlements if needed, and Apple notarization.
That is a separate release flavor and would be tested as one.
