# ProPresenter integration

StagePilot controls one reusable ProPresenter timer through ProPresenter's local HTTP API. When a song cue arrives, StagePilot stops the timer, configures it as a Countdown Timer with the song duration, resets it, verifies the saved duration, and then starts it.

## Required ProPresenter setup

1. Open ProPresenter **Settings → Network** and enable the API.
2. Note the configured API port. StagePilot defaults to `1025` but does not assume that value.
3. Create a countdown timer named `Song Countdown`, or select another detected countdown timer in StagePilot.
4. Link the timer to the desired ProPresenter stage layout.

## StagePilot configuration

The production setup panel persistently stores:

- Host
- Port
- Timer name
- Request timeout
- Connection test
- Timer rediscovery

Save the connection settings in the panel. Saving automatically enables real
ProPresenter output and connects right away, with no restart; no simulated
output choice is shown in the production panel. Switching between simulated and
real output is refused while the timer is running. Environment overrides remain available for
development or unattended startup:

```dotenv
STAGEPILOT_TIMER_OUTPUT=propresenter
STAGEPILOT_PROPRESENTER_HOST=127.0.0.1
STAGEPILOT_PROPRESENTER_PORT=1025
STAGEPILOT_PROPRESENTER_TIMER_NAME=Song Countdown
STAGEPILOT_PROPRESENTER_TIMEOUT_SECONDS=3
STAGEPILOT_PROPRESENTER_RECONNECT_INITIAL_SECONDS=1
STAGEPILOT_PROPRESENTER_RECONNECT_MAX_SECONDS=30
STAGEPILOT_PROPRESENTER_HEALTH_CHECK_SECONDS=10
```

## Connection status

ProPresenter is checked every few seconds (`STAGEPILOT_PROPRESENTER_HEALTH_CHECK_SECONDS`
while a timer is running, the reconnect interval otherwise). A connection event is published
only when the status or its detail changes; an unchanged check just refreshes "last checked".
In the event stream (`recent_events`, also in `GET /api/v1/state`) the newest `connection.changed`
entry per source replaces the older one, separately for connected and not-connected
statuses, so the last time ProPresenter was properly connected stays visible after a
disconnect.

## ProPresenter Link

The ProPresenter Link is how StagePilot reaches the **files and the screen of the ProPresenter computer**: it reads the
song files (to find which MultiTracks song each one is), adds new files to a library named `Playback`, and, on a Mac, drives
ProPresenter's own Search window to import a song. This version only builds the connection, its status, the helper and the
setup; nothing in ProPresenter is changed by it yet. It is optional: with no ProPresenter Link StagePilot works as before.

**Which connection (no switch, the saved ProPresenter computer decides).** The link is used while the ProPresenter output is
in use and a ProPresenter host is saved (`propresenter.host`). If that host is this computer (`127.0.0.1`, `localhost`, `::1`,
or one of this computer's own addresses) the link is **Local**: StagePilot reads and writes the ProPresenter folder itself, and
there is nothing to install, no key and no setup. Any other address is **Helper**: a small signed program on the
ProPresenter computer (the ProPresenter Link, port 8790) does it. While no ProPresenter host is saved nothing is asked, ever.

**Folder.** `~/Documents/ProPresenter` by default; `propresenter.link.data_folder` (a full path, optional, nothing secret) points
elsewhere. Every path is relative to it: `..`, absolute paths and links leading out of the folder are refused. Only `*.pro`
files are listed, and a file may be up to 40 MB. Files are only ever **created** (never replaced, renamed or deleted), only
in the library `Playback` (a library folder of another name can be created, never `Songs`), through a temporary file, fsync,
rename and a SHA-256 check. A song's MultiTracks number is read from its file by a small streaming reader (no whole-file load,
cached by path, size and time). The screen steps (check access, import a song through Search, select a library row) are
scripts bundled with StagePilot (`crates/stagepilot-propresenter/assets/gui`); values are passed to them as arguments, never
built into script text. Import needs a Mac with Accessibility permission for the program that runs the scripts, and says in
plain words when it cannot.

**Status.** `GET /api/v1/propresenter-link/status[?check=1]` answers
`{mode: "local"|"helper"|null, state, message, version?, bundled_version, can_import, import_blocker?, libraries, playback_library_exists, data_folder?, computer_address?, paired, updater, stale, update?}`.
`state` is `ready`, `not_installed`, `outdated` (works, an update is available), `needs_command` (the helper does not know this
StagePilot's key: run the setup command again), `not_answering` or `checking`. It is read when a ProPresenter host is saved and
at most every 30 seconds while a window asks (the Check button waits 2 seconds, 10 after a refusal); a helper that goes quiet
keeps its last good answer for a minute, marked `stale`. The state never changes the header's Ready / Check system verdict.

**Setup (Helper only).** `POST /api/v1/propresenter-link/helper-install` opens a 30-minute session and answers a command
to paste into Terminal on the ProPresenter computer; `DELETE` closes it. The command fetches `/propresenter-helper/<token>/install.sh`
and `/propresenter-helper/<token>/files/<name>` (a 128-bit token, private network sources only, never through the remote tunnel);
the installer checks every file against `SHA256SUMS`, installs a LaunchAgent (`io.stagepilot.propresenter-link`, in
`~/Library/Application Support/ProPresenterLink`), reads the key from a private temporary file (never a command line), and
answers only the StagePilot computer's address it was given. The session ends when the helper answers a signed request. StagePilot
makes the key once, keeps it in the credential store under its own name, and never shows or logs it. When the saved host is this
computer the route answers `409` with `code: "not_needed"` and one plain sentence.

**Helper requests.** Every request except the bare `GET /health` carries `X-PPL-Time` and `X-PPL-Sig`
(hex HMAC-SHA256 of `METHOD\n<path+query>\n<time>`, and for `PUT`/`POST` also `\n<SHA-256 of the body>`), within 120 seconds;
ten bad signatures from one address lock it for a minute (`429`). Endpoints: `GET /health`, `GET /index`,
`GET /libraries/<name>/files`, `GET /libraries/<name>/files/<file>`, `PUT /libraries/Playback/files/<file>` (create-only,
`X-Sha256`), `POST /libraries/<name>`, `POST /import {id, name, playlist}`, `POST /select-library {name}`, `POST /update`.
The helper updates itself like the Library Link's and the Lightkey helper's: StagePilot sends the signed bundle once nothing
plays and no timer runs (`helper_auto_update` turns it off). This helper is a copy of the Library Link's design with its own
code and key; the two helpers are independent. Without a Mac or the Accessibility permission, the files still work and
`can_import` is `false`.

To try it without ProPresenter: `python3 tools/fake_propresenter_link.py --port 18790 --key-file key.txt --allow-client 127.0.0.1 --allow-loopback`
runs the real helper on a temporary folder with the screen replaced; `tools/fake_propresenter_api.py` stands in for ProPresenter's
own HTTP API (libraries and playlists) for the later arrangement work.

## Timer sequence

For `song.started` and `song.restarted`, StagePilot performs:

1. Stop
2. Set countdown duration
3. Reset
4. Start

`timer.stop_requested` stops the real ProPresenter timer while retaining its
configured duration. **Reset Position** performs a full reset sequence:

1. Stop
2. Set countdown duration to `0:00`
3. Reset

The dashboard records the start immediately before the ProPresenter start
request and advances the displayed remaining time on the first fractional
second. This keeps StagePilot's main countdown aligned with ProPresenter rather
than displaying the previous whole second for nearly one extra second.

## Recovery

StagePilot periodically probes ProPresenter. If ProPresenter is offline at startup or restarts later, the plugin retries with capped exponential backoff. Timer identities are rediscovered by visible name, and failed timer operations clear the cached timer before one immediate retry.

## macOS diagnostic log

Packaged macOS builds save backend diagnostics here:

`~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`

In Finder, choose **Go → Go to Folder**, paste that path, and press Return. Fully quit and reopen StagePilot before reproducing a problem so the latest session appears at the end of the file. Review the file before sharing it; it should not contain saved secrets, but service and timer names may appear.

The dashboard distinguishes:

- API unreachable
- API connected but configured timer missing
- Timer found and ready
- Timer command failure
