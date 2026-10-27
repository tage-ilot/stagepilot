# Desktop packaging and server supervision

StagePilot's release builds are self-contained Windows and macOS desktop applications. They
bundle the React dashboard, the Tauri shell, the Rust server (linked into the shell and run
in-process) and `cloudflared` for Remote access. End users do not need Python, Node.js,
Rust or PowerShell.

## Build the installer

The build computer needs Node.js, npm, Rust (stable) and the Tauri Windows prerequisites.
Python 3 is needed only for the small `scripts/stage_cloudflared.py` helper, which downloads
and checks the pinned `cloudflared` binary.

```powershell
npm ci --prefix frontend
npm ci --prefix desktop
npm --prefix desktop run build
```

The desktop build:

1. builds the frontend production assets;
2. stages the verified `cloudflared` binary;
3. compiles the Tauri application, which links `stagepilot-server`, and creates an NSIS
   installer. The frontend is embedded as the `stagepilot_web` resource.

The installer is written below `desktop/src-tauri/target/release/bundle/nsis/`. Installer
outputs are intentionally ignored by Git. `npm --prefix desktop run build:ci` builds the same
installer without updater signing, for CI.

## Build the macOS disk images

macOS applications must be compiled on macOS. The **Release StagePilot** workflow
(`release-macos.yml`) builds separate Apple Silicon and Intel disk images on native
runners, re-signs them with the stable StagePilot identity (see `macos-adhoc-signing.md`),
verifies the app, DMG and updater archive, launches the built app once and attaches the
artifacts to the GitHub release. The same build can be run on a Mac with:

```sh
npm ci --prefix frontend
npm ci --prefix desktop
npm --prefix desktop run build:mac
```

The disk image is written below `desktop/src-tauri/target/release/bundle/dmg/`.

## Runtime lifecycle

On launch, the desktop shell reads the saved `server_port` from StagePilot's ordinary
settings file. An explicit `STAGEPILOT_PORT` environment override still takes priority for
development and diagnostics.

The shell then:

1. probes the configured loopback port;
2. reuses an existing StagePilot server without claiming ownership of it;
3. reports a clear failure if another application owns the port;
4. starts the in-process server when the port is available, passing its configuration
   (host, port, settings path, Remote state directory, bundled `cloudflared` and frontend)
   through the process environment;
5. polls `/api/v1/health` for up to 30 seconds;
6. publishes supervisor status to the connecting screen; and
7. stops the server gracefully when StagePilot exits: the HTTP listener drains, then Remote,
   the plugins and the state service stop in reverse order. The bundled `cloudflared` child
   is recorded in a pidfile, so a crashed run's leftover tunnel process is adopted or
   cleaned up on the next launch.

The server stays loopback-only unless LAN access is saved in the settings. The frontend
receives only a narrow custom status command; it does not receive general shell or
file-system permissions.

## Release smoke test

Test every installer candidate on a Windows account without repository tooling:

1. Install and launch StagePilot from the Start menu.
2. Confirm no terminal window appears and the dashboard connects automatically.
3. Complete first-launch configuration and close StagePilot.
4. Confirm no `stagepilot-desktop.exe` or `cloudflared.exe` process remains.
5. Launch again and confirm configuration and credentials persist.
6. Install the next version over the existing version and repeat the checks.
7. Uninstall StagePilot and confirm user settings remain available for an upgrade
   unless the release policy explicitly adds a separate data-removal option.

Execute this flow on Windows x64, macOS arm64 and macOS x64, including an actual reboot and
an in-app update from the previous release.

The Windows installer is not code-signed, so Windows may display an unknown-publisher
(SmartScreen) warning. The macOS builds are self-signed rather than notarized; see
`macos-adhoc-signing.md`.
