# Desktop packaging and backend supervision

StagePilot's release builds are self-contained Windows and macOS desktop
applications. They bundle the React dashboard, Tauri shell, and a
PyInstaller-frozen Python backend. End users do not need Python, uv, Node.js,
Rust, or PowerShell.

## Build the installer

The build computer needs the normal repository prerequisites: Python 3.12, uv,
Node.js, npm, Rust, and the Tauri Windows prerequisites.

```powershell
npm ci --prefix frontend
npm ci --prefix desktop
npm --prefix desktop run build
```

The desktop build performs these steps:

1. Resolve the locked `packaging` dependency group in an isolated uv environment.
2. Freeze `stagepilot.__main__` into a one-file backend executable.
3. Rename and stage the executable using Tauri's Rust target-triple convention.
4. Build the frontend production assets.
5. Compile the Tauri application and create an NSIS installer.

The installer is written below
`desktop/src-tauri/target/release/bundle/nsis/`. Generated backend executables,
PyInstaller work files, and installer outputs are intentionally ignored by Git.

## Build the macOS disk images

macOS applications must be compiled on macOS. The **Release macOS** GitHub
Actions workflow builds separate Apple Silicon and Intel disk images using the
matching runner architecture. It freezes the native backend sidecar, builds the
Tauri application, uploads the `.dmg` as a workflow artifact, and attaches it to
the selected GitHub release.

Native CI packaging is currently deferred: the preserved Windows x64, macOS
Apple Silicon, and macOS Intel jobs are explicitly disabled until matching
self-hosted native runners exist. The active `stagepilot-linux` runner performs
only Linux-compatible Cargo/Tauri checks and must not be treated as native
packaging or acceptance evidence.

The same build can be run on a Mac with:

```sh
npm ci --prefix frontend
npm ci --prefix desktop
npm --prefix desktop run build:mac
```

The disk image is written below
`desktop/src-tauri/target/release/bundle/dmg/`.

## Runtime lifecycle

On launch, the desktop shell reads the saved `server_port` from StagePilot's
ordinary settings file. An explicit `STAGEPILOT_PORT` environment override still
takes priority for development and diagnostics.

The shell then:

1. Probes the configured loopback port.
2. Reuses an existing StagePilot backend without claiming ownership of it.
3. Reports a clear failure if another application owns the port.
4. Starts the packaged backend when the port is available.
5. Polls `/api/v1/health` for up to 30 seconds.
6. Publishes supervisor status to the connecting screen.
7. Terminates the complete owned PyInstaller process tree when StagePilot exits.

The backend remains loopback-only. The frontend receives only a narrow custom
status command; it does not receive general shell or file-system permissions.

## Release smoke test

Test every installer candidate on a Windows account without repository tooling:

1. Install and launch StagePilot from the Start menu.
2. Confirm no terminal window appears and the dashboard connects automatically.
3. Complete first-launch configuration and close StagePilot.
4. Confirm no `stagepilot-backend.exe` process remains.
5. Launch again and confirm configuration and credentials persist.
6. Install the next version over the existing version and repeat the checks.
7. Uninstall StagePilot and confirm user settings remain available for an upgrade
   unless the release policy explicitly adds a separate data-removal option.

Execute this flow on Windows x64, macOS arm64, and macOS x64, including an actual
reboot and a signed in-app update from the previous release.

The application is not code-signed or notarized yet. Windows may display an
unknown-publisher warning, and macOS may require the user to approve the app in
Privacy & Security, until release signing is added during production hardening.
