# Lighting: OSC and MIDI output

StagePilot's **Lighting** connection sends elapsed-time cues to a lighting
application by **OSC** (primary) or **MIDI** (alternate). Lightkey is the
primary target, and the OSC presets cover the other common lighting apps. Pick the
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

Settings: **Preset**, **Host or IP address** and **UDP port**. A preset fills the
default port and shows an example address pattern as helper text; you can always
override both. Where an app's own docs name no default port, the preset leaves the
port empty rather than guess.

Each OSC cue has an elapsed time, a label, an OSC address (must start with `/`, no
spaces) and optional typed arguments (whole number, decimal, text, true or false).
Some presets offer an action menu that fills the address and arguments. The
**Send test cue** control sends one address and its arguments using the saved
settings.

### Lightkey setup

1. Lightkey receives OSC over UDP on port 21600 (see the table below). If nothing
   arrives, check Lightkey's External Control settings and the Mac's firewall.
2. Control-click a cue and choose **External Control, Copy OSC Address**.
3. In StagePilot, choose the **Lightkey** preset, add a cue and paste the address.
4. Use **Send test cue** with the same address to check it.

Two-Mac layout: if Lightkey runs on a different Mac than StagePilot, put that Mac's
IP address in **Host or IP address** (the Lightkey preset defaults to `127.0.0.1`,
which only reaches the same computer). Both Macs need to be on the same network.

### Presets and default ports

| Preset | Default port | Source |
| --- | --- | --- |
| Lightkey | 21600 | Lightkey Help, "In Lightkey, control-click a cue, choose External Control > Copy OSC Address. Set the destination port to 21600": https://lightkeyapp.com/en/help and the Lightkey User Guide (Chapter 16, External control, Open Sound Control) |
| QLC+ | 7700 | QLC+ OSC plugin docs: input listens on 7700 plus the universe number minus one (universe 1 is 7700): https://docs.qlcplus.org/v4/plugins/osc |
| Chamsys | 8000 | MagicQ manual, OSC: "A good default is 9000 transmit and 8000 receive" (a suggestion, set under Setup, View Settings, Network): https://secure.chamsys.co.uk/docs/magicq/manual/OSC.html |
| ETC Eos | 8000 | Eos Setup, Show Control: ETC recommends 8000 and 8001 for OSC UDP ports, and the console's RX port is set in Setup: https://www.etcconnect.com/WebDocs/Controls/EosFamilyOnlineHelp/en/Content/07_Setup/01_System/System_Show_Control.htm. Eos prefers TCP (port 3032), which StagePilot does not use |
| GrandMA | none | The grandMA3 manual describes the OSC menu but names no default port: https://help.malighting.com/grandMA3/2.1/HTML/remote_inputs_osc.html. Enter the port set in the console's OSC menu |
| Resolume | none | Resolume's own OSC page names no input port (https://resolume.com/support/en/osc). Third-party guides say 7000, which is not an official source, so it is left empty. Enter the port in Resolume Preferences, OSC |
| Custom | none | Enter what your app expects |

The Chamsys, ETC Eos and QLC+ ports are the receiving ports those apps document;
they can be changed in each app, so check the value in the app you are sending to.

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
