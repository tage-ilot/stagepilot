<h1>
  <img src="https://avatars.githubusercontent.com/u/314876342?s=200&v=4" width="48" align="absmiddle">
  StagePilot
</h1>

**Your production tech, working as one.**

If your services run on Planning Center, MultiTracks Playback, ProPresenter and Lightkey, you know the routine: four apps, several operators, and a lot of clicking to keep them in step. StagePilot connects them. Press play in Playback, and the countdown, the screens and the lights follow the song.

<p align="center"><img src="docs/img/now-playing.png" width="720" alt="Now Playing: the current song, time remaining, position in the set and the next song"></p>

## What it does

- **Knows the plan.** Loads this week's service from Planning Center and pairs it with your Playback setlist.
- **Follows the band.** Knows which song is playing, how long is left and what comes next, from Playback's network API or MIDI.
- **Runs the screens.** Keeps ProPresenter's song countdown and Look in step. Adds the setlist to your playlist with the arrangement and key Playback will play, and imports songs ProPresenter doesn't have yet.
- **Runs the lights.** Record your operator's Lightkey run once, and StagePilot replays it with that song every week after. If someone touches a fixture, StagePilot hands that fixture back. Or fire timed MIDI or OSC cues to any lighting controller.
- **One dashboard.** See it all on the computer, or from any browser on your network.

## A look inside

Each connection has its own card with a plain status, so you can see what's ready at a glance. Connect only what you use; each one works on its own.

<p align="center"><img src="docs/img/services.png" width="340" alt="Services card: connected, with the last plan sync time"></p>

**Playback:** the setlist with keys, lengths and each song's sections.

<img src="docs/img/playback.png" alt="Playback Live Link: connected, with the four songs of the set">

**Presentation:** ProPresenter's timer and Look, and the songs ready to import.

<img src="docs/img/presentation.png" alt="ProPresenter Live Link: connected, Timer and Look, Presenter Link with 4 songs ready to import">

**Lighting:** your Lightkey show, captured song by song and replayed in time.

<img src="docs/img/lighting.png" alt="sACN DMX Live Link: connected, 66 fixtures, songs captured for replay">

## Get started

1. Download StagePilot for Windows or Mac from the **[latest release](https://github.com/tage-ilot/stagepilot/releases/latest)**. It updates itself from there.
2. On a Mac, the first time, right-click StagePilot and choose **Open** ([why](docs/macos-adhoc-signing.md)).
3. Open each card (Services, Playback, Presentation, Lighting) and connect it.

The [getting started guide](docs/getting-started.md) walks through each step. [How StagePilot thinks](docs/overview.md) explains how it matches the plan to the band.

## Works with

| | |
|---|---|
| Planning Center Services | the service plan |
| MultiTracks Playback | what's playing, by network API or MIDI |
| ProPresenter | countdown, Looks, playlist import (import needs ProPresenter on a Mac) |
| Lightkey | record and replay over sACN (Lightkey runs on a Mac) |
| Other lighting controllers | timed MIDI or OSC cues |

StagePilot runs on Windows and macOS. Test the whole setup with your real computers and gear before a service.

## Help and contributing

Report problems on [GitHub issues](https://github.com/tage-ilot/stagepilot/issues), with the log from **Backend → Copy log** attached. Development notes are in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[GNU General Public License v3.0](LICENSE)
