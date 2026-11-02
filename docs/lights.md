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

The **Lightkey live link** (in the Lighting panel, OSC only) records what the
fixtures do during a song and puts it on that song's timeline as **look rows**,
which the timeline then plays like any other row. It reads the fixture changes
from [`lightkey-api`](https://github.com/huntrw6/lightkey-api) 0.4 or newer, a
small read-only agent that runs on the Lightkey Mac. Install it there first (its
README covers the one-time macOS permission click). A recording holds fixture
data only: **cues are neither recorded nor replayed**.

The strip has three controls:

- **Address and Connect.** Defaults to the Lighting OSC host on port 8770
  (`10.30.0.211:8770`). The dot shows *Connected*, *Lightkey is closed*, or
  *Not connected* with the reason. If the answer is "refused", add the wired
  interface to `allow_interfaces` in the agent's config.
- **Record from Lightkey.** While on, the next song's countdown is recorded:
  every fixture change (color, intensity, pan and tilt, gobo, zoom and more) is
  kept with its song time, measured from Lightkey's own DMX output with the
  clock difference between the two Macs removed. When the song ends (or Record
  is turned off mid-song) the changes are thinned and merged into rows, saved
  into the song's lighting cue map with the normal, serialized save, and the raw
  changes are discarded. A song is saved only if at least half of it was
  recorded and at least one fixture changed. Changes after the song's scheduled
  end are dropped, an oversize change never discards the capture, and moving the
  playhead during a recording discards it. Messages: `Recording <song>.`,
  `Saved N looks to <song>'s timeline.` (`... (reduced detail, 0.5 s).` when the song was
  too large for the chosen Detail and was saved at a coarser spacing), `Not saved: only X of Y seconds recorded.`
- **Replay captured lights.** On by default, and only paused while recording.
  Saved rows replay through the normal timeline scheduler (see below).

Saving replaces only that song's `source = captured` rows. Rows you added by
hand stay. Captured rows are labelled `Look - N fixtures` and have no colour.
An old `lightkey-takes.json` is **ignored**: it is not read, rewritten or deleted
(delete it by hand once you have re-recorded).

### Detail: thinning and merging

`lights.lightkey.detail` is `fine` (0.25 s), `normal` (0.5 s, default) or
`coarse` (1 s); it can be changed through `PUT /api/v1/lights/lightkey/settings`
(`{"detail_level": "fine"}`; `detail` is accepted as an older spelling). The status
(`GET /api/v1/lights/lightkey`) reports it as `detail_level` and leaves it out while
`normal`. Values are lower case. The same value is the interval and the row grid.

1. **Thin.** Per address a change is kept only if at least one interval has
   passed since the last kept one. The last value of a run is always kept, and so
   are big jumps: a dimmer to or from 0 or full, a colour/level jump past 0.25,
   an angle jump past 25 degrees, and any non-number or option change.
2. **Group on the grid.** Kept changes land on the nearest multiple of the
   interval, so one row holds everything that changed in that slot (an address
   changed twice in a slot keeps its last value). Idle slots make no row. Changes
   at or before time 0 (and the start value of any address that moves later)
   become a row at 0.0. Row times are stored to 0.1 s, so at Fine a row at 0.25 s
   is stored at 0.2 s and one at 0.75 s at 0.8 s.
3. **Merge.** One message per address per row (last wins); a message equal to
   the address's previous kept value is dropped; rows left empty are dropped; a
   row that only repeats addresses of the row before it with values within
   tolerance (colour and dimmer 2 %, angles 1 degree) of the last kept value is
   folded into that earlier row. Comparing with the last kept value means the
   replayed value never drifts further than the tolerance from the recorded one.

A song that would still pass the per-song caps (1,000 rows, 15,000 messages
including the manual rows) is saved at twice the interval, up to six times.
The status message then says which spacing was used (`Saved 88 looks to <song>'s
timeline (reduced detail, 1 s).`); the capture is never dropped for size.
Motion is steppier on replay than the original (0.5 s steps at Normal); Fine is
the remedy.

### Simplify this song (backend)

`POST /api/v1/lights/lightkey/simplify` `{"song_key": "...", "detail_level": "coarse",
"apply": false}` runs the same thin + merge step over the song's captured rows at
the chosen Detail (the saved Detail when omitted) and answers `rows_before`,
`rows_after`, `messages_before`, `messages_after` (the timeline shows
"412 rows -> 190"). Without `apply` nothing is changed. With `apply: true` the new
rows are saved through the serialized cue-map save. Manual rows, MIDI cues and
captured rows that are not fixture overrides are never touched. Nothing is saved
when the result would not be smaller or would still pass the per-song caps.

### Edited captured looks

Deleting a whole captured row leaves no row to carry an "edited" mark, so the
song's cue map has a flag `captured_edited` (left out while false). The cue-map
save sets it when a captured row disappears, it stays until a recording replaces
the captured rows, and it clears when no captured rows are left. Record asks
"Recording replaces this song's captured looks, including your edits" while it
is set.

Size on synthetic data shaped like real takes (25 ms steps, mostly colour, pan
and dimmer sweeps over 255 s):

| Take | Detail | Before merge | After merge |
|---|---|---|---|
| 109k commands, 63 addresses | Fine | 713 rows / 11,693 messages | 705 / 11,102 |
| | Normal | 360 / 6,274 | 357 / 5,904 |
| | Coarse | 183 / 3,566 | 181 / 3,305 |
| 51k commands, 106 addresses | Fine | 228 / 5,518 | 224 / 5,169 |
| | Normal | 116 / 2,976 | 114 / 2,774 |
| | Coarse | 61 / 1,718 | 60 / 1,593 |
| 150k commands, 200 addresses, 300 s | Fine | 361 / 16,150 | 348 / 15,246 (over the 15,000 cap: saved at 0.5 s) |
| | Normal | 181 / 8,723 | 175 / 8,179 |
| | Coarse | 92 / 4,957 | 88 / 4,590 |

### Pause, stop and song end hold the last look

Pausing, stopping, resetting the countdown or reaching the end of the song only
cancels the changes still scheduled. StagePilot sends **no clear**: the fixtures
keep the look they had, and Lightkey's own cues do not take over. Resuming at the
same place, or seeking, sets the fixtures to the song's state at the new position
and carries on from there.

### When the lights are given back

A held fixture returns to Lightkey only when:

- Replay is switched off, or Recording is turned on (replay pauses while
  recording);
- Lighting is turned off;
- StagePilot, or the Lighting plugin, shuts down;
- a new song starts: fixtures the new song's capture does not use are released.
  Fixtures it does use are simply taken over by its first sync, without a clear.

Each release sends one `overrides/clear` per fixture, twice, paced at no more than
200 packets a second. Fixtures that were never captured are never touched.

### Wired network

The fixture relay uses plain unicast UDP on port 21700, so it crosses the wired
router. Do not use Wi-Fi. Its timing is not steady enough: the agent's multicast
over Wi-Fi stalled for up to 0.8 s in a test. If the stream stops during a
recording, the message says that levels were not recorded.

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

## Look rows and the timeline as replay source

A song lighting row (`osc_cues`) may carry `messages` (a list of `{address, args}`) and a `source` of `manual` or `captured`. A row sends its `messages` when non-empty, otherwise its single address/args. Caps: `MAX_OSC_MESSAGES_PER_ROW`, `MAX_OSC_ROWS_PER_SONG`, `MAX_OSC_MESSAGES_PER_SONG`. Fixture look rows fire about 45 ms early and are paced to at most 200 packets/s. On start, resume or seek, the latest `/fixture/<name>/overrides/<prop>` message at or before the position is re-sent once. Pause, stop, end and reset hold the last look; fixtures are released (double `overrides/clear`) only when replay is switched off, Lighting is disabled, on shutdown, or when the next song does not use them.
