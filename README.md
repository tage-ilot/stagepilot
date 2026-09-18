<h1>
  <img src="https://avatars.githubusercontent.com/u/314876342?s=200&v=4" width="48" align="absmiddle">
  StagePilot
</h1>


StagePilot brings the moving parts of a live production into one dependable dashboard. It loads service plans from Planning Center, listens for MIDI cues from MultiTracks Playback, keeps a ProPresenter countdown in sync, and sends scheduled MIDI cues to a lighting controller such as Lightkey.

Production desktop builds check the signed
[GitHub updater feed](docs/updating-stagepilot.md) after startup. The header
stays unchanged when StagePilot is current; an **Update** button appears beside
the logo only when a newer signed release is available.

## Download StagePilot 1.1.103

Download the installer for your computer from the [StagePilot v1.1.103 release](https://github.com/huntrw6/stagepilot/releases/tag/v1.1.103):

- [Windows x64 installer](https://github.com/huntrw6/stagepilot/releases/download/v1.1.103/StagePilot_1.1.103_x64-setup.exe)
- [Intel Mac DMG](https://github.com/huntrw6/stagepilot/releases/download/v1.1.103/StagePilot_1.1.103_x64.dmg)
- [Apple Silicon Mac DMG](https://github.com/huntrw6/stagepilot/releases/download/v1.1.103/StagePilot_1.1.103_aarch64.dmg)

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
- A MIDI input from MultiTracks Playback or another MIDI source
- ProPresenter with its local network API enabled
- Optionally, a MIDI destination for Lightkey or another lighting controller

Planning Center, MIDI, and ProPresenter are independent. You can configure and test one connection without requiring every other integration to be online.

## Initial setup

All normal setup happens inside StagePilot; command-line variables are not required.

1. Click **StagePilot backend** and set the timezone, log level, and local server port.
2. Click **Planning Center**, enter the Personal Access Token Application ID and Secret, test the connection, and choose a service type from the discovered list.
3. Click **MIDI / Playback**, select the MIDI input, channel, cue note, action velocities, and debounce time.
4. Click **ProPresenter**, enter its host and API port, select the song timer and optional Look, then test the connection.
5. Click **Lights** to select an output and add elapsed-time lighting cues when lighting automation is needed.

Settings persist between launches. On future launches, StagePilot automatically refreshes the saved MIDI input and reconnects configured Planning Center, ProPresenter, and Lights integrations after startup finishes. Planning Center credentials are stored separately in the operating system credential store and are never returned by the StagePilot API.

The title-bar **X** hides the dashboard while StagePilot and its integrations continue running. Restore it from the StagePilot system-tray icon on Windows or the StagePilot Dock/menu-bar icon on macOS. Use **Quit StagePilot** from that menu when you want to stop the application and its managed backend completely.

## Browser dashboard

While the StagePilot desktop app is running, the same dashboard is available in a browser at [http://127.0.0.1:8765](http://127.0.0.1:8765), using the server port saved in **StagePilot backend**.

To open it from another computer on the same trusted network:

1. Open **StagePilot backend** and enable **Allow dashboard access from this local network**.
2. Replace the initial dashboard PIN (`1234`) with a private PIN, then save the settings.
3. Fully restart StagePilot.
4. Find the StagePilot computer's local IP address, such as `192.168.1.40`.
5. On the other computer, open `http://192.168.1.40:8765`, replacing the address and port as needed.
6. Enter the dashboard PIN. The normal StagePilot loading screen appears only after access is approved.

PIN protection is enabled by default and can be disabled in **StagePilot backend**, but LAN access should still be used only on a trusted, private production network—not public Wi-Fi. Allow StagePilot through the operating-system firewall if prompted.

## Weekly operation

When StagePilot opens, it looks for a plan on the current local date. If none exists, it loads the next upcoming plan for the selected service type. Equally plausible plans require a manual selection instead of being chosen silently.

Before a service:

1. Confirm the correct service and service time in **Service Plan**.
2. Review **Readiness Check** and open any connection card that needs attention.
3. Send a Playback cue or use **Manual Controls** to test the timer path.
4. Confirm ProPresenter shows the selected timer as a Countdown Timer with the correct duration.
5. Keep StagePilot open during the service to monitor timing, connections, lighting cues, and events.

Service headers and reference items remain visible in Planning Center order, while song rows drive the timer workflow. Select **Edit layout** to drag or resize the Service Plan, Now Playing, Manual Controls, Readiness Check, and Recent Event Stream widgets. StagePilot compacts accidental gaps, supports explicit spacers, provides keyboard ordering controls, and keeps separate desktop, tablet, and mobile layouts between launches. See [Dashboard layout](docs/dashboard-layout.md).

## MIDI controls

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

To use the Service Plan features, sign in to Planning Center's [Personal Access Token page](https://api.planningcenteronline.com/personal_access_tokens) and create a Personal Access Token for StagePilot.

1. Create a new token with a recognizable name such as `StagePilot`.
2. Copy the token's **Client ID** into StagePilot's **Application ID** field.
3. Copy the token's **Secret** into StagePilot's **Secret** field.
4. Select **Test connection**, load the available service types, choose the service StagePilot should follow, and save the settings.

Treat the Client ID and Secret like a password. Do not include them in screenshots, logs, support requests, or GitHub issues. StagePilot stores the secret in the operating system credential store rather than its normal settings file.

## Reliability and troubleshooting

StagePilot caches the last successfully loaded service plan. If Planning Center is temporarily unavailable, the cached plan remains visible with a stale warning rather than leaving the production dashboard empty.

If something fails:

1. Open the affected connection card and run its connection test.
2. Review **Recent Event Stream** for the specific operation that failed.
3. Set the backend log level to `DEBUG`, restart StagePilot, and reproduce the issue once.
4. Attach `stagepilot-backend.log` to a [GitHub issue](https://github.com/huntrw6/stagepilot/issues).

On macOS, the native **Help** menu searches the StagePilot documentation that
was bundled with the installed application. Enter a connection, setup, MIDI,
timer, lighting, or troubleshooting term in the Help search field to find the
matching local topics; an internet connection is not required.

Packaged macOS logs are stored at `~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`. More configuration and security information is available in the [configuration guide](docs/configuration.md) and [security notes](docs/security.md).

Test the complete signal path with the same computers, network, MIDI routes, Playback session, ProPresenter timer, and lighting setup that will be used during the event.

## Contributing

StagePilot is open source. Development, testing, and packaging instructions are in [CONTRIBUTING.md](CONTRIBUTING.md), with future direction in [ROADMAP.md](ROADMAP.md).

Maintainers can validate, version, push, and publish all supported installers
with one PowerShell command. See [Creating a StagePilot release](docs/creating-a-release.md).

## License

StagePilot is licensed under the [GNU General Public License v3.0](LICENSE).
