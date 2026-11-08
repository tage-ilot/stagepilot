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

The **Lighting card** on the dashboard follows both halves. It says **Connected** while the Lightkey helper is
connected (capture) and the OSC address is open (replay). It says **Ready** while only the replay address is
available and the helper is not connected yet; sending a cue does not change that, because UDP never confirms
delivery. It goes back to Ready if the helper drops, and to Connected again when it returns.

The strip has these controls:

- **Address and Connect.** Defaults to the Lighting OSC host on port 8770
  (`10.30.0.211:8770`). The dot shows *Connected*, *Lightkey is closed*, or
  *Not connected* with the reason. If the answer is "refused", add the wired
  interface to `allow_interfaces` in the agent's config.
- **Record from Lightkey.** While on, the next song's countdown is recorded:
  every fixture change (color, intensity, pan and tilt, gobo, zoom and more) is
  kept with its song time, measured from Lightkey's own DMX output with the
  clock difference between the two Macs removed. A recording finishes when the song
  ends **by itself** (the song clock reaches its scheduled length plus 5 s, so a
  fade-out after the last cue is kept; the ProPresenter timer deliberately keeps
  counting into overrun, so no stop event is needed or expected), when Record is turned
  off mid-song, when a new song starts, on a stop within 2 s of the song's length, or
  when StagePilot shuts down (saved as incomplete; the wait is capped at 3 s so
  quitting is never held up). The changes are fitted with ramps and effects (see
  Smooth capture), saved into the song's lighting cue map with the normal, serialized save, and the raw
  changes are discarded. **Every capture with at least one fixture change is saved,
  however short;** one with no change is not (`Not saved: no fixture changes ...`), and
  neither is a song whose length is unknown. **A pause never ends or saves a
  recording**, however late in the song: resuming in place carries on. After the
  capture has finished, later changes in the overrun are not recorded and do not
  start another pass.
  The last change of a fixture after the song's scheduled
  end is folded into the final row at the song's length (a fade-out is kept), an oversize change never discards the capture, and moving the
  playhead during a recording does not discard it (see Auto mode). Messages: `Recording <song>.`,
  `Saved N looks (and M loops) to <song>'s timeline.` (`... (fitted loosely: tolerance x2 ...)`
  when the song was too large for the caps and was fitted at a looser tolerance),
  followed by `Incomplete capture: X of Y seconds recorded; it captures again next time.` when
  more than 10 s of the song were missing, or `(N s of the song were not captured)` after the
  save message of a complete capture that has gaps.
- **Replay | Record toggle.** One two-way control. *Replay* (the default) replays
  the captured lights: saved rows play through the normal timeline scheduler (see
  below). *Record* is for capturing and needs a connected link. Switching back to
  Replay stops the recording first, then turns replay on; if that fails the toggle
  shows the real state and the reason. Replay is paused automatically while Record
  is selected. There is no replay-off switch in the UI; if a settings file or the
  API has `replay` off, the strip says "Replay is off" with a **Turn Replay on**
  button instead of looking as if it were replaying.

### Auto mode (per song)

The strip control is **Auto | Replay | Record**. Replay and Record pin the mode for every
song; Auto decides when each song's timer starts and never changes its mind mid-song:

| Connection (link connected, Lightkey running, fixture stream live, Lighting on; 60 s grace) | Song | Decision |
| --- | --- | --- |
| no | any | replay ("Auto: replaying (no connection)") |
| yes | not fully captured, incomplete, or manual-only | capture ("Auto: capturing this song") |
| yes | fully captured | replay ("Auto: replaying (fully captured)") |

A pass is *fully captured* when the **total** of the song it recorded is at least its
length minus 10 s (a 10 s margin, not a percentage; on a song shorter than 20 s, at least
half of it) and it has at least one lighting change. **Where the missing seconds are does
not matter**: a link or fixture-stream drop of any length, a pause, a small seek, a
restart and a late start are all just gaps. Only seconds when the timer ran and the
connection (API linked, Lightkey running, fixture stream live) was live count as
recorded; the stretches recorded are kept as a set, so a stretch recorded twice counts
once. Anything else with a change is saved as
**Incomplete capture** (the Songs strip shows "Incomplete capture"; a complete one shows
"Fully captured"), however short, and the strip says `Incomplete capture: X of Y
seconds recorded; it captures again next time.` (X is the total recorded). A complete one
with gaps says `Saved N looks to <song>'s timeline (6 s of the song were not captured).`
`capture.coverage_seconds` is that total, so a pass replaces a stored
partial only if it covers at least as much, and never replaces a complete capture. A pass
with no change is not saved and the song stays not captured. A song whose length is
unknown cannot be judged and is not saved (unchanged).

Moving the playhead no longer discards a pass. Seeking forward leaves the skipped stretch
as a gap and the pass goes on. Seeking back, or restarting the song, records the same
stretch again and **the latest recording of a stretch wins**: the earlier changes inside it
are dropped and the stretch counts once. Every stretch after a gap (link drop, seek, resume
after a pause) starts with a snapshot of the current value of every fixture property, so a
value that changed during the gap is right from the first moment of the stretch, and replay
holds the last value across the gap (samples more than 0.5 s apart are never joined). A
pause is not a gap in song time (the clock stops); changes made while paused are caught up
by the snapshot taken at the resume. The explicit **Reset position** still discards the pass and
Record off finishes and saves it. Older cue maps whose captured rows have no status count as
incomplete and are captured again. To re-capture a fully captured song, pin Record or
Clear it. An old settings file with no `mode` loads as Auto; `replay` stays the master replay switch (`false` replays nothing in any mode). Record is set through the recording switch and is not saved by the settings route.

Saving replaces only that song's `source = captured` rows. Rows you added by
hand stay. Captured rows are labelled `Look - N fixtures` and have no colour.
An old `lightkey-takes.json` is **ignored**: it is not read, rewritten or deleted
(delete it by hand once you have re-recorded).

### Smooth capture: ramps and effects

A recording is fitted, not thinned. Lightkey's DMX output arrives as samples 2 to 25 ms
apart; a song is 50,000 to 110,000 of them. The fit turns them into a few hundred timeline
items that replay with smooth motion: straight-line **ramps** ("this changes over N
seconds") and **effects** (a repeating pattern, stored once as a loop). The old Fine /
Normal / Coarse **Detail** no longer changes anything: `lights.lightkey.detail` (the API's
`detail_level`, or `detail`) is still accepted, stored and returned exactly as before so an
older client and saved settings keep working, but the fit ignores it and the status
messages no longer mention it.

**Rows with a fade.** A look row may carry `fade_seconds` (default 0, left out of the
saved file while 0, so every row saved before this change loads and saves back
unchanged). Every message in the row reaches its value over `fade_seconds`, starting at
the row's `at_seconds`, in a straight line from that address's value just before the row.
Options, text, a changed argument count and an address with no earlier value step at the
row's time. A row that starts while an earlier ramp of the same address is still running
continues from where that ramp is (no jump). Rows at the same time apply instant ones
first, then ramps. Row times and fades are stored to the millisecond (hand-made instant
rows stay on the 0.1 s grid).

**The ramp fit.** Per address (`/fixture/<name>/overrides/<property>`), the samples are fitted
with a greedy piecewise-linear line through the samples:

| Property | Largest error |
|---|---|
| level (dimmer, zoom, focus...) and each colour channel | 3 % (0.03 of the 0..1 range) |
| angle (pan, tilt, ...Angle, rotation) | 1.5 degrees |

A ramp is at most 60 s long. Samples more than 0.5 s apart are never joined: the value is
held and then steps ("hold then step"), so an idle stretch never becomes a fake ramp.
Wheels, options, text and bools never interpolate; they step when they change. A segment
under 0.1 s whose change is inside the tolerance is an instant step; a short but larger move
keeps its true short fade so the error bound holds. A flat stretch makes no row (the next
ramp starts from the held value). Values are stored to 4 decimals.

**Grouping.** Ramps and steps of one fixture that start within 20 ms of each other and last
the same time share one row (a colour and a dimmer that fade together are one row); rows of
different fixtures that are exactly identical (a chase at zero offset) share one row too.
The row is labelled `Look - N fixtures`.

**Effects.** Most of a busy song repeats: a mover's pan sweep is a triangle wave, a pixel
bar pulses with a period of 4 s, and a chase is the same loop at different phase offsets.
Such a window is stored as ONE item in the song's `effects` list:

```json
{"id": "...", "label": "Loop - 4 fixtures, 8 s", "start_seconds": 104.7,
 "end_seconds": 255.0, "period_seconds": 8.0, "source": "captured",
 "members": [{"fixture": "Z3"}, {"fixture": "Z2", "offset_seconds": 2.628}],
 "cycle": [{"at_seconds": 0.0, "property": "panAngle", "value": [-58.2], "fade_seconds": 4.0},
           {"at_seconds": 4.0, "property": "panAngle", "value": [41.0], "fade_seconds": 4.0}]}
```

Inside `[start_seconds, end_seconds)` every member plays the `cycle` at phase
`(t - start - offset) mod period`, and it overrides the rows for the properties named in
`cycle`; outside the window the rows apply. Each cycle step ramps to its `value` over
`fade_seconds`, from the value the previous step of the same property ended on (the first
step starts from the last step's value, so the loop closes). Effects with a different
shape are separate effects. An effect list is optional: older cue maps have none and load
unchanged.

Detection (`lightkey_effects.rs`):

1. Every numeric address is resampled to 50 ms and compared with itself `T` earlier for `T`
   from 1 to 30 s: a point matches when the two values are within twice the tolerance and
   the value moved by at least four tolerances during that period. The longest window where
   90 % of the points match and which holds at least three cycles is the loop; the
   fundamental period wins over its multiples. Up to three windows per address, so a loop
   that stops and starts again is found twice. The period is refined to the millisecond.
2. Properties of one fixture with the same period are joined; fixtures with the same
   properties, a period within 2 % and overlapping windows are clustered, each fixture's
   phase offset found by sliding its series against the first one's and refined on the raw
   samples. The period is refined on the cluster as a whole (the value that lines up the
   cycles of all members best).
3. The cycle is the per-phase median of all members and all cycles, fitted with ramps at
   half the tolerance and closed into a loop.
4. **Acceptance by reconstruction.** The effect is evaluated with the same evaluator the
   replay uses and compared with the raw samples of every member at 25 ms. At least
   98 % of the window must be within twice the tolerance for every member. If not, the
   window is narrowed to its best stretch (the start and the end of a pattern are usually
   not the loop) and tried again; members that still do not fit leave and are tried on
   their own; the stretches cut off are tried again as windows of their own. An effect must
   also be worth it: its cycle steps must be at most 60 % of the ramp segments the same
   window needs as plain rows.
5. Whatever is not accepted stays plain ramps for that window, so the rest of the fit is
   exact (see below).

False-positive guards: a flat stretch never matches; a slow monotone fade does not match
itself one period earlier (it either moved too little to count as moving or too far to be
within tolerance), so it is never an effect; a window under three cycles is never an
effect; two overlapping loops on one address match no period at 90 % and stay ramps; an
effect whose period drifts is cut by the acceptance into windows that hold or falls back
to ramps.

**Reconstruction metric.** `cargo run --release -p stagepilot-lights --example smooth_report -- takes.json`
rebuilds every address at 25 ms from the saved rows and effects (with
`lightkey_curve::Timeline`, the evaluator the replay is built on) and compares it with the
recorded samples (linear between samples at most 0.5 s apart, held otherwise). It prints,
per property class (angle, dimmer, colour, other level), the maximum error, the 99th
percentile and the share of time over the tolerance and over twice the tolerance, outside
and inside effects, plus every effect (period, window, members, phase offsets), the item
counts and the JSON size before and after, and which series stay plain ramps and why.
Acceptance: outside effects the largest error is at most the tolerance; overall the 99th
percentile is at most twice the tolerance. Over the two real takes (below) both hold; inside an
effect up to 2 % of the moments can be further off (the report prints the maximum).

**Caps.** `fade_seconds` 0 to 600 s (a captured ramp is at most 60 s). Per song: 1,000
rows and 15,000 messages as before, **200 effects** (a busy rig has a handful per group, a
song changing its loops many times stays under it), 256 members per effect, 1,024 cycle
steps per effect and **20,000 cycle steps** per song, a period of 0.1 to 600 s, at most 8
numbers per step. A song that would still pass a cap is fitted again at twice the tolerance,
up to six times (`Saved 120 looks and 4 loops to <song>'s timeline (fitted loosely:
tolerance x2, the song was very large).`); the capture is never dropped for size.

Real takes (two songs of 255 s and 290 s, 109,353 and 50,800 raw samples; the take
files are not in the repository). All numbers come from the report tool:

| | Unstoppable God (255 s) | Able (290 s) |
|---|---|---|
| Raw samples | 109,361 | 50,832 |
| Before (thin + merge at 0.5 s) | 439 rows, 7,398 messages, 1,124,595 bytes | 387 rows, 3,631 messages, 431,855 bytes |
| After (ramps + effects) | 537 rows (1,205 messages) + 9 effects (62 cycle steps) = 546 items, 257,437 bytes | 303 rows (799 messages) + 9 effects (54 cycle steps) = 312 items, 139,345 bytes |
| Max error outside effects (angle / colour / dimmer) | 1.486 deg / 0.030 / 0.030 | 1.443 deg / 0.008 / 0.030 |
| p99 error overall (angle / colour / dimmer) | 2.05 deg / 0.022 / 0.026 | 0.47 deg / 0.005 / 0.030 |
| Inside effects, points beyond 2x tolerance (angle / colour / dimmer) | 1.56 % / 0.68 % / 1.23 % | 1.71 % / n/a / 0.59 % |
| Plain-ramp sanity (script numbers reproduced) | 8,322 segments, longest 4.0 s | 2,727 segments, longest 3.2 s |

**Replay.** The next section ("Smooth replay") says how a captured song plays back. Real
playback on a Lightkey rig is not verified yet.

**Editing.** Rows and effects are separate lists. Saving a song's cue map from the
timeline (the cue-map `PUT`) changes the rows and keeps the effects the song already has: a
row edit, a deleted row or a merge never changes an effect. A new recording replaces the
captured rows AND the captured effects (rows and effects added by hand stay). Rows keep
their `fade_seconds` through every save; editing a row's time moves where its ramp starts
and leaves the fade alone.

### Smooth replay: ramps, effects and the packet budget

Replay plays a song's rows and effects with the same evaluator the capture report uses
(`lightkey_curve::Timeline`): the value of every fixture property at song position `p` is a
pure function of the rows and effects. Nothing is accumulated, so **seek, resume and a
re-sync are all the same thing: evaluate at the new `p`**, send what differs from what was
last sent, and carry on. Pause, stop, reset and the end of the song stop the engine and
**clear nothing** (the held-fixture rules above apply unchanged).

**Which songs.** A song with at least one row with a `fade_seconds` above 0, or with an
effect, is played by the engine. A song captured before smooth capture (no fades, no
effects) is played exactly as before: one packet per row message at the row's time less
45 ms, paced, nothing in between (a test with an old-style map pins this).

**One scheduler tick** drives everything. There is no task per fixture. While nothing moves
the engine sleeps until the next row, ramp end, effect start or end or instant step of an
effect (it does not tick at all when none is left). Per tick, for every address:

- A **step** (an instant row, the end of a ramp, the first look after a start, seek or
  take-back, an effect starting or ending, an instant step inside an effect's cycle) is sent
  at once, at its exact time (the same 45 ms lead as a look row), whatever the budget.
- A running **ramp** is looked at every 50 ms (20 Hz) and sent only when its value moved by
  at least **one DMX step of its property** since the last send: 1/255 for levels and
  colours (8 bit), 540/65535 degrees (about 0.008) for angles (16 bit). A 60 s dimmer ramp
  from 0 to 1 is therefore about 240 packets, not 1,200.
- An address driven by an **effect** is looked at every `effect interval` (50 ms, up to
  125 ms = 8 Hz), with the same one-step rule.
- A value that is still moving is sent as it will be half a tick later (the middle of the
  time it will be held), and **half a DMX step higher**, because Lightkey truncates to the
  DMX resolution. A value at rest is sent exactly.

**Budget.** At most **200 packets per second** leave (the existing 5 ms pacing). A token
bucket (200 per second, burst 10) decides what a tick may send: steps always go out (they
only use the bucket up, and the pacing spaces them), then ramps, then effect values, the
largest change first. When effect values had to wait, the effect interval grows towards
125 ms (8 Hz) and shrinks back when the budget has room; ramps keep 20 Hz. Beyond that a
busy second simply sends the biggest changes first.

**Pacing queue.** Between the ticks and the 5 ms spacing sits one priority queue: **steps
leave first, then ramp values, then effect values, oldest first inside a class**. A ramp or
effect value that is still waiting when a newer value for the same address is queued is
**replaced** by it (only the latest value of an address matters and a stale value is never
sent after a newer one; the replacement keeps the place of the value it replaces). Two steps
of one address are both kept, in order. The tick runs on time and does not wait for the queue
to drain, and the sender looks at the queue again at every 5 ms slot, so a step that arrives
behind a long run of effect values goes out in the next slot. What the queue cannot remove is
the spacing among steps themselves: a look change that touches 98 values takes 98 slots,
485 ms, however it is ordered (the `replay_budget` report prints the burst sizes and the wait
the queue adds beyond that floor: at most 6 / 20 ms on the two real takes). One OSC message per address: **no OSC
bundles and no pattern addresses** (`P{1,3,5}`), because neither is shown to work with
Lightkey's fixture overrides; the rig test (S4) decides.

**Step bursts start before their row.** A look change can be 60 to 100 values at one row
time, and `(n - 1) x 5 ms` (490 ms for 99 values) is a floor no queue order lowers. The
schedule is known in advance, so a burst that would take **longer than 40 ms** (10 or more
values; `STEP_BURST_MIN_MS`) is **started early and spread so that its middle lands on the row
time**: it starts `(n - 1) x 5 ms / 2` before the row (122 ms for 50 values, 242 ms for 98),
never more than `STEP_LEAD_MAX_MS` = **300 ms** (an engine can be given another value with
`with_step_lead_max_ms`, 0 = off; a per-song setting is meant to use that later; no UI).
Single steps and small bursts keep exact timing. Rules:
- *Order inside a burst* (fixed): dimmer / intensity, then colour, then other levels and
  options, then angles (pan, tilt, ...); by address inside a class. A step burst of several
  rows is sent in row order.
- *Never* before the song position of the tick (so not before position 0, and a resume or a
  seek, which builds the engine at the new position, never sends values of rows already
  past), *never* before the previous burst has left the pacing (a later row never overtakes
  an earlier one; rows closer together than a burst takes are late rather than early), and a
  small row right before a big burst that would start before it is sent together with that
  burst. A position that goes back on the same engine forgets what was sent early.
- The fixtures of an early burst are **held** at the row's values until the row time: their
  ramp and effect values in between are superseded by the step that follows. A fixture the
  operator takes over while its value waits is skipped (it takes no slot, nobody waits for it).
- *Budget*: a burst's steps are not charged to the token bucket, and while steps drain,
  **every 4th slot goes to a waiting ramp value** (a burst takes at most 3 of 4 slots, so ramps
  keep 50 packets a second instead of standing still; a burst next to running ramps is planned
  a third longer for that). Ramps also compete **by their distance in tolerances, not DMX steps**
  (an angle has 184 DMX steps per tolerance, a level 8): in a busy second the angles no longer
  take the whole budget from the dimmers.

*Why earliness up to a few hundred milliseconds is acceptable:* a look change is read by the
eye over 100 ms or more, a fixture's own dimmer and the haze take longer than that to show it,
and only part of a big look is early: the first values of the burst (the dimmers and colours
by the order above) lead by at most the lead, the last ones trail by the same amount. *The
price:* during a big look change some fixtures change slightly before the beat and others
slightly after it, spread symmetrically around it; with `STEP_LEAD_MAX_MS` = 0 all of them
trail it by up to 490 ms. The first look of a song (position 0) cannot start early: nothing
exists before position 0.

**Effects** play from the cycle template. A member sees the cycle at phase
`(p - start - offset) mod period`, computed from `p` directly each time (exact period
arithmetic, no accumulated drift over any number of cycles; a test compares values 240
periods apart to 1e-9). **Seams:** the window of a detected effect is only approximate, so
the rows just before it and the effect's first value can differ, and so can the effect's
last value and the rows after it. Playback (`Timeline::playback_at`) therefore moves
linearly between the rows' value and the effect's value over the first and the last 0.5 s
of the window (at the end, towards the value the rows have when the window is over). On the
two real takes the capture alone left 31 and 30 seams that jump more than the recorded motion
does by over a tolerance (up to 41x); with the blend all 150 seams are continuous.

**Operator take-over.** The engine's sends go through the same path as a row's, so the echo
rule of "The operator always wins, per fixture" sees every interpolated value: the same
fixture and property with the same value (within 2 %, or 1 degree) within about 200 ms is
the replay's own echo, however fast the property moves. A fixture the operator touched stops
being driven (its addresses are skipped, nothing is cleared). After 30 seconds without an
operator change the engine forgets what it last sent to that fixture and its next tick sends
the **interpolated value for that moment**, then carries on with the ramp or effect. While
the song is paused it waits for the resume or seek, as before.

**Packet budget report.** `cargo run --release -p stagepilot-lights --example replay_budget
-- <takes.json> [song]` replays a take on a simulated clock (no sockets) and prints packets
per second (mean, 99th percentile, max, seconds above 150 per second), the split into steps,
ramp and effect values, the share sent at the lowered effect rate, how long a step waits in
the pacing queue, the **lateness of every step value against its row** (negative = early:
median / p99 / max, the share later than 25 ms, the share earlier than 100 ms, with and
without the first look of the song, the worst rows), the ramp values that had to wait for
budget, the sent values against the raw recorded motion (inside and outside effects, both
"as intended" and "as paced", with where the maximum is), the seams, and the CPU time.
`BURST_LEAD_MAX_MS=<ms>` overrides `STEP_LEAD_MAX_MS` for the run (0 = no lead).
`BUDGET_TRACE=<address>,<from>,<to>` also prints that address's packets and timeline.

| | Unstoppable God (255 s) | Able (290 s) |
|---|---|---|
| Packets | 20,128 | 15,484 |
| Packets per second: mean / p99 / max | 78.6 / 200 / 200 | 53.2 / 146 / 200 |
| Seconds above 150 per second | 8 | 3 |
| Steps / ramp values / effect values | 398 / 4,455 / 15,275 | 453 / 3,851 / 11,180 |
| Effect values sent at the lowered rate (below 20 Hz) | 98.0 % | 53.5 % |
| Step wait behind the pacing queue: median / p99 / max | 15 / 270 / 290 ms | 15 / 460 / 485 ms |
| Max deviation from the raw motion outside effects (angle / colour / dimmer), as intended | 2.87 deg / 0.124 / 0.122 | 24.4 deg / 0.035 / 0.084 |
| Share of points beyond the tolerance outside effects (angle / colour / dimmer) | 0.04 % / 2.09 % / 0.23 % | 0.07 % / 0.16 % / 0.17 % |
| Inside effects, beyond the tolerance (angle / colour / dimmer) | 11.4 % / 57.7 % / 52.7 % | 7.5 % / n/a / 8.9 % |
| CPU time of the simulated run | 54 ms (0.21 ms per song second) | 61 ms (0.21 ms per song second) |

With step bursts started early (this section's numbers replace the step rows above where
they differ; same two takes):

| | Unstoppable God (255 s) | Able (290 s) |
|---|---|---|
| Step lateness against the row, without the first look of the song: median / p99 / max | 2 / 35 / 45 ms | 5 / 35 / 40 ms |
| ... the same with the lead off (`BURST_LEAD_MAX_MS=0`) | 11 / 102 / 117 ms | 10 / 80 / 95 ms |
| Earliest step value (limit `STEP_LEAD_MAX_MS` = 300) | 70 ms early | 50 ms early |
| First look of the song (cannot start early): values / latest | 59 / 290 ms | 98 / 485 ms |
| Seconds above 150 packets per second | 8 | 2 |
| Ramp values that waited for budget / seconds with one waiting | 4,051 / 10.9 s | 2,074 / 4.2 s |
| Max dimmer deviation outside effects, as intended | 0.137 | 0.057 |

The large "outside effects" angle maximum of Able is one grid point at the start of a 0.3 s,
55 degree zoom ramp: 20 Hz cannot follow a ramp that short. Inside effects the deviation
is the price of the budget: Unstoppable God holds 20 fixtures' colour pulses plus pan sweeps
at 8 Hz for most of the song.

### Simplify this song (backend)

`POST /api/v1/lights/lightkey/simplify` `{"song_key": "...", "apply": false}` runs the older
thin + merge step (0.5 s grid; `detail_level` is accepted and ignored) over the song's
captured instant rows and answers `rows_before`, `rows_after`, `messages_before`,
`messages_after`. It exists for songs captured before ramps. Without `apply` nothing is
changed. With `apply: true` the new rows are saved through the serialized cue-map save.
Manual rows, MIDI cues and captured rows that are not fixture overrides are never touched.
**A song that has any row with a `fade_seconds` or any effect is left exactly as it is**
(answered as nothing to simplify): a ramp starts from the value the row before it left, so
thinning the rows around it, or around the rows an effect hands over to, would move it.
Nothing is saved when the result would not be smaller or would pass the per-song caps.

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

Size of the older thin-and-merge rows (still used by Simplify for songs captured before
ramps) on synthetic data shaped like real takes (25 ms steps, mostly colour, pan and
dimmer sweeps over 255 s):

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

### Setting up the Lighting computer

While there is no connected Lighting computer, the Lightkey strip shows **Set up the Lighting computer**. It installs the
`lightkey-api` helper on the Lighting Mac from StagePilot itself: the Mac needs no internet, no GitHub and no SSH. A
connected helper older than the one StagePilot carries gets the same box as **Update the helper**.

1. On the Lighting Mac, turn on sACN output in Lightkey and open your project.
2. In StagePilot press **Set up the Lighting computer**, then **Copy**, open Terminal on the Lighting Mac and paste.
3. Wait in StagePilot. The strip changes to `Connected` by itself and the command stops working.

**What the command does.** `curl -fsS http://<this computer>:<port>/lightkey-helper/<token>/install.sh | sh` fetches a
short generated `sh` script. It downloads the five bundled helper files (`install.sh`, `lightkey_api.py`,
`lightkey_cues.py`, `uninstall.sh`, `SHA256SUMS`) from the same address into a temporary folder, checks each against
checksums written inside the script (and against `SHA256SUMS`), and stops with a message if any file differs. It then
runs the helper's own installer there with `--allow-for <this computer's address>` and, when the saved Lighting address is a
local IP address, `--allow-address <that address>`. The installer then opens the network interface that has the Lighting
address (the one StagePilot connects on), which is not always the one that reaches StagePilot: on a Mac with a wired lighting
network and a second network, the two differ. Without a saved local address it uses the interface that reaches StagePilot. It prints `Done. Go back to StagePilot.` and removes the temporary folder.

**What it needs.** `curl`, `sh` and Python 3 on the Lighting Mac. Python comes with Apple's command-line tools; if
Terminal says it is missing, run `xcode-select --install` first. The helper starts through a small signed app named "Lightkey API" (the installer builds it with the Mac's own developer tools), so macOS
can ask about it by name: it may ask to let Lightkey API use your local network and read your Documents and Downloads folders so the helper
can find your Lightkey project. Click Allow each time; the Lighting Mac's screen must be unlocked for the questions to appear. If you click Don't Allow, run the same command again and macOS asks once more.

**LAN only, and it expires.** Each press creates a setup session: a random 128-bit token held in memory only (never
written to disk or logs), valid for 30 minutes (pressing again extends it), ended when the helper connects or when you
press Cancel. Only addresses on a private network (192.168.x.x, 10.x.x.x, 172.16-31.x.x, link-local, loopback) can fetch
anything; a token serves at most 30 requests; an unknown or expired token answers `404` with no body. The files are
never served through Remote access. With LAN access on in StagePilot the files are served on the normal port;
otherwise StagePilot opens a temporary listener bound to the one address of this computer that routes to the Lighting
host (not every address), on a random port, and closes it when the session ends. If StagePilot cannot find that
address (`Could not find this computer's address on the Lighting network`) the box asks you to type it.

**The helper that is carried.** The helper files are compiled into StagePilot
(`crates/stagepilot-lights/assets/lightkey-helper/`, refreshed with `node scripts/vendor-lightkey-helper.mjs <tag>`),
so the installer always matches the StagePilot that offers it.

**Removing it.** The helper's `uninstall.sh` is one of the five served files. Press **Set up the Lighting computer** to
start a session, then in Terminal on the Lighting Mac run
`curl -fsS http://<this computer>:<port>/lightkey-helper/<token>/files/uninstall.sh | sh` (the address is in the
command the box shows; add `-s -- --purge` after `sh` to delete the helper's settings too). It stops the helper's
LaunchAgent and removes the helper; Cancel the session afterwards.

### Finding the Lighting computer

While nothing is connected, StagePilot looks for the Lighting computer by itself, so a Lighting Mac on the same
computer or on a network this computer can reach connects without typing its address. It needs nothing new on the
Lighting Mac: a helper already installed (0.4.x or 0.5.0) is found as it is.

**Automatic or chosen.** The address is **chosen** when `lightkey.api_url` is set (you typed an address and pressed
Connect). It is **Automatic** when that is blank. In Automatic the address in use is the last Lighting computer the
search connected to (`lightkey.found_url`, empty until one is found), else the Replay host on port 8770 as before. A
restart therefore tries the last one first. A chosen address is never edited by the search: if it answers, nothing is
searched and nothing is said.

**When it searches.** Only while nothing is connected, and only with Lighting on and set to OSC (never for MIDI, never
with Lighting off). The first search starts about 5 s after StagePilot starts or after a setting changes (so the saved
address gets its chance first), then every 60 s while still not connected, and at once when you press **Search again**.
It stops the moment the link connects and never runs twice at the same time. A Lighting computer that answers but has
Lightkey closed counts as reachable: nothing is searched.

**What is asked, and what never is.** One question per address: `GET /health`, once, 600 ms in total, at most 64 at a
time, no redirects, the answer read up to 64 KB and nothing else ever sent. Only these addresses, and only private ones
(10.x.x.x, 172.16-31.x.x, 192.168.x.x), are asked, at most 1,100 per search:

1. `127.0.0.1` (the helper on this very computer);
2. the saved Lighting address, the saved Replay host and the last found computer, and the /24 around each (this is what
   finds a Lighting Mac on a routed network: StagePilot on 10.20.0.x, the Lighting Mac on 10.30.0.211 behind a router);
3. every network this computer is directly attached to (each up, non-loopback interface with its real prefix; a network
   larger than /22 is cut to the /24 around this computer's address), without this computer's own addresses.

Public addresses, link-local (169.254.x.x) and other loopback addresses are never contacted. The port is the one of the
saved Lighting address, else 8770. An answer counts as a **Lighting computer** when it is HTTP 200 JSON with `ok: true`
and a true/false `lightkey` (Lightkey running); anything else is ignored. HTTP 403 with
`{"error": "not available on this interface"}` is a computer that is there but **refusing** StagePilot's network.

**What you see** (in the Lightkey strip; nothing here is an error colour, and the setup box stays under it):

| Situation | Text and buttons |
|---|---|
| Looking | `Looking for a Lighting computer on this network…` |
| Automatic, exactly one found | connects by itself and saves it: `Found a Lighting computer at <ip> and connected.` |
| Automatic, several found | connects to none: `Found <n> Lighting computers. Choose one:`, a row each `<ip> (Lightkey running)` or `<ip> (Lightkey not running)`, button `Use this one` |
| Chosen address down, another found | nothing switches: `<chosen> isn't answering. A Lighting computer answered at <ip>. Use it?` with `Use <ip>` and `Keep waiting` |
| Nothing found | `No Lighting computer found on this network.` and `Search again` |
| A refusing computer was seen | `A Lighting computer at <ip> is refusing StagePilot. Run the setup command on it again.` |
| Chosen address connected | nothing |

`Keep waiting` silences the offer for that address until StagePilot restarts or the chosen address changes. `Use <ip>`
saves it as the chosen address. **Replay host:** a found computer fills the Replay host only if it is blank; if it is
set and differs, the strip adds `Capture is from <ip>; Replay sends to <host>.` and changes nothing. The search never
edits a chosen address or a set Replay host by itself.

**API.** `GET /api/v1/lights/lightkey` gains `discovery`: `{state: off|searching|connected|choose|offer|none, found:
[{address, version?, lightkey_running, project?}], refusing: [address], chosen?, chosen_down_for_s?, last_scan_at?,
message?, note?}`. `POST /api/v1/lights/lightkey/discovery/scan` is Search again (accepted only while nothing is
connected); `POST .../discovery/use` `{address}` switches to a computer the last search found (any other address is a
plain 422); `POST .../discovery/dismiss` `{address}` is Keep waiting. None of them is on a remote-tunnel allow-list.

**Test hook.** The environment variable `STAGEPILOT_LIGHTKEY_DISCOVERY` can only narrow the search: `off` turns it off,
`loopback:127.0.0.2,127.0.0.3` asks only those loopback addresses (anything else in the list is dropped). The
end-to-end harness (`scripts/lighting-e2e`, stage `discovery`) uses it so that it never probes a real network.

### The Lightkey project, Read fixtures and the helper version

The Lighting section shows which Lightkey project the helper is reading, under the state pill:
`Project: Show.lightkeyproj · 66 fixtures`. The name is the project's base name (never a path) and comes from the
relay's `/relay/hello` (project, fixture count and map id); until a hello arrives the helper's own `GET /health` answer
fills it in, and before that the line says `Project not reported yet`.

- **Helper version and watch state.** On connect and about every 30 s StagePilot reads the helper's `GET /health`
  and keeps `version`, `fixture_map` (project, count, map id) and `watch` (state and a plain sentence) in the status
  (`GET /api/v1/lights/lightkey`, key `helper`). A helper that reports no `version` is older than 0.5.0.
- **Helper update notice.** For a helper older than 0.5.0 the strip says `Helper update available`. Capture and
  replay keep working exactly as before; only the new features below need 0.5.0.
- **Read fixtures** (button in the strip, `POST /api/v1/lights/lightkey/read-fixtures`) asks the helper to read the
  open Lightkey project again, for fixtures added to the same project. The answer is `{ok, message, project, count,
  changed, code?}` and the strip shows `message`: `Read 66 fixtures from Show.lightkeyproj`, `No change: 66 fixtures`,
  or the helper's own plain-language reason (Lightkey closed, no saved project, a read already running, the macOS
  folder permission text, an unreadable project). The button is disabled when not connected and, for a helper
  older than 0.5.0, shows `Update the helper to use this.` The route is served like the other lights routes (same
  host and session checks, no body) and is not on any remote-tunnel allow-list.
- **Captures remember their project.** A saved recording stores `capture.project`, the project name the hello gave
  when the recording began (optional: older captures have none and are never warned about; an rc.5 build ignores the
  extra key and drops it the next time it saves that song).
- **Different project than the song was captured under: a notice, nothing more.** When both the song's stored
  project and the open project are known and differ, `Captured with <X>. Lightkey has <Y> open.` is shown on the
  song's lighting timeline and in the Lightkey strip while that song is current. Replay is never held, skipped or
  stopped because of it: captures name fixtures (B1, D1 ...), so a different project may drive other or missing
  fixtures, and the notice is how you notice.
- **A project change during a recording discards it.** If the hello's map id or project name changes while a
  song is being recorded, the recording is thrown away (a mix of two fixture maps is never saved) with
  `Not saved: the Lightkey project or its fixtures changed during the recording (<old> -> <new>). It captures again
  next time.` Replay is not affected. A hello sent again with the same map id and name is not a change, so the
  helper's repeated hellos and a project saved without changing its fixtures do nothing.

### Wired network

The fixture relay uses plain unicast UDP on port 21700, so it crosses the wired
router. Do not use Wi-Fi. Its timing is not steady enough: the agent's multicast
over Wi-Fi stalled for up to 0.8 s in a test. If the stream stops during a
recording, the message says that levels were not recorded.

### The clock between the two Macs

Each fixture change carries Lightkey's own clock; StagePilot converts it to its own clock
with an offset measured by NTP-style pings (`/relay/ping`, a burst of 12 at the start of a
session and every 120 s). Rules (rc.4; rc.3 threw the estimate away at every burst):

- **Best sample in a window.** Samples of the last 10 minutes are kept and the one with the
  smallest round trip is used. A new burst never clears a good estimate: its samples only
  join the window. A ping that waited in a frozen or stalled agent's buffer comes back with a
  huge round trip (and an offset wrong by half of it), so it can never win against a good
  sample. This was the 6.6 s error of the rig test: the agent was frozen for 20 s while a burst
  was sent, the answers were stamped when the agent woke, and the offset stayed wrong until the
  next burst two minutes later.
- **Poor estimate, ask again soon.** If one second after a burst there is no estimate or the
  best round trip is above 0.2 s (a stalled relay), the burst is repeated after 3 s instead of
  after 120 s.
- **Re-dating.** If a later estimate moves the clock by more than 50 ms, the changes the
  running capture recorded since the previous estimate took effect are moved by the
  difference (log `lightkey_capture_redated`), so a poor start does not stay in the take.
- **Log.** Every sample is logged as `lightkey_clock_offset` (`source` relay or http, `offset`,
  `round_trip`, `used`, `adopted`, `redate`). Every HTTP `/time` measurement is also compared
  with the relay's: a difference of more than 50 ms is logged as
  `lightkey_clock_offset_mismatch`. Search the backend log for these when a take looks
  early or late.

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
lights capture, and with the same rules: a pause leaves a gap, a seek forward leaves a gap and the take goes on, a seek
back (or restart) replaces what was heard in the stretch heard again (the latest take of a slot
wins), resetting discards the take, and it is saved only if the timer ran for at least half the song. While Record is chosen but no song
runs, the input is open only as long as the input picker is on screen (to show its level meter) and closes
a few seconds after you leave. In Replay mode it never opens.

Opening the input can wait for the microphone permission dialog, so it never blocks
anything else: opening and closing run on their own thread, the song start waits at
most 1.5 s for it (loudness then starts when the input opens), the picker answers from
its last listing after 2 s, and repeated picker polls do not start a second open. This also holds
when the dialog is never answered, refused or fails: the open stays on its own thread (a later song
waits the same 1.5 s for it, never longer), Record keeps capturing lights, and the status line says
why there is no loudness. Covered by `an_audio_open_that_never_returns_never_holds_up_any_song`.

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

A song lighting row (`osc_cues`) may carry `messages` (a list of `{address, args}`) and a `source` of `manual` or `captured`. A row sends its `messages` when non-empty, otherwise its single address/args. Caps: `MAX_OSC_MESSAGES_PER_ROW`, `MAX_OSC_ROWS_PER_SONG`, `MAX_OSC_MESSAGES_PER_SONG`. Fixture look rows fire about 45 ms early and are paced to at most 200 packets/s. On start, resume or seek, the latest `/fixture/<name>/overrides/<prop>` message at or before the position is re-sent once (a song with ramps or effects is evaluated at the position instead, see "Smooth replay"). Pause, stop, end and reset hold the last look; fixtures are released (double `overrides/clear`) only when replay is switched off, Lighting is disabled, on shutdown, or when the next song does not use them.

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
- **Fewer items.** Tick two or more rows (or Shift/Ctrl-click markers, drag across the empty track, or long-press a marker on touch) and press **Merge rows**: one row at the earliest time, latest value per address; it asks first if a value would change at an earlier time. Ramps merge only with ramps of the same length that start together and move different properties, and never with instant rows (the button is disabled and says why). There is no Simplify action in the app any more: a recording is already fitted, and the backend route (`POST /api/v1/lights/lightkey/simplify`) stays for old clients. Merge and Remove fixture each have one step of Undo until the next edit.
- **Ramps and loops.** A row with `fade_seconds` is drawn as a bar from its start to start + duration (at least 4 px), the row list shows its duration (`+3.2 s`) and edits it (0 to 60 s, ending inside the song). A song's loops are bands on lanes under the timeline (at most three) and one expandable row each; selecting one shows its period, window, fixtures with phase offsets, properties and a one-cycle sparkline. A loop is never edited: removing it asks first and sends `remove_effect_ids` with the cue-map save (the only way a save changes loops).
- **Density.** Markers closer than 14 px collapse into one numbered cluster (click, Enter or tap zooms in); the Zoom control goes up to 120 px/s. The cue list shows 40 rows per page; picking a marker jumps to its page.
- The Lightkey strip reads "N captured items (R ramps/steps, E loops) on this song"; there is no Detail setting (the API still accepts `detail_level` and ignores it); the old list of takes is gone.

## Clearing cues

**Clear song...** (the selected song, in the timeline toolbar), **Clear all songs...** and the **x** on a song chip (see
"Lighting screen layout" below), and **Clear loudness**, all ask first in an in-app
dialog. It says the cues will be **permanently erased** and cannot be undone, names the song (or "every song in
<plan>"), and counts what goes: looks and hand-made cues (loudness for Clear loudness). The focus starts on **Keep
cues**; **Erase permanently** is the only button that erases. Esc, a click outside the dialog and **Keep cues** leave
everything as it was.

## Header layout

The connection-method toggle (Playback API | MIDI, OSC | MIDI) sits in the middle of the panel header, between the
title and the status pill and close button, whenever the panel is at least 57 rem (912 px) wide. That is the width at
which the longest right-hand group ("MIDI · Alternate", Connected, close) still fits beside it. It is in the same place
on the Playback API, Playback MIDI and Lighting screens. Narrower, it moves to its own row under the title.

## Lighting screen layout

### sACN DMX Live Link (one line)

In OSC mode the connection is one bordered block, **sACN DMX Live Link**. Its header has the title and one **state
pill** (`Connected · 66 fixtures`, `Not connected`, `Connecting…`; the reason for a failure is the pill's tooltip). Under it
is the project line (see "The Lightkey project, Read fixtures and the helper version"), then a field row that wraps cleanly on narrow screens: **one host or IP box**, a small **OSC UDP** port box (default 21600),
a small **API** port box (default 8770), **Connect / Check again**, and at the right the labelled **Mode** (Auto | Replay |
Record) toggle. A quiet strip below has one line of state text (for example "Holding 12 fixtures ·
1 yielded to the operator"), **Release lights**, **Read fixtures**, **Send a test cue** and **How to find a cue's address**. The Lighting
computer that receives OSC and the one that runs `lightkey-api` are always the same machine, so the host is shared.

Saved settings are unchanged: `osc_host` and `osc_port` are saved with the Save bar as before, and `lightkey.api_url`
is saved by Connect. The screen writes `api_url` as `<host>:<API port>`, and leaves it **empty** when the API port is
8770 and the host is the saved OSC host, so the backend derives it as it always did. If a saved `api_url` names a
different host than `osc_host`, a one-line notice names that host and offers **Use the OSC host**. StagePilot keeps
using the saved address until you accept; nothing is rewritten silently. Host and port range messages are unchanged.
MIDI mode is unchanged apart from the shared header.

### Send a test cue

The test cue is a collapsed **Send a test cue** disclosure under the connection (OSC and MIDI alike), no longer part of
the timeline section. It behaves as before. Its **OSC address** field is the cue's OSC *path* (for example
`/live/*/cue/Stars/activate`), not the host.

### Songs strip

Above the timeline, **Songs in this service - N of M captured** shows one chip per plan song, in plan order, with
**Clear all songs...** at the right. A captured song is a small coloured box with its title, its look count (or cue
count in MIDI mode), a note glyph when it has loudness, and an **x** that opens the permanent-erase dialog for that song
only. A plan song with nothing saved is greyed with a dashed outline and says "not captured". Clicking a chip selects
that song in the timeline exactly like the **Song** menu (a hand pick; **Follow live** returns). The song that is
playing has a green dot, the selected chip an outline. The chips look small but their buttons are 44 px tall, and the
list scrolls inside a fixed height, so 40 songs do not push the page around.

### Song lighting timeline section

One toolbar row when wide, wrapping on narrow screens, with every control 34 px tall (44 px on touch): Song menu and
the following-live status, Add cue, Clear song..., Merge rows, Undo, then at the right Zoom (- / Fit / +) and
timeline **Height** (S / M / Auto). The type-filter chips are on their own compact line, right aligned. The selected
cue's details are one compact strip under the timeline. The **Cue board** folds away; it starts open and remembers
how you left it on this device.

### Timeline height

**Auto** (the default) sizes the timeline from the number of stacked marker labels at the current zoom and clustering:
about 200 px with one or two rows, growing by 22 px per row up to 420 px, so finer recordings and more crowded zooms
get taller. **S** (200 px) and **M** (320 px) are fixed and kept on this device. Labels sit in the top rows. The
loudness waveform and the playhead start below the last label row, so they never draw over a label pill. The selected
marker gets a thicker bar, a ring around its label and a raised label. The first ruler label starts at the left edge and
the last one is right-aligned to the end; labels near the end are shortened to fit the track.
