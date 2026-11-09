<h1>
  <img src="https://avatars.githubusercontent.com/u/314876342?s=200&v=4" width="48" align="absmiddle">
  StagePilot
</h1>

StagePilot brings the moving parts of a live production into one dependable dashboard. It loads service plans from Planning Center, follows MultiTracks Playback (over its network API or MIDI cues), keeps a ProPresenter countdown in sync, and sends scheduled MIDI or OSC cues to a lighting controller such as Lightkey.

## Download

Get the installer for your computer from the **[latest StagePilot release](https://github.com/tage-ilot/stagepilot/releases/latest)**: Windows x64 (`.exe`), Intel Mac (`x64.dmg`), or Apple Silicon Mac (`aarch64.dmg`). Only install downloads from the official StagePilot GitHub release page. StagePilot updates itself from there; an **Update** button appears beside the logo when a new release is ready.

On **Windows**, run the installer and open StagePilot from the Start menu.

On **macOS**, open the DMG and drag StagePilot to Applications. The app is not Apple-notarized, so the first time, right-click StagePilot and select **Open** (or use **System Settings → Privacy & Security → Open Anyway**). See [macOS ad-hoc signing](docs/macos-adhoc-signing.md).

## What you need

- Planning Center Services (sign in, or a Personal Access Token)
- MultiTracks Playback on the same network, or a MIDI input
- ProPresenter with its network API enabled
- Optionally, a MIDI or OSC destination for Lightkey or another lighting controller

Planning Center, Playback, ProPresenter, and Lighting are independent. You can set up and test one without the others being online.

## Initial setup

Everything is set up inside StagePilot.

1. Open **Backend** and set the timezone, log level, and server port under Advanced.
2. Open **Services**, choose **Sign in with Planning Center** (or **Manual API Connection** with a Personal Access Token), test the connection, and choose a service type. Treat the token like a password and keep it out of screenshots and issues.
3. Open **Playback**. The default method is **Playback API**: pick your Playback computer (or enter its address) and connect. To use MIDI cues instead, choose **MIDI**; the cue setup is in the [configuration guide](docs/configuration.md).
4. Open **Presentation**, enter ProPresenter's host and API port, select the song timer and optional Look, then test the connection.
5. Open **Lighting** to choose MIDI or OSC, select the output, and add elapsed-time lighting cues.

If Playback's song order differs from the service plan, use **Song arrangement** in the Playback panel to skip songs on either side. Nothing is reordered, and skips are saved with **Save configuration**.

Settings persist between launches, and StagePilot reconnects your integrations on startup. Planning Center credentials are kept in the operating system credential store.

The title-bar **X** hides the dashboard while StagePilot keeps running. Restore it from the system-tray icon (Windows) or Dock/menu-bar icon (macOS), and use **Quit StagePilot** there to stop it completely.

## Browser dashboard

While StagePilot is running, the dashboard is also at [http://127.0.0.1:8765](http://127.0.0.1:8765) (or the port saved in **Backend**). To open it from another computer on a trusted network, set **Connection Access** to **Local network** in **Backend**, then replace the initial PIN (`1234`) under **Access PIN**. Restart StagePilot if it asks, then open `http://<this computer's IP>:8765`. **Internet** turns on Remote access. A PIN is always required from other devices; don't use Local network on public Wi-Fi.

## Weekly operation

StagePilot loads the plan for today, or the next upcoming plan for the selected service type; if several are equally likely it asks you to choose.

Before a service:

1. Confirm the service and time in **Service plan**.
2. Check that the status at the top of the dashboard reads **Ready**, and open any connection card that needs attention.
3. Start a song in Playback (or use **Manual Controls**) and confirm ProPresenter shows the countdown with the right duration.
4. Keep StagePilot open during the service.

Select **Edit layout** to rearrange the dashboard widgets. See [Dashboard layout](docs/dashboard-layout.md).

## Troubleshooting

StagePilot keeps the last loaded plan, so if Planning Center is unavailable the dashboard shows it with a stale warning instead of going empty.

If something fails:

1. Open the affected connection card and run its connection test.
2. Check **Recent Event Stream** for the specific error.
3. In **Backend**, select **Copy Log** (or **Send all logs to developer**). For more detail, first set the log level to `DEBUG`, restart StagePilot, and reproduce the issue once.
4. Attach the log to a [GitHub issue](https://github.com/tage-ilot/stagepilot/issues).

More in the [configuration guide](docs/configuration.md) and [security notes](docs/security.md). Test the full signal path with your real computers, network, and gear before an event.

## Contributing

Development and packaging instructions are in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

StagePilot is licensed under the [GNU General Public License v3.0](LICENSE).
