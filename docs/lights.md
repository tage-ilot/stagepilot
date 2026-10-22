# Lighting: OSC and MIDI output

StagePilot's **Lighting** connection sends elapsed-time cues to a lighting
application by **OSC** (primary) or **MIDI** (alternate). Lightkey is the
primary target, and any app that listens for OSC over UDP works. Pick the
method with the **Connection method** switch at the top of the Lighting panel.

Only the saved method fires. When OSC is saved, MIDI cues are disabled, and the
other way round. Cues for the inactive method are kept and return when you switch
back. Saving a method change cancels any running timeline; the new method starts
at the next song start. StagePilot never sends both.

A fresh install defaults to OSC. An install that already saved Lighting settings
before OSC existed stays on MIDI, and its MIDI cue maps work unchanged.

## OSC

OSC messages go out over UDP, one message per cue. They are one-shot: there is no
note length or pulse. UDP has no handshake, so StagePilot reports **Sent**, never
Connected, and cannot tell whether the app received a cue. It does surface errors it
can see: a host name that cannot be found, or a socket error while sending.

Settings: **Host or IP address** and **UDP port**. The port is typed in: every
app listens on its own, and StagePilot does not guess one for you. The stored
default is 21600 (Lightkey's port), so a fresh install works with Lightkey on the
same computer, but you can change it to whatever your app expects.

Each OSC cue has an elapsed time, a label, an OSC address (must start with `/`, no
spaces) and optional typed arguments (whole number, decimal, text, true or false).
The **Send test cue** control sends one address and its arguments using the saved
settings.

### Lightkey setup

1. Lightkey receives OSC over UDP on port 21600 (see the table below). If nothing
   arrives, check Lightkey's External Control settings and the Mac's firewall.
2. Control-click a cue and choose **External Control, Copy OSC Address**.
3. In StagePilot, check the port is 21600, add a cue and paste the address.
4. Use **Send test cue** with the same address to check it.

Two-Mac layout: if Lightkey runs on a different Mac than StagePilot, put that Mac's
IP address in **Host or IP address** (the default, `127.0.0.1`,
only reaches the same computer). Both Macs need to be on the same network.

### Finding your app's port

Each app documents its own OSC receive port, and it can usually be changed in the
app, so use the value from the app you are sending to:

- **Lightkey:** 21600 (Lightkey Help, External Control: https://lightkeyapp.com/en/help).
- **Other apps:** look in the app's OSC or network settings for the receive (input)
  port and type it in.

## MIDI

### Two-Mac routing

For the production layout described here:

- StagePilot runs on the iMac.
- Lightkey runs on the MacBook.
- Both Macs are connected to the same reliable wired production network.

On the MacBook, open **Audio MIDI Setup**, show MIDI Studio, open **MIDI Network
Setup**, and create and enable a session. Allow the iMac to connect. On the
iMac, open its MIDI Network Setup and connect to the MacBook session. The
connected session should then appear in StagePilot's **Lighting** output list.

In Lightkey, open **Settings → External Control** and select the network session
under Input. Add a MIDI trigger to a cue, then use **Send test cue** in StagePilot
to let Lightkey learn the selected note. StagePilot intentionally offers MIDI
channels 1–15 because Lightkey recommends avoiding channel 16 for this trigger
workflow.

## Song timelines

Choose a loaded song in **Lighting configuration** and add cue rows. Each row
contains:

- elapsed `mm:ss` from the confirmed song/timer start;
- MIDI note;
- Note On velocity;
- an optional operator label.

StagePilot starts the timeline only after ProPresenter reports that its timer
start sequence succeeded. Stopping or restarting the timer cancels all remaining
cues from the old timeline. A restart creates one fresh timeline at elapsed
`00:00`. Cues beyond the confirmed song duration are not scheduled.

The large dashboard clock counts down the scheduled remaining time. The smaller
clock counts elapsed time up from the same start timestamp.

## Lightkey live link: record and replay

Lightkey can be controlled by OSC and MIDI, but it never sends anything back, so
StagePilot cannot ask it which cues are on. The **Lightkey live link** (in the
Lighting panel, OSC only) reads that from
[`lightkey-api`](https://github.com/huntrw6/lightkey-api), a small read-only
agent that runs on the Lightkey Mac. Install it there first (its README covers
the one-time macOS permission click).

The strip has three controls:

- **Address and Connect.** Defaults to the Lighting OSC host on port 8770
  (`10.30.0.211:8770`). The dot shows *Connected*, *Lightkey is closed*, or
  *Not connected* with the reason. If the answer is "refused", add the wired
  interface to `allow_interfaces` in the agent's config.
- **Record from Lightkey.** While on, the next song's countdown is recorded:
  every cue Lightkey turns on or off is stored with its song time, measured
  from Lightkey's own DMX output with the clock difference between the two Macs
  removed. The capture is attached to that song and saved next to the settings
  (`lightkey-takes.json`). A song is saved only if at least half of it was
  recorded, so a stray start and stop never replaces a good capture. Moving the
  playhead during a recording discards it. Turning Record off mid-song saves
  what was captured, if it was enough.
- **Replay captured lights.** On by default, and only paused while recording.
  When a song with a capture plays, StagePilot first sets every captured cue to
  its recorded state at that point, then sends each change at its time, 40 ms
  early to absorb the OSC and fade delay (measured: 28 to 52 ms). Seeking and
  resuming re-sync from the new position.

Replay only ever touches cues that appear in the capture; every other cue stays
with the Lightkey operator. If the operator changes a cue that replay also
drives, replay leaves that cue alone for 3 seconds. A cue that changes right
after replay sent it is treated as replay's own echo, not an operator action.

Replay uses `/live/*/cue/<name>/activate` and `/deactivate` with no fade
argument, so each cue keeps its own fade. Cue names that contain `/` cannot be
addressed over OSC and are skipped.

### Color, intensity and movers

With `lightkey-api` 0.4 or newer, the live link also captures what the fixtures do. That is color,
intensity, pan and tilt, gobo, zoom and more, not only which cues are on. StagePilot needs no
setup for this. The agent sends its decoded fixture changes to the same address as the API, on UDP
port 21700. The strip shows a second line, **Color and levels**, with the number of fixtures.

- **Record.** The same Record switch captures both. Each fixture change is stored with its song
  time in Lightkey's OSC form, for example `/fixture/MH1/overrides/panAngle 90.0`. Replay needs no
  conversion. A fixture that never changes during the song is not stored. It stays with the cues
  and the Lightkey operator. The song's line in the list counts the color and level changes next
  to the cue changes.
- **Replay.** Replay sets each captured fixture to its recorded state at that point. Then it sends
  each change 45 ms before its time. Replay holds those fixtures with overrides. When the song
  ends, stops, or replay is switched off, StagePilot clears every fixture it held. It sends one
  clear per fixture, twice. The cues then take over again. Fixtures that were never captured are
  not touched.
- **Older agents.** An agent without the fixture relay shows no second line. Recording then
  captures cues only, as before. If the stream stops during a recording, the cue capture is
  unaffected, and the message says that levels were not recorded.
- **Wired network.** The relay uses plain unicast UDP, so it crosses the wired router. Do not use
  Wi-Fi. Its timing is not steady enough: the agent's multicast over Wi-Fi stalled for up to
  0.8 s in a test.

## Cue board, live capture and the timeline

Cues are easiest to set while the song is running in practice. Under **Song
lighting timeline** the panel has a **Cue board** and a visual **Timeline**; the
cue rows below stay the precise editor and always match the timeline.

### Cue board

A pad is a saved, reusable cue: a name, an optional colour and the payload for
one method (an OSC address with typed arguments, or a MIDI note and velocity).
Choose **Edit pads** to add, edit, rename, reorder and delete pads. Pads are
global (shared by every song) and saved in settings, so they survive restarts.

A pad belongs to the method it was made under. When the other method is on
screen, the pad stays on the board, dimmed and disabled, and is not deleted.
Only the saved method fires, as everywhere else in Lighting.

### Live capture

While the selected song's timer is running, pressing a pad (click, tap, or keys
1-9 while the board has focus) does two things:

1. fires the cue at once through the saved method, and
2. adds a cue to the song's timeline at the current elapsed time, rounded to
   0.1 s. The cue takes the pad's name, colour and payload. A short note such as
   "Added at 01:12" confirms it.

**Preview only** turns the second step off, so pads can be tested without
writing to the song. **Undo last capture** removes the most recent capture. With
no song running, a press only fires and the board says nothing was added.

Cue times are kept to 0.1 s. Existing whole-second cue maps load unchanged.

### Timeline

A horizontal bar covers the song's scheduled length, with a time ruler, one
marker per cue (coloured from its pad) and a playhead while the timer runs.
Drag a marker to move it (it snaps to 0.1 s and stays inside the song), or focus
it and press the left or right arrow to move it 0.1 s (Shift: 1 s). Click or
press Enter to open the small editor to set an exact time or delete the cue.
Markers for the method that is not on screen are not drawn; those cues stay
saved. On narrow widths the bar scrolls sideways, and **Zoom** changes its scale.

### Saving and restarts

Captures, undo and cue-map saves are serialized on the backend, so a capture
and a timer restart cannot overwrite each other's cues. The scheduler is
unchanged: a cue added mid-song takes effect at the next timer start.

## Why MIDI Clock is not used for `mm:ss`

Playback MIDI Clock carries musical tempo pulses while tracks are playing. It
does not provide an absolute song timestamp, so counting its pulses would be
more complex and less reliable for a seconds-based timeline. StagePilot uses a
local monotonic clock for `mm:ss` cues. MIDI Clock can be added later as a
separate optional input for beat- or bar-based lighting effects without changing
the seconds-based scheduler.
