<h1>
  <img src="https://avatars.githubusercontent.com/u/314876342?s=200&v=4" width="48" align="absmiddle">
  StagePilot
</h1>


StagePilot brings the moving parts of a live production into one dependable dashboard. It loads service plans from Planning Center, follows MultiTracks Playback (over its network API or MIDI cues), keeps a ProPresenter countdown in sync, and sends scheduled MIDI or OSC cues to a lighting controller such as Lightkey.

Production desktop builds check the signed
[GitHub updater feed](docs/updating-stagepilot.md) after startup. The header
stays unchanged when StagePilot is current; an **Update** button appears beside
the logo only when a newer signed release is available. There is one release channel: every install follows the releases on this page.

## Download StagePilot 2.42.0-rc.4

Download the installer for your computer from the [StagePilot v2.42.0-rc.1 release](https://github.com/tage-ilot/stagepilot/releases/tag/v2.42.0-rc.4-rc.3-rc.2-rc.1):

- [Windows x64 installer](https://github.com/tage-ilot/stagepilot/releases/download/v2.42.0-rc.4-rc.3-rc.2-rc.1/StagePilot_2.42.0-rc.1_x64-setup.exe)
- [Intel Mac DMG](https://github.com/tage-ilot/stagepilot/releases/download/v2.42.0-rc.4-rc.3-rc.2-rc.1/StagePilot_2.42.0-rc.1_x64.dmg)
- [Apple Silicon Mac DMG](https://github.com/tage-ilot/stagepilot/releases/download/v2.42.0-rc.4-rc.3-rc.2-rc.1/StagePilot_2.42.0-rc.1_aarch64.dmg)

On **Windows**, run the installer and open StagePilot from the Start menu.

On **macOS**, open the DMG and drag StagePilot to Applications. StagePilot is
ad-hoc signed rather than Apple Developer ID signed or notarized, so macOS may
ask you to confirm that you trust the application. If it cannot be opened,
right-click StagePilot and select **Open**, or open **System Settings → Privacy
& Security** and select **Open Anyway** for StagePilot. Do not disable
Gatekeeper globally.

StagePilot's macOS package is ad-hoc signed and does not require a paid Apple
Developer account. The release pipeline verifies that the final Intel and Apple
Silicon application, DMG, and automatic-update archive can each start the
packaged backend. Technical details are in
[macOS ad-hoc signing](docs/macos-adhoc-signing.md).

Only install downloads from the **official StagePilot GitHub release page**.

## What you need

StagePilot works with:

- Planning Center Services and a Personal Access Token
- MultiTracks Playback on the same network (or a MIDI input from Playback or another MIDI source)
- ProPresenter with its local network API enabled
- Optionally, a MIDI or OSC destination for Lightkey or another lighting controller

Planning Center, Playback, ProPresenter, and Lighting are independent. You can configure and test one connection without requiring every other integration to be online.

## Initial setup

All normal setup happens inside StagePilot; command-line variables are not required.

1. Open **Backend** and set the timezone, log level, and local server port under Advanced. Choose how StagePilot is reached under **Connection Access** (see below).
2. Open **Services**, choose **Sign in with Planning Center** (or use **Manual API Connection** with a Personal Access Token), test the connection, and choose a service type from the discovered list.
3. Open **MIDI / Playback**. The default connection method is **Playback API**: scan the network for Playback and connect, and StagePilot stays connected automatically. To use MIDI cues instead, choose **MIDI** and set the input, channel, cue note, action velocities, and debounce time.
4. Open **Presentation**, enter ProPresenter's host and API port, select the song timer and optional Look (the default is **Current Look**), then test the connection.
5. Open **Lighting** to choose MIDI or OSC, select the output, and add elapsed-time lighting cues when lighting automation is needed.

If Playback's song order differs from the service plan, use **Song arrangement** in the Playback panel: skip a song on either side and the remaining songs pair up in order. Nothing is ever reordered, and skips are saved with **Save configuration**.

Settings persist between launches. On future launches, StagePilot automatically refreshes the saved MIDI input and reconnects configured Planning Center, ProPresenter, and Lighting integrations after startup finishes. Planning Center credentials are stored separately in the operating system credential store and are never returned by the StagePilot API.

The title-bar **X** hides the dashboard while StagePilot and its integrations continue running. Restore it from the StagePilot system-tray icon on Windows or the StagePilot Dock/menu-bar icon on macOS. Use **Quit StagePilot** from that menu when you want to stop the application and its managed backend completely.

## Browser dashboard

While the StagePilot desktop app is running, the same dashboard is available in a browser at [http://127.0.0.1:8765](http://127.0.0.1:8765), using the server port saved in **Backend**.

To open it from another computer on the same trusted network:

1. Open **Backend** and set **Connection Access** to **Local network**. The choice saves immediately.
2. Replace the initial dashboard PIN (`1234`) with a private PIN under **Access PIN** and select **Save PIN**.
3. If StagePilot says a restart is needed, restart it so it listens on the network.
4. Find the StagePilot computer's local IP address, such as `192.168.1.40`.
5. On the other computer, open `http://192.168.1.40:8765`, replacing the address and port as needed.
6. Enter the dashboard PIN. The normal StagePilot loading screen appears only after access is approved.

**Host** keeps the dashboard on this computer only. **Internet** turns on Remote access, which gives the dashboard a private web address you can use away from the building (local network access stays available too). A dashboard PIN is always required for access from another device; use LAN access only on a trusted, private production network, not public Wi-Fi. Allow StagePilot through the operating-system firewall if prompted.

## Weekly operation

When StagePilot opens, it looks for a plan on the current local date. If none exists, it loads the next upcoming plan for the selected service type. Equally plausible plans require a manual selection instead of being chosen silently.

Before a service:

1. Confirm the correct service and service time in **Service Plan**.
2. Review the system check at the top of the dashboard (it reads **Ready** when everything is connected) and open any connection card that needs attention.
3. Start a song in Playback or use **Manual Controls** to test the timer path.
4. Confirm ProPresenter shows the selected timer as a Countdown Timer with the correct duration.
5. Keep StagePilot open during the service to monitor timing, connections, lighting cues, and events.

Service headers and reference items remain visible in Planning Center order, while song rows drive the timer workflow. Select **Edit layout** to drag or resize the Service plan, Now Playing, Manual Controls, and Recent Event Stream widgets. StagePilot compacts accidental gaps, supports explicit spacers, provides keyboard ordering controls, and keeps separate desktop, tablet, and mobile layouts between launches. See [Dashboard layout](docs/dashboard-layout.md).

## MIDI controls

This section applies when the connection method is **MIDI**. (With the default **Playback API** method, StagePilot follows Playback directly and needs no cues.)

The default Playback cue uses MIDI channel 1 and note 112 (`E7`). For each song,
create a note-on cue at the exact beginning and set its velocity to the song's
one-based position in the loaded service plan:

| Velocity | Song target |
| ---: | --- |
| 1 | First song |
| 2 | Second song |
| 3–99 | Corresponding service-plan song |

StagePilot starts the exact matching song, so a missed or extra Playback cue
cannot silently advance the dashboard to a different song. The MIDI
Configuration panel displays the current plan's complete song-to-velocity map.

Velocities 100–127 are reserved for the existing action cues:

| Velocity | Action |
| ---: | --- |
| 100 | Start next |
| 101 | Restart current |
| 102 | Previous |
| 103 | Next |
| 104 | Reload plan |
| 105 | Stop timer |

The note, channel, and action velocities can be changed in MIDI Configuration.
Manual Controls are unchanged. **Reset Position** remains available there and
returns both StagePilot and the configured ProPresenter timer to `0:00`.

## Planning Center access

The simplest way to connect is **Sign in with Planning Center** in the Services panel: approve StagePilot in the browser window that opens, then test the connection, load the available service types, choose the one StagePilot should follow, and save.

If you prefer a Personal Access Token, use **Manual API Connection**. Sign in to Planning Center's [Personal Access Token page](https://api.planningcenteronline.com/personal_access_tokens), create a token with a recognizable name such as `StagePilot`, copy its **Client ID** into the **Application ID** field and its **Secret** into the **Secret** field, then test and save.

Treat the Client ID and Secret like a password. Do not include them in screenshots, logs, support requests, or GitHub issues. StagePilot stores credentials in the operating system credential store rather than its normal settings file.

## Reliability and troubleshooting

StagePilot caches the last successfully loaded service plan. If Planning Center is temporarily unavailable, the cached plan remains visible with a stale warning rather than leaving the production dashboard empty.

If something fails:

1. Open the affected connection card and run its connection test.
2. Review **Recent Event Stream** for the specific operation that failed.
3. Set the backend log level to `DEBUG`, restart StagePilot, and reproduce the issue once.
4. Attach `stagepilot-backend.log` to a [GitHub issue](https://github.com/tage-ilot/stagepilot/issues).

On macOS, the native **Help** menu searches the StagePilot documentation that
was bundled with the installed application. Enter a connection, setup, MIDI,
timer, lighting, or troubleshooting term in the Help search field to find the
matching local topics; an internet connection is not required.

Packaged macOS logs are stored at `~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`. More configuration and security information is available in the [configuration guide](docs/configuration.md) and [security notes](docs/security.md).

Test the complete signal path with the same computers, network, MIDI routes, Playback session, ProPresenter timer, and lighting setup that will be used during the event.

## Contributing

StagePilot is open source. Development, testing, and packaging instructions are in [CONTRIBUTING.md](CONTRIBUTING.md).

Maintainers can validate, version, push, and publish all supported installers
with one PowerShell command. See [Creating a StagePilot release](docs/creating-a-release.md).

## License

StagePilot is licensed under the [GNU General Public License v3.0](LICENSE).
