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

## Why MIDI Clock is not used for `mm:ss`

Playback MIDI Clock carries musical tempo pulses while tracks are playing. It
does not provide an absolute song timestamp, so counting its pulses would be
more complex and less reliable for a seconds-based timeline. StagePilot uses a
local monotonic clock for `mm:ss` cues. MIDI Clock can be added later as a
separate optional input for beat- or bar-based lighting effects without changing
the seconds-based scheduler.
