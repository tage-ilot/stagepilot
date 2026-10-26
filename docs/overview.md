# How StagePilot thinks

Most of StagePilot is simple: connect your tools, open the dashboard, run the service. Three ideas trip people up, though. This page explains each one with a picture first and the details after. For setup steps, use the [README](../README.md).

Think of StagePilot as a **stage manager**. The printed program (Planning Center) says what is supposed to happen. The band (Playback) says what is happening right now. The stage manager matches the two, then calls the cues for the screens and the lights.

```mermaid
flowchart LR
  PC["<b>Planning Center</b><br/>The printed program<br/>what should happen"]
  PB["<b>Playback</b><br/>The band<br/>what is happening now"]
  SP(("<b>StagePilot</b><br/>The stage manager"))
  PP["<b>ProPresenter</b><br/>Countdown on the screens"]
  LK["<b>Lightkey</b><br/>The lights"]

  PC -->|"the plan"| SP
  PB -->|"a song just started"| SP
  SP -->|"start a countdown"| PP
  SP -->|"fire lighting cues"| LK
```

Each connection is independent. If ProPresenter is offline, the dashboard, Planning Center and lights keep working, and you can test one connection without the rest.

---

## 1. How does StagePilot know which song is playing?

Playback and Planning Center each have their own song list, and **they don't share song IDs**. StagePilot can't match "Song B" by name. It matches by **position**: the third song in one list is paired with the third song in the other.

That works until the two lists disagree. Maybe Playback doesn't have a song that is in the plan, or the plan has a spoken item between songs.

```mermaid
flowchart LR
  subgraph plan ["Planning Center plan"]
    direction TB
    P1["1. Opening Song"]
    P2["2. Welcome video<br/>(no Playback track)"]
    P3["3. Second Song"]
    P4["4. Closing Song"]
  end
  subgraph pb ["Playback setlist"]
    direction TB
    B1["1. Opening Song"]
    B2["2. Second Song"]
    B3["3. Closing Song"]
  end

  P1 <-->|"paired"| B1
  P2 -. "skipped on the plan side" .-> X(("no partner"))
  P3 <-->|"paired"| B2
  P4 <-->|"paired"| B3
```

This is what **Song arrangement** in the Playback panel is for. Skip an item on either side and the rest pair up in order. Nothing is ever reordered, and skips are saved with **Save configuration**.

> [!IMPORTANT]
> If StagePilot isn't sure the two lists line up, it refuses to start a song rather than guess. Starting the wrong countdown in the middle of a service is worse than not starting one.

### Two ways to listen, and only one at a time

StagePilot can hear "a song started" in two ways. You choose with **Connection method** in the **MIDI / Playback** panel.

```mermaid
flowchart TD
  CHOICE{"Connection method<br/>(you choose)"}

  CHOICE -->|"Playback API (default)"| API["StagePilot connects to Playback<br/>over the network and follows it directly.<br/>No cues to build."]
  CHOICE -->|"MIDI"| MIDI["Playback sends a MIDI note at the start of<br/>each song. You build one cue per song<br/>inside Playback."]

  API --> NOTE1["MIDI messages are ignored,<br/>even if they arrive"]
  MIDI --> NOTE2["The network connection is not started"]

  API -.- NF["There is no automatic fallback<br/>between the two"]
  MIDI -.- NF
```

> [!TIP]
> If you aren't sure which to use, use the default, **Playback API**. You don't have to create any cues in Playback, and StagePilot reconnects by itself if Playback restarts.

#### In MIDI mode, the velocity is the song number

MIDI mode has one more unusual idea. Every song uses the **same note** (channel 1, note 112 by default). The **velocity** of that note says which song it is.

```mermaid
flowchart LR
  subgraph cues ["Cues you create in Playback"]
    C1["Song 1 cue<br/>note 112, velocity 1"]
    C2["Song 2 cue<br/>note 112, velocity 2"]
    C3["Song 3 cue<br/>note 112, velocity 3"]
  end
  C3 --> SP["StagePilot looks up<br/>the 3rd song in the loaded plan"]
  SP --> GO["Starts that song's countdown"]
```

Velocities 1–99 are song numbers. Velocities 100–105 are reserved for actions such as Start next, Restart, Previous, Next, Reload plan and Stop timer. If you send velocity 12 and the plan only has 8 songs, StagePilot rejects it instead of starting something else.

---

## 2. What happens when a song starts?

One trigger sets three things moving: the ProPresenter countdown, StagePilot's own clocks, and the lighting timeline. They don't all start at the same instant, and the order matters.

```mermaid
sequenceDiagram
  autonumber
  participant IN as Playback or a dashboard button
  participant SP as StagePilot
  participant PP as ProPresenter
  participant LK as Lightkey

  IN->>SP: Song 3 started
  Note over SP: Look up Song 3 in the plan and read its duration (for example 4:30)

  rect rgb(240, 245, 255)
    Note over SP,PP: Four steps, every time
    SP->>PP: 1. Stop the timer
    SP->>PP: 2. Set it to count down 4:30
    SP->>PP: 3. Reset it
    SP->>PP: 4. Start it
    PP-->>SP: Started OK
  end

  Note over SP: Only now does StagePilot start its own clocks
  SP->>SP: Dashboard countdown 4:30 and up-counter 0:00
  SP-)LK: Lighting cues at 0:00, 1:12, 3:30 and so on, each at its own elapsed time
```

Why four steps? ProPresenter has **one reusable timer**. StagePilot re-points it at each song instead of creating timers. Resetting first makes sure no leftover time from the previous song can show up.

### Common surprises

| What you see | What is going on |
| --- | --- |
| The lights never started, but the countdown did | Lighting only starts after ProPresenter confirms the timer started. If that confirmation didn't arrive, there's no lighting timeline. |
| A restart made the lights "jump back" | Restart cancels every remaining cue from the old timeline and creates a fresh one at 0:00. This is on purpose. |

---

## 3. Lighting: where cues come from, and which ones fire

Lighting causes the most "wait, why?" moments. It has three ways to **make** a cue, two ways to **send** one, and a status light that doesn't mean what you'd expect.

### Making cues

```mermaid
flowchart LR
  subgraph make ["Three ways to make a cue"]
    A["<b>Type it</b><br/>Add a row: time, address or note"]
    B["<b>Tap a pad</b> while the song plays<br/>Fires now and saves at this moment"]
    C["<b>Record from Lightkey</b><br/>Run the lights by hand once,<br/>StagePilot remembers it"]
  end

  A --> TL[("This song's<br/>lighting timeline")]
  B --> TL
  C --> TL

  TL -->|"song starts"| FIRE{"Which method<br/>is saved?"}
  FIRE -->|"OSC"| OSC["Sends OSC over UDP"]
  FIRE -->|"MIDI"| MIDI["Sends MIDI notes"]
  OSC --> LK(("Lightkey"))
  MIDI --> LK
```

Ways to think about it:

- **A pad is a reusable button.** Press it mid-song and it both fires the cue and writes it into this song's timeline at the current time, so you can program lights by playing along. Switch on **Preview only** to test pads without saving anything.
- **Record and replay** (optional, needs the `lightkey-api` agent on the Lightkey Mac) captures cues, colors and mover positions from a live run and plays them back on later runs. Replay only touches what was recorded.

### Only one method fires, never both

> [!NOTE]
> If OSC is saved, your MIDI cues are switched off, and the other way around. Nothing is deleted. The inactive method's cues are kept and come back when you switch. Changing the method cancels any running timeline, and the new one takes effect at the next song start.

### "Sent" is not "Connected"

```mermaid
flowchart LR
  subgraph handshake ["Has a handshake"]
    H1["Planning Center<br/>MIDI port<br/>ProPresenter"] --> H2["StagePilot can verify it<br/>and shows <b>Connected</b>"]
  end
  subgraph none ["No handshake (OSC over UDP)"]
    N1["Lighting over OSC"] --> N2["StagePilot fires and moves on.<br/>It shows <b>Sent</b>, never Connected"]
  end
```

UDP has no reply, so StagePilot can't know whether Lightkey received a cue. It does report problems it can see, such as a host name that can't be found. So **a green "Sent" means "we sent it", not "it worked"**. Use **Send test cue** to check it end to end before the service.

---

## Go deeper

| I want to... | Read |
| --- | --- |
| Install and set up | [README](../README.md) |
| Connect Playback over the network | [Playback API](playback-api.md) |
| Set up ProPresenter and the timer | [ProPresenter](propresenter.md) |
| Set up Lightkey, pads, record and replay | [Lighting](lights.md) |
| Look up a setting or MIDI velocity | [Configuration](configuration.md) |
