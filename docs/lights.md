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

The strip has these controls:

- **Address and Connect.** Defaults to the Lighting OSC host on port 8770
  (`10.30.0.211:8770`). The dot shows *Connected*, *Lightkey is closed*, or
  *Not connected* with the reason. If the answer is "refused", add the wired
  interface to `allow_interfaces` in the agent's config.
- **Record from Lightkey.** While on, the next song's countdown is recorded:
  every fixture change (color, intensity, pan and tilt, gobo, zoom and more) is
  kept with its song time, measured from Lightkey's own DMX output with the
  clock difference between the two Macs removed. When the song ends (or Record
  is turned off mid-song, or a new song starts) the changes are thinned and merged into rows, saved
  into the song's lighting cue map with the normal, serialized save, and the raw
  changes are discarded. A song is saved only if at least half of it was
  recorded and at least one fixture changed. **A pause never ends or saves a
  recording**, however late in the song: resuming in place carries on, and only
  the song's end (within 2 s of its length), a new song, or Record off saves it.
  The last change of a fixture after the song's scheduled
  end is folded into the final row at the song's length (a fade-out is kept), an oversize change never discards the capture, and moving the
  playhead during a recording discards it. Messages: `Recording <song>.`,
  `Saved N looks to <song>'s timeline.` (`... (reduced detail, 0.5 s).` when the song was
  too large for the chosen Detail and was saved at a coarser spacing), `Not saved: only X of Y seconds recorded.`
- **Replay | Record toggle.** One two-way control. *Replay* (the default) replays
  the captured lights: saved rows play through the normal timeline scheduler (see
  below). *Record* is for capturing and needs a connected link. Switching back to
  Replay stops the recording first, then turns replay on; if that fails the toggle
  shows the real state and the reason. Replay is paused automatically while Record
  is selected. There is no replay-off switch in the UI; if a settings file or the
  API has `replay` off, the strip says "Replay is off" with a **Turn Replay on**
  button instead of looking as if it were replaying.

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

### Saving a song's cue map

Every save that changes a song's rows raises that song's revision
(`settings.lights.cue_revisions[song_key]`). The cue-map `PUT` may carry the
`revision` it was edited from; when it is older than the saved one the save is refused
with **409** ("This song's lights changed since you opened them"), so an edit made on
a stale list cannot overwrite a recording that finished meanwhile. A request without
`revision` is accepted as before. The manual save and the Lighting settings save take
the same lock as the recording, capture, undo and Simplify saves, so none of them can
lose another's rows. The cue-map route accepts bodies up to 8 MB (a song at the
per-song caps is about 3 MB). A look message may carry at most 8 arguments, and a
text argument at most 256 characters.

Look rows store the placeholder address `/stagepilot/look` and send their `messages`.
An older StagePilot that does not know `messages` would send that placeholder, which
no rig listens to, instead of a fixture address without its value; still, do not
downgrade with look rows saved.

Captured looks live in the one settings file, which is rewritten (pretty-printed) on every save and sent to
the browser with every settings read. Measured with 40 dense songs (148 rows of 40
fixture messages each, 236,800 messages): `settings.json` 60.5 MiB on disk, 22.4 MiB as
the settings payload, and about 2.2 s per save in a debug build (the test is
`forty_dense_songs_settings_file_size_and_save_latency`). That is above what a real
service needs (real takes are 3 to 6 thousand messages a song, so roughly a quarter of
this for 40 songs), and the storage layout is unchanged; moving looks out of the
settings file is a separate design (see the report's open items).

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
and carries on from there. Seeking **back** to before a fixture's first row gives
that fixture back to Lightkey (the song has not touched it yet at that position)
instead of leaving it on a later look.

A single failed send (a dropped network packet, a reconnect) is retried once after
100 ms and then skipped with a log line and the strip's error; the rest of the
song, including every later row, still goes out. The error clears as soon as
sending works again. Only a closed output stops the timeline.

### When the lights are given back

A held fixture returns to Lightkey only when:

- **Record is armed** (replay would otherwise be recorded) or **Replay is
  switched off**;
- Lighting is turned off;
- StagePilot, or the Lighting plugin, shuts down;
- Lighting is reconfigured in a way that matters (Lighting, Replay, the method or
  the host changes);
- **a new song starts**: fixtures the new song's rows do not use are released.
  This happens right after the new song's timeline has started, so its first
  look is never delayed by the releases. Fixtures it does use are simply taken over
  by its first sync, without a clear;
- the operator presses **Release lights**.

Pause, stop, reset, seek, resume and the song's end never clear anything.

Each release sends one `overrides/clear` per fixture, twice, paced at no more than
200 packets a second. Fixtures that were never captured are never touched.

### The operator always wins, per fixture

Lightkey's own controls beat the replay. While the replay holds a fixture, a DMX
change on it that is **not** the echo of something the replay just sent is taken
as the operator's. An echo is the same fixture and property with the same value
(within 2 %, or 1 degree for angles) within about 200 ms, so an operator change on
a property that moves all the time is still caught. Wheel and option properties
(gobo, wheel, slot, prism, macro) and non-numeric values match with any value,
because wheel slots are approximate. Only properties the replay has itself sent to
that fixture are judged: a pan, colour or effect that Lightkey's own running cues
move is never taken for the operator. Fixture names compare without regard to case
or `%20` encoding. StagePilot then stops
sending to that fixture, sends **no clear** (the operator's change stays), and
keeps replaying every other fixture. The strip shows "Holding N fixtures" and
"M yielded to the operator".

After 30 seconds with no further operator change on that fixture the replay takes
it back: while the song runs, it sends the fixture's latest message at or before
the current position and carries on with its later rows; while the song is paused
or stopped it waits and takes the fixture back at the resume or seek re-sync. Every
new operator change on the fixture restarts its 30 seconds. The relay's own
keyframes and hello packets, repeated packets, bursts at a re-sync and a relay
reconnect are never treated as operator input.

**Limit.** Manual control changes (faders, overrides, a pad that moves a
property) show up as DMX changes and are detected. A cue the operator activates on
a fixture the replay is holding is probably invisible in DMX, because the replay's
override sits on top of it. Use **Release lights** for that.

**Release lights** (button in the Lightkey strip, `POST /api/v1/lights/lightkey/release`)
sends the double `overrides/clear` to every fixture held right now **and** every
fixture any saved look can override, then empties the held set. The held set lives
in memory only, so after a crash or restart Lightkey may still hold overrides that
StagePilot has forgotten; naming every known fixture is what clears them. The
button is disabled only when nothing is held and no saved look names a fixture.
Fixtures already yielded to the operator are not cleared. **While a song is
playing the release is momentary**: the running timeline picks a fixture up again at
that fixture's next look row. To keep the lights released, pause or stop first.

### Wired network

The fixture relay uses plain unicast UDP on port 21700, so it crosses the wired
router. Do not use Wi-Fi. Its timing is not steady enough: the agent's multicast
over Wi-Fi stalled for up to 0.8 s in a test. If the stream stops during a
recording, the message says that levels were not recorded.

## Cue board, live capture and the timeline

The timeline follows the song that is playing (or the last one that played) and
switches when a new song starts. A song ending, and the plan's current song moving on
to the next one, do not move it. A row you are still typing (no address yet) is kept
when the settings refresh and when the follow moves to another song. If you pick another song by hand, it stays on your pick
until the next song starts; the line beside the song picker then reads "Showing
<title>" with a **Follow live** button to jump back at once. With no song played
yet it shows the first song of the plan.

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
marker per cue (coloured and shaped by what the cue is, see below) and a playhead while the timer runs.
Drag a marker to move it (it snaps to 0.1 s and stays inside the song), or focus
it and press the left or right arrow to move it 0.1 s (Shift: 1 s). Click or
press Enter to open the small editor to set an exact time or delete the cue.
Markers for the method that is not on screen are not drawn; those cues stay
saved. On narrow widths the bar scrolls sideways, and **Zoom** changes its scale.

### Saving and restarts

Captures, undo and cue-map saves are serialized on the backend, so a capture
and a timer restart cannot overwrite each other's cues. The scheduler is
unchanged: a cue added mid-song takes effect at the next timer start.

## Room loudness behind the timeline

While Lightkey records a song, StagePilot can also measure how loud the room is and draw it as a faint
filled envelope behind the song's look rows in the Lighting timeline (scaled to the song's own loudest
moment, so it shows the shape of the song, not an absolute level). Markers stay on top and the picture
zooms and scrolls with the timeline. A text alternative reads for example "Loudness: peak at 02:10".

**Privacy.** Only loudness numbers are saved: one byte per 0.25 s window, about 1,000 numbers for a four
minute song. Sound is measured the moment it arrives and thrown away. No audio is stored, written to disk
or sent anywhere, and nothing leaves the machine.

**When it listens.** Only while Record is chosen **and** a song is running, on the same song clock as the
lights capture, and with the same rules: a pause leaves a gap, moving the playhead or resetting discards
the take, and it is saved only if at least half the song was recorded. While Record is chosen but no song
runs, the input is open only as long as the input picker is on screen (to show its level meter) and closes
a few seconds after you leave. In Replay mode it never opens.

Opening the input can wait for the microphone permission dialog, so it never blocks
anything else: opening and closing run on their own thread, the song start waits at
most 1.5 s for it (loudness then starts when the input opens), the picker answers from
its last listing after 2 s, and repeated picker polls do not start a second open.

**Choosing the input.** With Record chosen, the Lightkey strip shows an **Audio input** list with a live
level meter. The default is the system default input (the built-in microphone). Pick an audio interface or a
line feed from the board if you have one. The choice is saved by name. If that input is not there next
time (unplugged), StagePilot uses the default input and says so; the saved choice stays so it comes back
when the interface does. **Clear loudness** removes the selected song's envelope and nothing else, and asks first (see below).

**More than one recording.** Recording a song again averages the new loudness into the old. Levels are
averaged as power (not as decibels), each recording counts equally up to eight; after that the oldest
influence fades, so it can still follow how the room changes. A slot one recording missed (a pause) keeps
the other's value.

**If audio is not available** (the Docker or headless server, no input device, microphone permission
refused) the picker is hidden and one short line says why. Lights capture is never affected: the song's
lights record as usual and the status line adds "Loudness is not recorded: ...".

**Limits.**
- The numbers are **dBFS** (decibels relative to the input's full scale, from -60 to 0), not sound
  pressure (SPL). They compare moments within a recording, not rooms or nights. Louder means louder
  *at that input with that gain*.
- The built-in microphone hears the room: the band, the congregation and the speakers together. A line
  feed from the console is cleaner and ignores the room, but it is a different level than a microphone.
  Do not mix inputs across recordings of the same song, or the average blends two different scales.
- Anything stored stays near 0.25 s resolution and at most 4,000 numbers per song (over sixteen minutes).
- Device delay is not compensated; a few tens of milliseconds are far below the 0.25 s window.

### macOS microphone permission

The first time StagePilot listens (when you choose Record, or open the input picker), macOS asks once:
"StagePilot would like to access the Microphone". The text it shows is: *StagePilot measures how loud the
room is while you record lights. It never saves audio.* Choose **OK**. If you chose **Don't Allow**, the
picker shows that microphone access was refused. To allow it later: **System Settings > Privacy &
Security > Microphone**, switch **StagePilot** on, then reopen the app. If StagePilot is not in that list
yet, choose Record once so macOS asks again. Windows has its own switch under **Settings > Privacy >
Microphone** (allow desktop apps). The app is ad-hoc signed without the hardened runtime, so no audio
entitlement is needed today; if it ever ships with the hardened runtime, the
`com.apple.security.device.audio-input` entitlement must be added then. The usage text lives in
`desktop/src-tauri/Info.plist` (`NSMicrophoneUsageDescription`).

## Why MIDI Clock is not used for `mm:ss`

Playback MIDI Clock carries musical tempo pulses while tracks are playing. It
does not provide an absolute song timestamp, so counting its pulses would be
more complex and less reliable for a seconds-based timeline. StagePilot uses a
local monotonic clock for `mm:ss` cues. MIDI Clock can be added later as a
separate optional input for beat- or bar-based lighting effects without changing
the seconds-based scheduler.

## Look rows and the timeline as replay source

A song lighting row (`osc_cues`) may carry `messages` (a list of `{address, args}`) and a `source` of `manual` or `captured`. A row sends its `messages` when non-empty, otherwise its single address/args. Caps: `MAX_OSC_MESSAGES_PER_ROW`, `MAX_OSC_ROWS_PER_SONG`, `MAX_OSC_MESSAGES_PER_SONG`. Fixture look rows fire about 45 ms early and are paced to at most 200 packets/s. On start, resume or seek, the latest `/fixture/<name>/overrides/<prop>` message at or before the position is re-sent once. Pause, stop, end and reset hold the last look; fixtures are released (double `overrides/clear`) only when replay is switched off, Lighting is disabled, on shutdown, or when the next song does not use them.

### Cue types and the legend

Every marker and cue-list row has a type. The type sets the bar colour **and**
style, and a glyph box repeats it, so nothing depends on colour alone. The
accessible name starts with the type: "Captured look, Look - 12 fixtures, at
00:14".

| Type | Bar | Glyph | What it is |
|---|---|---|---|
| Captured look | cyan, solid | ◆ | A look recorded from Lightkey, not changed since |
| Edited look | amber, solid | ◆✎ | A captured look you changed |
| Manual look | indigo, solid | ◇ | A look you built by hand (never replaced by Record) |
| Lightkey cue on | lime, solid | ▶ | `/live/*/cue/<name>/activate` (names may contain spaces) |
| Lightkey cue off | lime, hollow (3 px border) | ■ | `/live/*/cue/<name>/deactivate` |
| Fixture message | magenta, solid | ◐ | One `/fixture/<name>/overrides/...` message |
| Other OSC | grey, dashed | ○ | Any other address |
| MIDI note C D E F G A B | one colour per letter, same in every octave | the letter | A MIDI note-on cue. Sharps keep their letter's colour with a diagonal-striped bar and a ♯ in the glyph box (flats are shown as the sharp of the same pitch). Colour comes from the pitch class in both note display modes |

The pad colour no longer colours the bar: it is a small square after the
label. On the MIDI timeline the legend lists the seven letters (a letter with
no cues is greyed out).

The **legend** above the timeline groups the types (Looks / Lightkey cues /
Single messages) and each chip is a toggle: it hides or shows that type on the
timeline **and** in the cue list. The chip row says "2 types hidden" and has a
**Show all** button. Hiding is only a view setting kept for the browser
session; it is never saved in the cue map and never changes what replays. A
marker you have selected is never hidden. A cluster of mixed types is a neutral
chip with the commonest type's glyph; its accessible name lists the counts.

Colours are the `cue-*` tokens in `DESIGN.md`. Each is at least 3:1 against the
track backgrounds, and a test simulates protanopia, deuteranopia and
tritanopia to keep the colours apart.

### Editing look rows in the timeline

- A row with several messages is a **look**: "Look - N fixtures". Rows from a recording (`captured`) are drawn in the Lighting colour and badged *Captured*; rows made by hand are *Manual*. Press **Edit** on a row to see its messages grouped by fixture. Each message's address and values are editable (the same typed values as a single cue), each can be deleted, and **Add message** adds one (it saves once its address is valid). A row left with no messages is removed. **−0.1 s / +0.1 s**, the time box, dragging the marker and the arrow keys move a row; times snap to 0.1 s.
- **Remove fixture from this song** (in each fixture's heading) asks first, then deletes that fixture's messages from every row of the song. Rows left empty are removed.
- Changing a captured row marks it **edited** (`edited` in the saved cue). With Record chosen, starting a recording on a song that has edited captured looks asks "Recording replaces this song's captured looks, including your edits" first. Deleting a whole captured row counts too: the backend sets `captured_edited` on the song's cue map for it.
- **Fewer items.** Tick two or more rows (or Shift/Ctrl-click markers, drag across the empty track, or long-press a marker on touch) and press **Merge rows**: one row at the earliest time, latest value per address; it asks first if a value would change at an earlier time. **Simplify this song** runs on the backend (`POST /api/v1/lights/lightkey/simplify`): it thins and merges the captured rows exactly as a recording does, at Fine (0.25 s), Normal (0.5 s) or Coarse (1 s), asks for the count first ("412 rows → 190") and applies through the normal cue-map save. Manual rows are never touched. Merge, Simplify and Remove fixture each have one step of Undo until the next edit.
- **Density.** Markers closer than 14 px collapse into one numbered cluster (click, Enter or tap zooms in); the Zoom control goes up to 120 px/s. The cue list shows 40 rows per page; picking a marker jumps to its page.
- The **Detail** setting (Fine / Normal / Coarse) sits in the Lightkey strip and shows only while Record is chosen. The strip reads "N captured looks on this song"; the old list of takes is gone.

## Clearing cues

**Clear** (the selected song), **Clear all** (every song in the plan) and **Clear loudness** all ask first in an in-app
dialog. It says the cues will be **permanently erased** and cannot be undone, names the song (or "every song in
<plan>"), and counts what goes: looks and hand-made cues (loudness for Clear loudness). The focus starts on **Keep
cues**; **Erase permanently** is the only button that erases. Esc, a click outside the dialog and **Keep cues** leave
everything as it was.

## Header layout

The connection-method toggle (Playback API | MIDI, OSC | MIDI) sits in the middle of the panel header, between the
title and the status pill and close button, whenever the panel is at least 57 rem (912 px) wide. That is the width at
which the longest right-hand group ("MIDI · Alternate", Connected, close) still fits beside it. It is in the same place
on the Playback API, Playback MIDI and Lighting screens. Narrower, it moves to its own row under the title.
