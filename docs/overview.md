# StagePilot at a glance

StagePilot is one dashboard for the moving parts of a live service. It reads the plan, listens for cues, keeps the countdown in sync and fires lighting.

## What goes in, what comes out

```mermaid
flowchart LR
  PC["Planning Center<br/>service plan"] --> SP
  PB["MultiTracks Playback<br/>MIDI cue"] --> SP
  MAN["Dashboard buttons<br/>Manual Controls"] --> SP

  SP(("StagePilot"))

  SP --> PP["ProPresenter<br/>countdown timer"]
  SP --> LK["Lightkey<br/>MIDI cues"]
  SP --> DASH["Dashboard<br/>status and events"]
```

Each connection is independent. You can set up and test one without the others being online.

## One song, start to finish

```mermaid
sequenceDiagram
  autonumber
  participant PB as Playback
  participant SP as StagePilot
  participant PC as Planning Center
  participant PP as ProPresenter
  participant LK as Lightkey

  Note over SP,PC: On open, load today's plan (or the next upcoming one)
  SP->>PC: Fetch service plan
  PC-->>SP: Songs, order, durations

  PB->>SP: MIDI note 112, velocity 2
  SP->>SP: Velocity 2 = second song in the plan
  SP->>PP: Set countdown to song duration and start
  SP->>LK: Send scheduled MIDI cue
  SP-->>SP: Update dashboard (now playing, next)
```

## Which cue does what

The default cue is MIDI channel 1, note 112. The **velocity** picks the action.

```mermaid
flowchart TD
  CUE["Note-on, channel 1, note 112"] --> V{"Velocity?"}
  V -->|"1 to 99"| SONG["Start that song<br/>(position in the plan)"]
  V -->|"100"| A1["Start next"]
  V -->|"101"| A2["Restart current"]
  V -->|"102"| A3["Previous"]
  V -->|"103"| A4["Next"]
  V -->|"104"| A5["Reload plan"]
  V -->|"105"| A6["Stop timer"]
```

## Which service plan loads

```mermaid
flowchart TD
  OPEN["StagePilot opens"] --> TODAY{"Plan with a service<br/>time today?"}
  TODAY -->|"Exactly one"| LOAD["Load it"]
  TODAY -->|"More than one"| ASK["Ask the operator to pick"]
  TODAY -->|"None"| NEXT{"Upcoming plan within<br/>30 days?"}
  NEXT -->|"One on the earliest date"| LOAD
  NEXT -->|"Several on that date"| ASK
  NEXT -->|"None"| NF["Not found"]
```

StagePilot never picks silently between equally plausible plans.

## How the inside is wired

Inputs never call outputs directly. They publish events, and anything that cares reacts. A failure in one integration cannot take down the others.

```mermaid
flowchart TB
  subgraph in [Input plugins]
    P1[planning_center]
    P2[midi_playback]
  end
  subgraph core [Core]
    BUS{{Typed event bus}}
    STATE[(Application state)]
  end
  subgraph out [Output plugins]
    P3[propresenter]
    P4[lights]
  end
  subgraph clients [Clients]
    API[REST + WebSocket]
    UI[React dashboard / Tauri app]
  end

  P1 -->|plan.loaded| BUS
  P2 -->|song.started| BUS
  BUS --> P3
  BUS --> P4
  BUS --> STATE
  STATE --> API --> UI
  UI -->|commands| API
  API -->|same actions as MIDI| BUS
```

## Connection health

```mermaid
stateDiagram-v2
  [*] --> disconnected
  disconnected --> connected: real request, port open or API probe succeeds
  connected --> disconnected: connection lost
  disconnected --> disconnected: demo, simulated or unconfigured
```

"Connected" means verified, never assumed. Demo mode never reports a live connection.

## Where to read more

| I want to... | Read |
| --- | --- |
| Install and set up | [README](../README.md) |
| Write a plugin or understand the design | [plugins](plugins.md) |
| Control lighting | [lights](lights.md) |
| Lock things down | [security](security.md) |
