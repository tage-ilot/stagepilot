# Getting started

Back to the [README](../README.md). For how StagePilot matches the plan to the band, see [How StagePilot thinks](overview.md).

## What it works with

| | |
|---|---|
| Planning Center Services | the service plan |
| MultiTracks Playback | what's playing, by network API or MIDI |
| ProPresenter | countdown, Looks, playlist import (import needs ProPresenter on a Mac) |
| Lightkey | record and replay over sACN (Lightkey runs on a Mac) |
| Other lighting controllers | timed MIDI or OSC cues |

StagePilot runs on Windows and macOS.

## Install

Get the installer for your computer from the **[latest StagePilot release](https://github.com/tage-ilot/stagepilot/releases/latest)**: Windows x64 (`.exe`), Intel Mac (`x64.dmg`), or Apple Silicon Mac (`aarch64.dmg`). Only install downloads from the official StagePilot GitHub release page. StagePilot updates itself from there; an **Update** button appears beside the logo when a new release is ready.

On **Windows**, run the installer and open StagePilot from the Start menu.

On **macOS**, open the DMG and drag StagePilot to Applications. The app is not Apple-notarized, so the first time, right-click StagePilot and select **Open** (or use **System Settings → Privacy & Security → Open Anyway**). See [macOS ad-hoc signing](macos-adhoc-signing.md).

## Connect your tools

Everything is set up inside StagePilot, one card at a time. Each connection works on its own, so set up only what you use.

1. **Backend:** set the timezone, log level and server port under **Advanced**.
2. **Services:** choose **Sign in with Planning Center** (or **Manual API Connection** with a Personal Access Token), then choose a service type. Treat the token like a password and keep it out of screenshots and issues.
3. **Playback:** the default is the **Playback API**. Enter your Playback computer's address in **Host** (or use **Search**), then **Connect**. To use MIDI cues instead, choose **MIDI**; the cue setup is in the [configuration guide](configuration.md). The optional **Library Link** reads the setlist order, keys, arrangements and waveforms from the Playback computer; its card walks you through the one-time setup.
4. **Presentation:** enter ProPresenter's host and API port (or use **Search**), **Connect**, then pick the song **Timer** and an optional **Look**. **Presenter Link** adds the setlist to a ProPresenter playlist with each song's Playback arrangement and key. See [ProPresenter](propresenter.md).
5. **Lighting:** connect to Lightkey to record and replay your show, or choose MIDI or OSC and add elapsed-time cues. See [Lighting](lights.md).

If Playback's song order differs from the service plan, use **Song order** in the Playback panel to skip songs on either side. Nothing is reordered, and skips are saved with the configuration.

Settings persist between launches, and StagePilot reconnects on startup. Passwords and tokens are kept in the operating system's credential store.

The title-bar **X** hides the dashboard while StagePilot keeps running. Restore it from the system-tray icon (Windows) or the Dock or menu-bar icon (macOS), and use **Quit StagePilot** there to stop it completely.

## Browser dashboard

While StagePilot is running, the dashboard is also at [http://127.0.0.1:8765](http://127.0.0.1:8765) (or the port saved in **Backend**). To open it from another computer on a trusted network, set **Connection Access** to **Local network** in **Backend**, then replace the initial PIN (`1234`) under **Access PIN**. Restart StagePilot if it asks, then open `http://<this computer's IP>:8765`. **Internet** turns on Remote access. A PIN is always required from other devices; don't use Local network on public Wi-Fi.

## Each week

StagePilot loads the plan for today, or the next upcoming plan for the selected service type. If several are equally likely, it asks you to choose.

Before a service:

1. Confirm the service and time in **Service plan**.
2. Check that the status at the top of the dashboard reads **Ready**, and open any card that needs attention.
3. Start a song in Playback (or use **Manual Controls**) and confirm ProPresenter shows the countdown with the right length.
4. Keep StagePilot open during the service.

Select **Edit layout** to rearrange the dashboard widgets. See [Dashboard layout](dashboard-layout.md).

## Troubleshooting

StagePilot keeps the last loaded plan, so if Planning Center is unavailable the dashboard shows it with a stale warning instead of going empty.

If something fails:

1. Open the affected card and select **Connect** again.
2. Check **Recent Event Stream** for the specific error.
3. In **Backend**, select **Copy log** (or **Send all logs to developer**). For more detail, first set the log level to `DEBUG`, restart StagePilot, and reproduce the issue once.
4. Attach the log to a [GitHub issue](https://github.com/tage-ilot/stagepilot/issues).

More in the [configuration guide](configuration.md) and [security notes](security.md). Test the full signal path with your real computers, network and gear before an event.
