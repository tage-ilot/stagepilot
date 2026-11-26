<h1>
  <img src="https://avatars.githubusercontent.com/u/314876342?s=200&v=4" width="48" align="absmiddle">
  StagePilot
</h1>

**Your production tech, working as one.**

A live service usually means four apps and several operators keeping them in step by hand. StagePilot connects Planning Center, MultiTracks Playback, ProPresenter and Lightkey, so they share one plan and follow one song. Press play in Playback, and the countdown, the screens and the lights follow. Everyone sees what's playing, how long is left and what's next, on the computer or in any browser on your network.

<img src="docs/img/now-playing.png" width="100%" alt="Now Playing: the current song, time remaining, position in the set and the next song">

### Planning Center

StagePilot loads this week's service plan and keeps it up to date. It pairs each song in the plan with your Playback setlist, so the plan and the band always agree.

<img src="docs/img/services.png" width="100%" alt="Services card: connected, with the last plan sync time">

### Playback

StagePilot follows Playback over its network API or MIDI: which song is playing, how far in it is and what's next. With the Library Link it also reads each song's key, arrangement and waveform.

<img src="docs/img/playback.png" width="100%" alt="Playback Live Link: connected, with the songs of the set">

### ProPresenter

StagePilot runs ProPresenter's song countdown and Look. It adds the setlist to your playlist with each song's Playback arrangement and key, and imports songs ProPresenter doesn't have yet.

<img src="docs/img/presentation.png" width="100%" alt="ProPresenter Live Link: connected, Timer and Look, Presenter Link with 4 songs ready to import">

### Lightkey

Record your operator's lighting once, and StagePilot replays it in time with that song every week. If someone touches a fixture, StagePilot hands it back. Other lighting controllers can get timed MIDI or OSC cues.

<img src="docs/img/lighting.png" width="100%" alt="sACN DMX Live Link: connected, 66 fixtures, songs captured for replay">

## Try it

Download StagePilot for Windows or Mac from the **[latest release](https://github.com/tage-ilot/stagepilot/releases/latest)**, then follow the [getting started guide](docs/getting-started.md).

[Contributing](CONTRIBUTING.md) · [GPL-3.0 license](LICENSE)
