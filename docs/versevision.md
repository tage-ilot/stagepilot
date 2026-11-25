# VerseVision in StagePilot

VerseVision turns a pastor's notes into ProPresenter verse slides. StagePilot now carries it: the
page runs inside StagePilot's own server, starts and stops with it, and is updated with it. There is
nothing to install, no second program and no second port.

## What the button is

**Configuration → Presentation → VerseVision** (the VerseVision card in the Presentation panel, under the Presenter Link) opens VerseVision in a new browser tab. In the desktop app it opens your default
browser. The page is `http://<this computer>:8765/versevision/` (the same address and port as the
dashboard), so you can also bookmark it.

- It uses the **Presentation connection**. Whatever host and port Presentation is connected to is
  what VerseVision talks to, and a change on the Presentation page is used by the next thing
  VerseVision does. VerseVision's own Config page keeps a ProPresenter host and port that StagePilot
  does not use and does not write.
- VerseVision reads and writes ProPresenter's library **on the computer StagePilot runs on**. Run
  StagePilot on the ProPresenter computer. With Presentation pointed at another computer VerseVision
  cannot find that computer's files and says ProPresenter is not reachable.
- Who can open it: the same people who can open the dashboard. If the dashboard has a PIN, sign in
  to StagePilot in that browser first (a tab that was not signed in says so and links to
  StagePilot); a page from another website cannot make VerseVision change anything. It is **not**
  available through Remote Access (the remote address answers "Not Found", and the button is not
  drawn there): VerseVision changes the ProPresenter library of the computer it runs on, so it is for
  that computer and the local network.
- There is no "start at login" for VerseVision: it runs whenever StagePilot runs.

## Where its files live

Under StagePilot's settings folder, in `versevision/` (created the first time the page is opened):

| Folder | Holds | macOS / Linux | Windows |
| --- | --- | --- | --- |
| `config/` | `config.toml` (the Config page) | `~/.config/StagePilot/versevision/config/` | `%APPDATA%\StagePilot\versevision\config\` |
| `data/` | the import history (undo) and the scripture cache | `…/versevision/data/` | `…\versevision\data\` |
| `logs/` | the run logs ("explain" and "replay") | `…/versevision/logs/` | `…\versevision\logs\` |
| `keys/` | `keys.env`, the Bible keys you type on the Config page; owner-only (`0700` / `0600`; on Windows your own profile) | `…/versevision/keys/` | `…\versevision\keys\` |

`STAGEPILOT_SETTINGS_PATH` moves `settings.json`, and `versevision/` with it. Uninstalling
StagePilot does not remove them.

## Bible keys

By default VerseVision gets verse text from **StagePilot's shared Bible service**: nothing to
enter, and no key is stored on your computer. It asks the service as this StagePilot installation, which
works once StagePilot has checked in with its control plane (it does by itself; Remote Access does not
need to be on). To use **your own keys** instead (API.Bible, YouVersion, ESV), open VerseVision's
**Config** page → **Bible keys**, type the key (it is never shown again) and choose "Use my own keys"
or leave "Automatic" (your own key for a Bible if you have one, else the shared service). A key typed
there goes to `keys/keys.env` and nowhere else: not to StagePilot's settings, logs or diagnostics.

## For the person who builds StagePilot

- **Which VerseVision a StagePilot release carries:** always the newest VerseVision release. `stagepilot-server` depends on four
  crates of `github.com/huntrw6/versevision` through the branch `stable` (see the comment in
  `crates/stagepilot-server/Cargo.toml`). VerseVision's release workflow moves `stable` to each final release tag (a pre-release tag
  like `v1.2.0-rc1` leaves it alone), and `scripts/create-release.ps1` runs `node scripts/update-versevision.mjs` before it builds,
  which re-resolves both `Cargo.lock` files (the workspace and `desktop/src-tauri`) to that commit. Nobody edits a version or a commit.
  `Cargo.lock` still pins one exact commit, so a build is reproducible and every tag shows which VerseVision it holds.
  `node scripts/update-versevision.mjs --check` says whether the lockfiles are behind (exit 1) without changing anything. A
  VerseVision release reaches users with the next StagePilot release, not in between. Nothing in the VerseVision repository depends on
  StagePilot. A VerseVision release that is not ready to ship should not be tagged; `stable` only ever points at a published release.
- The code is `crates/stagepilot-server/src/versevision_addon.rs`; its tests are
  `crates/stagepilot-server/tests/versevision_addon.rs`. The page's own Content-Security-Policy is
  kept for `/versevision/` (it has inline script); every other header is StagePilot's.
- The button is the `SectionButton` template (`frontend/src/components/sections/`, DESIGN.md §6.6)
  inside `VerseVisionButton`. The desktop app may open `http://127.0.0.1:<port>/versevision/`
  (`desktop/src-tauri/capabilities/default.json`).

## Moving from the standalone VerseVision (Python app, port 8777)

A computer that still runs the old standalone VerseVision (`versevision serve`, page on port
`8777`) can switch to the one inside StagePilot. Nothing is moved by StagePilot itself; do this by
hand, on that computer, once StagePilot is updated:

1. **Stop the old app and its start-at-login.** Run `versevision autostart disable` (or turn it off on
   the old page's Config), or remove it yourself: macOS `~/Library/LaunchAgents/org.versevision.serve.plist`
   (`launchctl bootout gui/$(id -u) <that file>`), Linux the user service `versevision-serve.service`,
   Windows the scheduled task `VerseVision`. Then quit any running `versevision serve`.
2. **Copy what you want to keep** into StagePilot's folders above (create them by opening the page
   once): the old `config.toml` into `config/` (its ProPresenter host and port are ignored; StagePilot's
   Presentation connection is used), `keys.env` into `keys/` (on macOS/Linux then `chmod 600`), and
   `imports.json` into `data/` if you want the old import history to show. The old app kept them in
   `~/Library/Application Support/versevision/` (macOS), `~/.config/versevision/` and
   `~/.local/share/versevision/` (Linux) or `%LOCALAPPDATA%\versevision\` (Windows); its run logs can be
   copied into `logs/` the same way.
3. **Open VerseVision from the Presentation panel**, and update bookmarks from
   `http://localhost:8777/` to `http://localhost:8765/versevision/`.

The two must not both run against the same ProPresenter library at once: keep the old one stopped.
