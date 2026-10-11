# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Volunteer and staff production operators at live events, mainly church
services: the person running the show from a laptop or phone while it is
happening, often with little technical training and no time to look things up.
A secondary audience is the person who sets StagePilot up once and then
maintains it.

## Product Purpose

StagePilot keeps a live service on time. It reads the day's service plan,
follows the song that is playing, shows a countdown, and sends cues to lighting
and presentation software, so the operator can see at a glance that everything
is connected and correct. Success is an operator who runs a whole service
without opening a manual.

## Positioning

A single status-first control surface that sits between the planning, playback,
presentation and lighting tools a team already uses. It works with many
products of each kind, not only the defaults the maintainer uses.

## Operating Context

Runs on a computer in the venue (desktop app) and is reached from phones and
other computers on the local network or remotely. Used in dim rooms, under time
pressure, with a live show running. A wrong tap can change what the audience
sees or what the lights do.

## Capabilities and Constraints

- Four connections plus the app itself: Services, Playback, Presentation,
  Lights, and StagePilot.
- Playback can connect by the Playback API (main) or MIDI (alternate); Lights
  connect by MIDI.
- Changes that could alter what a running show sends are held until the
  operator presses Save configuration; other changes save as they are made.
- Terminology is fixed in DESIGN.md (Vocabulary). Vendor and trademarked names
  appear only where signing in to, or an API specific to, that company needs
  them.
- Service icons are generic illustrations, not vendor logos.
- Demo mode must keep working without vendor accounts or equipment.

## Brand Commitments

Name: StagePilot, set in the arcade-style wordmark with a black outline. An
8-bit-meets-film-flare look with a modern baseline is the owner's stated
direction; the rules for it live in DESIGN.md ("Our look").

## Evidence on Hand

Real screens and source in this repository. No customer testimonials, usage
statistics or benchmarks exist; do not invent any.

## Product Principles

1. Anyone can use it without training. Controls are where the user expects
   them, labels say what will happen, and options fit the moment.
2. Status first: the state of every connection is visible without opening
   anything.
3. Never leave the operator doubting: always show whether a change is saved,
   waiting, or failed.
4. One word for one thing, everywhere.
5. Protect the live show: risky changes are deliberate, routine ones are quick.

## Accessibility & Inclusion

Text contrast at least 4.5:1, 44px touch targets on touch devices, rock-solid
mouse-hover and touch behaviour (the stated priority), visible focus and the
existing keyboard and screen-reader support kept intact, and reduced-motion
support. Status is never
conveyed by colour alone.
